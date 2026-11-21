//! The agents of the app: each provider's program found on the machine, how far it is set up,
//! and the settings the composer chose ([`AgentSettings`]). The Agents section of Settings sets
//! them up ([`Agents::sections`], [`Agents::act`]); the chat runs a thread on one that is ready.
//!
//! Finding, checking, downloading and signing in run in the background. What they found comes
//! in at [`Agents::poll`]: each result names the job it is of, and one of a job that was
//! replaced or cancelled meanwhile is dropped.

use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use smol::channel::Sender;
use smol::future;

use crate::environment::put_first_on_path;
use crate::install::{self, InstallError};
use crate::jobs::{self, Inbox, Job, Outbox, Waker};
use crate::settings::AgentSettings;
use crate::setup::{self, Reason, Section, Setup, SetupAction, SigningIn};
use crate::{
    Account, Answer, Installed, Model, Provider, Service, SignInChoice, SignInStep, Status,
    login_shell_environment, program_on_path,
};

/// How long the browser may take to sign in before the app gives up and asks again.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How long the program may take to say whether it is signed in, or to list its models.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Agents {
    /// Where downloaded programs are kept, `agents/` in the support folder. `None` when the
    /// machine has no support folder; a program named in the environment still runs.
    downloads: Option<PathBuf>,
    /// Each provider found, once the login shell answered, in [`Provider::ALL`] order.
    programs: Vec<Program>,
    /// Reads the login shell and looks for the programs.
    finding: Option<Job>,
    /// The login shell's, once it answered, to look for a program installed since.
    environment: Option<HashMap<OsString, OsString>>,
    /// See [`Agents::new`].
    first_on_path: Option<PathBuf>,
    settings: AgentSettings,
    /// The id of the next job.
    jobs: u64,
    /// For the chat's thread, or for the window while there is none.
    notices: Vec<String>,
    /// Pages the window opens in the browser.
    open_urls: Vec<String>,
    inbox: Inbox<Done>,
}

/// What [`Agents::poll`] has for the window.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Polled {
    /// Lines for the thread, such as a shell that did not answer.
    pub notices: Vec<String>,
    /// Pages to open in the browser, such as a sign-in's.
    pub open_urls: Vec<String>,
}

/// One provider's program and how far it is set up.
pub struct Program {
    pub provider: Provider,
    /// The program and its environment. A download may not be on disk yet.
    pub installed: Option<Installed>,
    /// What the Settings page shows of it.
    pub setup: Setup,
    /// Whether the last check found it signed in: a thread runs on it while it is. A sign-in
    /// to another of Pi's providers, or a check, leaves it as it is.
    signed_in: bool,
    /// Claude Code's ways in, from the last check.
    pub choices: Vec<SignInChoice>,
    /// Pi's services, from the last check.
    pub services: Vec<Service>,
    /// The way in of the running sign-in, which **Open the page again** starts again, or of
    /// the last one, whose end the Settings page shows by its service.
    pub(crate) signing_in: Option<SignInChoice>,
    /// The service of Pi being signed out, by its id.
    signing_out: Option<String>,
    /// Hands the composer's answers to the running sign-in.
    answers: Option<Sender<(String, String)>>,
    /// The download, the sign-in or a question to the program, and its id. Dropping it
    /// cancels it.
    task: Option<(u64, Job)>,
    /// Whether it is still signed in, after a failed turn. Apart from `task`, which may run a
    /// sign-in meanwhile.
    rechecking: Option<(u64, Job)>,
}

impl Program {
    pub(crate) fn new(provider: Provider, installed: Option<Installed>, setup: Setup) -> Self {
        Program {
            provider,
            installed,
            signed_in: matches!(setup, Setup::Ready { .. }),
            setup,
            choices: Vec::new(),
            services: Vec::new(),
            signing_in: None,
            signing_out: None,
            answers: None,
            task: None,
            rechecking: None,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.signed_in
    }

    /// The way in of the running or the last sign-in.
    pub fn signing_in(&self) -> Option<&SignInChoice> {
        self.signing_in.as_ref()
    }

    /// The service of Pi being signed out, by its id.
    pub fn signing_out(&self) -> Option<&str> {
        self.signing_out.as_deref()
    }

    fn is_task(&self, job: u64) -> bool {
        self.task.as_ref().is_some_and(|(id, _)| *id == job)
    }
}

/// What the login shell found of each provider.
struct Found {
    programs: Vec<(Provider, Option<PathBuf>, bool)>,
    environment: HashMap<OsString, OsString>,
    error: Option<io::Error>,
}

/// What a job of one provider found.
enum Done {
    Found(Found),
    Job(Provider, u64, Step),
}

enum Step {
    /// `None`: it did not answer in time.
    Checked {
        answer: Option<io::Result<Status>>,
        reason: Option<Reason>,
        picks: bool,
    },
    Listed(Result<Vec<Model>, String>),
    Received(u64),
    Downloaded(Result<PathBuf, InstallError>),
    SignInStep(SignInStep),
    SignInEnded(SignInEnded),
    SignedOut(io::Result<()>),
    LookedAgain(Option<Installed>),
    /// After a failed turn: it answered signed out.
    SignedOutMeanwhile,
}

impl Agents {
    /// Reads the login shell in the background, then asks each program found whether it is
    /// signed in. A program is the one the provider's environment variable names, else the
    /// composer's own on the login shell's `PATH`, else the download in `downloads`.
    /// `first_on_path` goes first on the agents' `PATH`: the folder of the programs the app
    /// ships, so the agent runs the same ones.
    pub fn new(
        downloads: Option<PathBuf>,
        first_on_path: Option<PathBuf>,
        settings: AgentSettings,
        waker: Waker,
    ) -> Self {
        let mut agents = Self::empty(downloads, first_on_path, settings, waker);
        agents.find();
        agents
    }

    fn empty(
        downloads: Option<PathBuf>,
        first_on_path: Option<PathBuf>,
        settings: AgentSettings,
        waker: Waker,
    ) -> Self {
        Agents {
            downloads,
            programs: Vec::new(),
            finding: None,
            environment: None,
            first_on_path,
            settings,
            jobs: 0,
            notices: Vec::new(),
            open_urls: Vec::new(),
            inbox: Inbox::new(waker),
        }
    }

    /// For a test or a picture: each provider set up and signed in with its program, or
    /// Claude Code not installed with none. Nothing runs in the background.
    pub fn with_programs(
        programs: Vec<(Provider, Installed)>,
        settings: AgentSettings,
        waker: Waker,
    ) -> Self {
        let mut agents = Self::empty(None, None, settings, waker);
        agents.programs = (programs.into_iter())
            .map(|(provider, installed)| {
                let ready = Setup::Ready {
                    account: Account::default(),
                    reason: None,
                };
                Program::new(provider, Some(installed), ready)
            })
            .collect();
        if agents.programs.is_empty() {
            let program = Program::new(Provider::Claude, None, Setup::NotInstalled);
            agents.programs.push(program);
        }
        agents
    }

    /// For a picture: Pi's services, as a check would list them.
    pub fn with_services(mut self, services: Vec<Service>) -> Self {
        if let Some(program) = self.program_mut(Provider::Pi) {
            program.services = services;
        }
        self
    }

    /// For a picture: `provider`, which [`Self::with_programs`] made, at `setup`.
    pub fn with_setup(mut self, provider: Provider, setup: Setup) -> Self {
        if let Some(program) = self.program_mut(provider) {
            program.signed_in = matches!(setup, Setup::Ready { .. });
            program.setup = setup;
        }
        self
    }

    pub fn settings(&self) -> &AgentSettings {
        &self.settings
    }

    /// Reads the settings again, for what another window changed: once this one gains the keys.
    pub fn read_settings_again(&mut self) {
        self.settings.read_again();
    }

    pub(crate) fn settings_mut(&mut self) -> &mut AgentSettings {
        &mut self.settings
    }

    pub fn programs(&self) -> &[Program] {
        &self.programs
    }

    pub fn program(&self, provider: Provider) -> Option<&Program> {
        (self.programs.iter()).find(|program| program.provider == provider)
    }

    fn program_mut(&mut self, provider: Provider) -> Option<&mut Program> {
        (self.programs.iter_mut()).find(|program| program.provider == provider)
    }

    /// The program of `provider`, once it is signed in.
    pub fn ready(&self, provider: Provider) -> Option<Installed> {
        let program = self.program(provider)?;
        program.installed.clone().filter(|_| program.is_ready())
    }

    /// Whether the login shell is still read, or every program still asked: nothing to show
    /// but the shape of a thread yet.
    pub fn is_checking(&self) -> bool {
        self.is_finding()
            || (!self.programs.is_empty()
                && (self.programs.iter()).all(|program| program.setup == Setup::Checking))
    }

    /// Whether the login shell is still read, so no program is known yet.
    pub fn is_finding(&self) -> bool {
        self.finding.is_some()
    }

    /// What the Agents section shows of each provider, with `query` in the search of Pi's
    /// services.
    pub fn sections(&self, query: &str) -> Vec<Section> {
        (self.programs.iter())
            .map(|program| setup::section(program, query))
            .collect()
    }

    /// The question of a sign-in that is answered with text, by its provider and id, and
    /// whether the text is a secret. A new id empties the field and gives it the keys.
    pub fn text_question(&self) -> Option<(Provider, &str, bool)> {
        self.programs.iter().find_map(|program| {
            let Setup::SigningIn(signing_in) = &program.setup else {
                return None;
            };
            let question = signing_in.question.as_ref()?;
            match question.answer {
                Answer::Text { secret } => Some((program.provider, question.id.as_str(), secret)),
                Answer::Pick(_) => None,
            }
        })
    }

    /// What came in from the background since the last frame, and what the window does now.
    pub fn poll(&mut self) -> Polled {
        let mut notices = self.settings.poll();
        while let Some(done) = self.inbox.take() {
            match done {
                Done::Found(found) => self.found(found),
                Done::Job(provider, job, step) => self.step(provider, job, step),
            }
        }
        notices.append(&mut self.notices);
        Polled {
            notices,
            open_urls: std::mem::take(&mut self.open_urls),
        }
    }

    fn find(&mut self) {
        let downloads = self.downloads.clone();
        let first_on_path = self.first_on_path.clone();
        let outbox = self.inbox.outbox();
        self.finding = Some(jobs::spawn(async move {
            let (mut environment, error) = match login_shell_environment().await {
                Ok(environment) => (environment, None),
                Err(error) => (std::env::vars_os().collect(), Some(error)),
            };
            let programs = Provider::ALL.map(|provider| {
                let downloaded = (downloads.clone())
                    .zip(provider.download())
                    .map(|(downloads, download)| download.program(&downloads));
                let named = std::env::var_os(provider.program_variable()).map(PathBuf::from);
                let program = named
                    .or_else(|| program_on_path(provider.command(), &environment))
                    .or(downloaded);
                let present = program.as_ref().is_some_and(|program| program.is_file());
                (provider, program, present)
            });
            // Without it the agent still runs, with only the composer's own programs.
            if let Some(folder) = &first_on_path
                && let Err(error) = put_first_on_path(&mut environment, folder)
            {
                eprintln!("{} is not on the agent's PATH: {error}", folder.display());
            }
            outbox.send(Done::Found(Found {
                programs: programs.to_vec(),
                environment,
                error,
            }));
        }));
    }

    fn found(&mut self, found: Found) {
        self.finding = None;
        if let Some(error) = found.error {
            let notice = format!("The agent runs without the settings of your shell: {error}.");
            self.notices.push(notice);
        }
        self.environment = Some(found.environment.clone());
        // Every provider shows: one not on the machine says how to get it.
        self.programs = (found.programs.into_iter())
            .map(|(provider, program, present)| {
                let installed = program.map(|program| Installed {
                    program,
                    environment: found.environment.clone(),
                });
                let setup = if present {
                    Setup::Checking
                } else {
                    Setup::NotInstalled
                };
                Program::new(provider, installed, setup)
            })
            .collect();
        for provider in Provider::ALL {
            let checking = self.program(provider);
            if checking.is_some_and(|program| program.setup == Setup::Checking) {
                self.check(provider, None, false);
            }
        }
    }

    fn step(&mut self, provider: Provider, job: u64, step: Step) {
        let Some(program) = self.program_mut(provider) else {
            return;
        };
        if let Step::SignedOutMeanwhile = step {
            if program
                .rechecking
                .as_ref()
                .is_some_and(|(id, _)| *id == job)
            {
                program.rechecking = None;
                program.signed_in = false;
                // A sign-in that runs meanwhile keeps its steps on the page.
                if !matches!(program.setup, Setup::SigningIn(_)) {
                    let reason = Some(Reason::new(format!("{} is signed out.", provider.name())));
                    program.setup = Setup::SignedOut { reason };
                }
            }
            return;
        }
        if !program.is_task(job) {
            return;
        }
        match step {
            Step::Checked {
                answer,
                reason,
                picks,
            } => self.checked(provider, answer, reason, picks),
            Step::Listed(Ok(models)) => self.settings.set_models(provider, models),
            Step::Listed(Err(error)) => {
                let name = provider.name();
                self.notices
                    .push(format!("{name} did not list its models: {error}"));
            }
            Step::Received(received) => {
                if let Setup::Downloading { size, .. } = program.setup {
                    program.setup = Setup::Downloading { received, size };
                }
            }
            Step::Downloaded(installed) => self.downloaded(provider, installed),
            Step::SignInStep(step) => self.sign_in_step(provider, step),
            Step::SignInEnded(ended) => self.sign_in_ended(provider, ended),
            Step::SignedOut(signed_out) => {
                if let Err(error) = signed_out {
                    self.notices.push(format!("Could not sign out: {error}"));
                }
                self.check(provider, None, false);
            }
            Step::LookedAgain(None) => self.set_setup(provider, Setup::NotInstalled),
            Step::LookedAgain(Some(installed)) => {
                program.installed = Some(installed);
                self.check(provider, None, false);
            }
            Step::SignedOutMeanwhile => {}
        }
    }

    fn set_setup(&mut self, provider: Provider, setup: Setup) {
        if let Some(program) = self.program_mut(provider) {
            program.setup = setup;
        }
    }

    /// Runs `work` for `provider`, in place of what ran. An older recheck goes too: its word on
    /// the sign-in would be stale once this one has run. `work` hands its steps to the outbox
    /// it is given, under its own id.
    fn run<F>(&mut self, provider: Provider, work: impl FnOnce(JobOutbox) -> F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.jobs += 1;
        let job = self.jobs;
        let outbox = JobOutbox {
            outbox: self.inbox.outbox(),
            provider,
            job,
        };
        let task = jobs::spawn(work(outbox));
        if let Some(program) = self.program_mut(provider) {
            program.task = Some((job, task));
            program.rechecking = None;
        }
    }

    /// The account the running agent of `provider` reports, the newest word on it.
    pub(crate) fn signed_in_as(&mut self, provider: Provider, account: Account) {
        if let Some(Program {
            setup: Setup::Ready { account: shown, .. },
            ..
        }) = self.program_mut(provider)
        {
            *shown = account;
        }
    }

    /// What a button of the Agents section asks for. **Continue** is the view's to answer,
    /// with the text of its field ([`Self::answer`]).
    pub fn act(&mut self, provider: Provider, action: SetupAction) {
        match action {
            SetupAction::Download => self.download(provider),
            SetupAction::Cancel => self.cancel(provider),
            SetupAction::SignIn(choice) => self.sign_in(choice),
            SetupAction::OpenPageAgain => {
                let Some(program) = self.program(provider) else {
                    return;
                };
                match (&program.setup, program.signing_in.clone()) {
                    (
                        Setup::SigningIn(SigningIn {
                            page: Some(page), ..
                        }),
                        _,
                    ) => {
                        let page = page.clone();
                        self.open_url(page);
                    }
                    (_, Some(choice)) => self.sign_in(choice),
                    _ => {}
                }
            }
            SetupAction::Pick(id) => self.answer(provider, id),
            SetupAction::Continue => {}
            SetupAction::SignOut(service) => self.sign_out(provider, service),
            SetupAction::Check => self.check(provider, None, false),
            SetupAction::GetProgram => self.open_url(provider.website().to_string()),
            SetupAction::LookAgain => self.look_again(provider),
        }
    }

    fn open_url(&mut self, url: String) {
        self.open_urls.push(url);
        // Taken on the next frame.
        self.inbox.wake();
    }

    /// After a failed turn: the program may have been signed out meanwhile, such as in a
    /// terminal. Only an answer of signed out changes anything; the failed turn says the rest.
    pub(crate) fn check_still_signed_in(&mut self, provider: Provider) {
        let Some(installed) = self.ready(provider) else {
            return;
        };
        self.jobs += 1;
        let job = self.jobs;
        let outbox = self.inbox.outbox();
        let task = jobs::spawn(async move {
            // Signed in, or no answer: the next message shows whether the agent runs.
            if let Ok(status) = provider.status(&installed).await
                && status.account.is_none()
            {
                outbox.send(Done::Job(provider, job, Step::SignedOutMeanwhile));
            }
        });
        if let Some(program) = self.program_mut(provider) {
            program.rechecking = Some((job, task));
        }
    }

    /// Asks the program whether it is signed in, and how it signs in. `reason` is what the
    /// composer reads when it is not, after a sign-in. Signed in after a sign-in that `picks`,
    /// the provider becomes the composer's pick.
    fn check(&mut self, provider: Provider, reason: Option<Reason>, picks: bool) {
        let installed = (self.program(provider)).and_then(|program| program.installed.clone());
        let Some(installed) = installed else {
            self.set_setup(provider, Setup::NotInstalled);
            return;
        };
        // A program signed in shows so until the answer, and its threads go on meanwhile.
        if !self.program(provider).is_some_and(Program::is_ready) {
            self.set_setup(provider, Setup::Checking);
        }
        self.run(provider, |outbox| async move {
            let answer = jobs::within(CHECK_TIMEOUT, provider.status(&installed)).await;
            outbox.send(Step::Checked {
                answer,
                reason,
                picks,
            });
        });
    }

    fn checked(
        &mut self,
        provider: Provider,
        answer: Option<io::Result<Status>>,
        reason: Option<Reason>,
        picks: bool,
    ) {
        let stopped = |message: String| (Setup::Stopped { message }, None);
        let mut listed = None;
        let (setup, ways) = match answer {
            Some(Ok(status)) => {
                listed = status.models;
                let ways = Some((status.choices, status.services));
                match status.account {
                    // A sign-in that ended without its account still says why.
                    Some(account) => {
                        let reason = reason.filter(|_| !picks);
                        (Setup::Ready { account, reason }, ways)
                    }
                    None => (Setup::SignedOut { reason }, ways),
                }
            }
            Some(Err(error)) => stopped(error.to_string()),
            None => stopped("it did not answer within 30 seconds.".to_string()),
        };
        let ready = matches!(setup, Setup::Ready { .. });
        let Some(program) = self.program_mut(provider) else {
            return;
        };
        // A program that did not answer keeps the services it listed last.
        if let Some((choices, services)) = ways {
            program.choices = choices;
            program.services = services;
        }
        program.signing_out = None;
        program.signed_in = ready;
        program.setup = setup;
        if !ready {
            return;
        }
        if picks {
            self.settings.set_provider(provider);
        }
        if let Some(models) = listed {
            self.settings.set_models(provider, models);
            return;
        }
        let Some(installed) = self.ready(provider) else {
            return;
        };
        // The menu shows the models before the first message starts an agent. Asked after
        // every check, as another account may offer others. Claude Code lists them from an
        // agent that is never sent a message, in a folder of no project.
        self.run(provider, |outbox| async move {
            let folder = std::env::temp_dir();
            let listed = jobs::within(CHECK_TIMEOUT, provider.models(installed, folder)).await;
            let listed = match listed {
                Some(Ok(models)) => Ok(models),
                Some(Err(error)) => Err(error.to_string()),
                None => Err("it did not answer within 30 seconds".to_string()),
            };
            outbox.send(Step::Listed(listed));
        });
    }

    /// Downloads the pinned program, from where an earlier try stopped.
    fn download(&mut self, provider: Provider) {
        let name = provider.name();
        let (Some(downloads), Some(download)) = (self.downloads.clone(), provider.download())
        else {
            let message = match self.downloads {
                None => format!("There is no folder on this computer to keep {name} in."),
                Some(_) => format!("{name} does not run on this computer."),
            };
            self.set_setup(provider, Setup::DownloadFailed { message });
            return;
        };
        let size = download.size;
        self.set_setup(provider, Setup::Downloading { received: 0, size });
        self.run(provider, |outbox| async move {
            let progress = outbox.clone();
            let installed = install::install(&download, &downloads, move |received| {
                progress.send(Step::Received(received));
            })
            .await;
            outbox.send(Step::Downloaded(installed));
        });
    }

    fn downloaded(&mut self, provider: Provider, installed: Result<PathBuf, InstallError>) {
        match installed {
            Ok(program) => {
                if let Some(found) = self.program_mut(provider) {
                    // The login shell answered before **Set up** showed.
                    let environment = found.installed.take().map_or_else(
                        || std::env::vars_os().collect(),
                        |installed| installed.environment,
                    );
                    found.installed = Some(Installed {
                        program,
                        environment,
                    });
                }
                self.check(provider, None, false);
            }
            Err(error) => {
                let message = error.sentence(provider.name());
                self.set_setup(provider, Setup::DownloadFailed { message });
            }
        }
    }

    /// Runs the provider's sign-in and waits for it, at most ten minutes. Its steps show as
    /// they come. Started again, it ends the one before.
    fn sign_in(&mut self, choice: SignInChoice) {
        let provider = choice.provider();
        let installed = (self.program(provider)).and_then(|program| program.installed.clone());
        let Some(installed) = installed else {
            return;
        };
        let (answers, answered) = smol::channel::unbounded();
        let running = choice.clone();
        self.run(provider, |outbox| async move {
            let (steps, shown) = smol::channel::unbounded();
            let showing = async {
                while let Ok(step) = shown.recv().await {
                    outbox.send(Step::SignInStep(step));
                }
                future::pending().await
            };
            let signing_in = async {
                let ran = jobs::within(SIGN_IN_TIMEOUT, running.run(&installed, steps, answered));
                match ran.await {
                    Some(Ok(())) => SignInEnded::Finished,
                    Some(Err(error)) => SignInEnded::Failed(error),
                    None => SignInEnded::TimedOut,
                }
            };
            // The one that loses the race is dropped: a sign-in out of time ends its process.
            let ended = future::or(signing_in, showing).await;
            outbox.send(Step::SignInEnded(ended));
        });
        if let Some(program) = self.program_mut(provider) {
            program.signing_in = Some(choice);
            program.answers = Some(answers);
            program.setup = Setup::SigningIn(SigningIn::default());
        }
    }

    fn sign_in_step(&mut self, provider: Provider, step: SignInStep) {
        // The provider waits on its page, which the composer has not seen yet.
        if let SignInStep::Page { url } | SignInStep::Code { url, .. } = &step {
            self.open_urls.push(url.clone());
        }
        if let Some(Program {
            setup: Setup::SigningIn(signing_in),
            ..
        }) = self.program_mut(provider)
        {
            signing_in.show(step);
        }
    }

    /// Answers the question the sign-in of `provider` asked, with a pick or the text of a
    /// field.
    pub fn answer(&mut self, provider: Provider, answer: String) {
        let Some(Program {
            setup: Setup::SigningIn(signing_in),
            answers: Some(answers),
            ..
        }) = self.program_mut(provider)
        else {
            return;
        };
        let Some(question) = signing_in.question.take() else {
            return;
        };
        // Closed only when the sign-in ended, and then it asks nothing more.
        answers.try_send((question.id, answer)).ok();
    }

    fn sign_in_ended(&mut self, provider: Provider, ended: SignInEnded) {
        if let Some(program) = self.program_mut(provider) {
            program.answers = None;
        }
        // The program says whether it worked, also when it did not: a provider signed in
        // before stays signed in.
        let finished = matches!(ended, SignInEnded::Finished);
        self.check(provider, Some(ended.reason()), finished);
    }

    fn cancel(&mut self, provider: Provider) {
        let Some(program) = self.program_mut(provider) else {
            return;
        };
        // Dropping the task ends the download or the sign-in, and its process.
        program.task = None;
        program.answers = None;
        if matches!(program.setup, Setup::Downloading { .. }) {
            program.setup = Setup::NotInstalled;
        } else {
            self.sign_in_ended(provider, SignInEnded::Cancelled);
        }
    }

    /// Signs the program out, for every app that shares its login, or for Pi one service.
    /// A thread that runs on it ends its agent once it shows signed out.
    fn sign_out(&mut self, provider: Provider, service: Option<String>) {
        let Some(installed) = self.ready(provider) else {
            return;
        };
        if let Some(program) = self.program_mut(provider) {
            match &service {
                // Pi stays signed in to its other services meanwhile, and its threads go on.
                Some(service) => program.signing_out = Some(service.clone()),
                None => {
                    program.signed_in = false;
                    program.setup = Setup::Checking;
                }
            }
        }
        self.run(provider, |outbox| async move {
            let signed_out = provider.sign_out(&installed, service.as_deref()).await;
            outbox.send(Step::SignedOut(signed_out));
        });
    }

    /// Looks for a program the composer installed since the app started, on the `PATH` of
    /// the login shell read again: an installer may have added to it. Then asks the program
    /// whether it is signed in.
    fn look_again(&mut self, provider: Provider) {
        let Some(read_before) = self.environment.clone() else {
            return;
        };
        let first_on_path = self.first_on_path.clone();
        self.set_setup(provider, Setup::Checking);
        self.run(provider, |outbox| async move {
            let environment = match login_shell_environment().await {
                Ok(mut environment) => {
                    // As at the start; without it the agent runs with the composer's programs.
                    if let Some(folder) = &first_on_path {
                        put_first_on_path(&mut environment, folder).ok();
                    }
                    environment
                }
                Err(_) => read_before,
            };
            let program = program_on_path(provider.command(), &environment)
                .filter(|program| program.is_file());
            let installed = program.map(|program| Installed {
                program,
                environment,
            });
            outbox.send(Step::LookedAgain(installed));
        });
    }
}

/// Hands the steps of one job to the inbox, under its provider and id.
struct JobOutbox {
    outbox: Outbox<Done>,
    provider: Provider,
    job: u64,
}

impl Clone for JobOutbox {
    fn clone(&self) -> Self {
        Self {
            outbox: self.outbox.clone(),
            provider: self.provider,
            job: self.job,
        }
    }
}

impl JobOutbox {
    fn send(&self, step: Step) {
        self.outbox.send(Done::Job(self.provider, self.job, step));
    }
}

/// How a sign-in ended.
#[derive(Debug)]
enum SignInEnded {
    /// The program asks whether it worked.
    Finished,
    Failed(io::Error),
    TimedOut,
    Cancelled,
}

impl SignInEnded {
    /// What the composer reads when the sign-in ended with nobody signed in.
    fn reason(&self) -> Reason {
        match self {
            SignInEnded::Finished => Reason::new("The sign-in did not finish."),
            SignInEnded::Failed(error) => Reason {
                details: Some(error.to_string()),
                ..Reason::new("The sign-in did not work. Try again.")
            },
            SignInEnded::TimedOut => Reason::new("The sign-in took over 10 minutes."),
            SignInEnded::Cancelled => Reason::new("The sign-in was cancelled."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::testing;

    fn pi() -> Installed {
        Installed {
            program: "/nonexistent/pi".into(),
            environment: HashMap::new(),
        }
    }

    fn agents(programs: Vec<(Provider, Installed)>) -> (Agents, smol::channel::Receiver<()>) {
        let (waker, woken) = testing::waker();
        let settings = AgentSettings::new(None, waker.clone());
        (Agents::with_programs(programs, settings, waker), woken)
    }

    /// A sign-in to another of Pi's providers leaves Pi signed in, so its threads go on.
    #[test]
    fn a_sign_in_leaves_a_signed_in_provider_signed_in() {
        let (mut agents, _woken) = agents(vec![(Provider::Pi, pi())]);
        let service = Service::example("OpenRouter", None, None);
        let choice = service.with_key.expect("every example takes a key");
        agents.act(Provider::Pi, SetupAction::SignIn(choice));
        assert!(matches!(
            agents.program(Provider::Pi).map(|program| &program.setup),
            Some(Setup::SigningIn(_))
        ));
        assert!(agents.ready(Provider::Pi).is_some());
    }

    /// Cancel ends the sign-in, and what it still sent is no more: the program is asked again.
    #[test]
    fn a_cancelled_sign_in_shows_nothing_more_of_itself() {
        let (mut agents, _woken) = agents(vec![(Provider::Pi, pi())]);
        let choice = Service::example("OpenRouter", None, None)
            .with_key
            .expect("a key");
        agents.act(Provider::Pi, SetupAction::SignIn(choice));
        let Some((job, _)) = agents
            .program(Provider::Pi)
            .and_then(|program| program.task.as_ref())
        else {
            panic!("no sign-in runs");
        };
        let job = *job;
        agents.act(Provider::Pi, SetupAction::Cancel);
        let note = Step::SignInStep(SignInStep::Note("Waiting".into()));
        agents.step(Provider::Pi, job, note);
        let setup = agents
            .program(Provider::Pi)
            .map(|program| program.setup.clone());
        assert_eq!(setup, Some(Setup::SigningIn(SigningIn::default())));
    }

    /// The page the program waits on opens in the browser by itself; **Get Pi** opens Pi's.
    #[test]
    fn pages_go_to_the_window_to_open() {
        let (mut agents, woken) = agents(vec![(Provider::Pi, pi())]);
        agents.act(Provider::Pi, SetupAction::GetProgram);
        testing::wait(&woken);
        let polled = agents.poll();
        assert_eq!(polled.open_urls, [Provider::Pi.website()]);
        assert!(agents.poll().open_urls.is_empty());
    }
}
