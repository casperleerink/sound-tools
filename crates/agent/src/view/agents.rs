//! The agents of the app: each provider's program found on the machine, and how far it is set
//! up. One for the app, shared by the Settings page, which sets them up, and every sidebar,
//! which runs a thread on one that is ready.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{AppContext as _, Context, Entity, EventEmitter, Task};
use smol::channel::Sender;
use smol::future;

use super::onboarding::{Reason, Setup, SetupAction, SigningIn};
use crate::environment::put_first_on_path;
use crate::install::{self, InstallError};
use crate::settings::AgentSettings;
use crate::{
    Account, Installed, Provider, Service, SignInChoice, SignInStep, login_shell_environment,
    program_on_path,
};

/// How long the browser may take to sign in before the app gives up and asks again.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How long the program may take to say whether it is signed in.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Agents {
    /// Where downloaded programs are kept, `agents/` in the support folder. `None` when the
    /// machine has no support folder; a program named in the environment still runs.
    downloads: Option<PathBuf>,
    /// Each provider found, once the login shell answered, in [`Provider::ALL`] order.
    programs: Vec<Program>,
    /// Reads the login shell and looks for the programs.
    finding: Option<Task<()>>,
    /// The login shell's, once it answered, to look for a program installed since.
    environment: Option<HashMap<OsString, OsString>>,
    /// See [`Agents::new`].
    first_on_path: Option<PathBuf>,
    /// Where each provider's models are listed, its default first.
    settings: Entity<AgentSettings>,
}

/// Something every sidebar shows in its thread, as it happens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentsEvent {
    Notice(String),
}

impl EventEmitter<AgentsEvent> for Agents {}

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
    signing_in: Option<SignInChoice>,
    /// The service of Pi being signed out, by its id.
    signing_out: Option<String>,
    /// Hands the composer's answers to the running sign-in.
    answers: Option<Sender<(String, String)>>,
    /// The download, the sign-in or a question to the program. Dropping it cancels it.
    task: Option<Task<()>>,
    /// Whether it is still signed in, after a failed turn. Apart from `task`, which may run a
    /// sign-in meanwhile.
    rechecking: Option<Task<()>>,
}

impl Program {
    fn new(provider: Provider, installed: Option<Installed>, setup: Setup) -> Self {
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
}

/// What the login shell found of each provider.
struct Found {
    programs: Vec<(Provider, Option<PathBuf>, bool)>,
    environment: HashMap<OsString, OsString>,
    error: Option<io::Error>,
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
        settings: Entity<AgentSettings>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut agents = Agents {
            downloads,
            programs: Vec::new(),
            finding: None,
            environment: None,
            first_on_path,
            settings,
        };
        agents.find(cx);
        agents
    }

    /// For a test or a snapshot: each provider set up and signed in with its program, or
    /// Claude Code not installed with none. Nothing runs in the background.
    pub fn with_programs(
        programs: Vec<(Provider, Installed)>,
        settings: Entity<AgentSettings>,
    ) -> Self {
        let mut programs: Vec<Program> = (programs.into_iter())
            .map(|(provider, installed)| {
                let account = Account::default();
                let ready = Setup::Ready {
                    account,
                    reason: None,
                };
                Program::new(provider, Some(installed), ready)
            })
            .collect();
        if programs.is_empty() {
            programs.push(Program::new(Provider::Claude, None, Setup::NotInstalled));
        }
        Agents {
            downloads: None,
            programs,
            finding: None,
            environment: None,
            first_on_path: None,
            settings,
        }
    }

    /// For a snapshot: Pi's services, as a check would list them.
    pub fn with_services(mut self, services: Vec<Service>) -> Self {
        if let Some(program) = self.program_mut(Provider::Pi) {
            program.services = services;
        }
        self
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

    fn find(&mut self, cx: &mut Context<Self>) {
        let downloads = self.downloads.clone();
        let first_on_path = self.first_on_path.clone();
        self.finding = Some(cx.spawn(async move |agents, cx| {
            let found = cx
                .background_spawn(async move {
                    let (mut environment, error) = match login_shell_environment().await {
                        Ok(environment) => (environment, None),
                        Err(error) => (std::env::vars_os().collect(), Some(error)),
                    };
                    let programs = Provider::ALL.map(|provider| {
                        let downloaded = (downloads.clone())
                            .zip(provider.download())
                            .map(|(downloads, download)| download.program(&downloads));
                        let named =
                            std::env::var_os(provider.program_variable()).map(PathBuf::from);
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
                    Found {
                        programs: programs.to_vec(),
                        environment,
                        error,
                    }
                })
                .await;
            // An app that quit in the meantime has nobody to tell.
            agents.update(cx, |agents, cx| agents.found(found, cx)).ok();
        }));
    }

    fn found(&mut self, found: Found, cx: &mut Context<Self>) {
        self.finding = None;
        if let Some(error) = found.error {
            let notice = format!("The agent runs without the settings of your shell: {error}.");
            cx.emit(AgentsEvent::Notice(notice));
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
                self.check(provider, None, false, cx);
            }
        }
        cx.notify();
    }

    fn set_setup(&mut self, provider: Provider, setup: Setup, cx: &mut Context<Self>) {
        if let Some(program) = self.program_mut(provider) {
            program.setup = setup;
            cx.notify();
        }
    }

    /// Runs `task` for `provider`, in place of what ran. An older recheck goes too: its word
    /// on the sign-in would be stale once this one has run.
    fn set_task(&mut self, provider: Provider, task: Task<()>) {
        if let Some(program) = self.program_mut(provider) {
            program.task = Some(task);
            program.rechecking = None;
        }
    }

    /// The account the running agent of `provider` reports, the newest word on it.
    pub fn signed_in_as(&mut self, provider: Provider, account: Account, cx: &mut Context<Self>) {
        let Some(Setup::Ready { reason, .. }) =
            self.program(provider).map(|program| &program.setup)
        else {
            return;
        };
        let reason = reason.clone();
        self.set_setup(provider, Setup::Ready { account, reason }, cx);
    }

    fn set_signed_in(&mut self, provider: Provider, signed_in: bool, cx: &mut Context<Self>) {
        if let Some(program) = self.program_mut(provider) {
            program.signed_in = signed_in;
            cx.notify();
        }
    }

    /// What a button of the Settings page asks for. **Continue** is the page's to answer, with
    /// the text of its field ([`Self::answer`]).
    pub fn act(&mut self, provider: Provider, action: SetupAction, cx: &mut Context<Self>) {
        match action {
            SetupAction::Download => self.download(provider, cx),
            SetupAction::Cancel => self.cancel(provider, cx),
            SetupAction::SignIn(choice) => self.sign_in(choice, cx),
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
                    ) => cx.open_url(page),
                    (_, Some(choice)) => self.sign_in(choice, cx),
                    _ => {}
                }
            }
            SetupAction::Pick(id) => self.answer(provider, id, cx),
            SetupAction::Continue => {}
            SetupAction::SignOut(service) => self.sign_out(provider, service, cx),
            SetupAction::Check => self.check(provider, None, false, cx),
            SetupAction::GetProgram => cx.open_url(provider.website()),
            SetupAction::LookAgain => self.look_again(provider, cx),
        }
    }

    /// After a failed turn: the program may have been signed out meanwhile, such as in a
    /// terminal. Only an answer of signed out changes anything; the failed turn says the rest.
    pub fn check_still_signed_in(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(installed) = self.ready(provider) else {
            return;
        };
        let asking = cx.background_spawn(async move { provider.status(&installed).await });
        let task = cx.spawn(async move |agents, cx| {
            // Signed in, or no answer: the next message shows whether the agent runs.
            if let Ok(status) = asking.await
                && status.account.is_none()
            {
                let reason = Some(Reason::new(format!("{} is signed out.", provider.name())));
                (agents.update(cx, |agents, cx| {
                    agents.set_signed_in(provider, false, cx);
                    // A sign-in that runs meanwhile keeps its steps on the page.
                    let shown = agents.program(provider).map(|program| &program.setup);
                    if !matches!(shown, Some(Setup::SigningIn(_))) {
                        agents.set_setup(provider, Setup::SignedOut { reason }, cx);
                    }
                }))
                .ok();
            }
        });
        if let Some(program) = self.program_mut(provider) {
            program.rechecking = Some(task);
        }
    }

    /// Asks the program whether it is signed in, and how it signs in. `reason` is what the
    /// composer reads when it is not, after a sign-in. Signed in after a sign-in that `picks`,
    /// the provider becomes the composer's pick.
    fn check(
        &mut self,
        provider: Provider,
        reason: Option<Reason>,
        picks: bool,
        cx: &mut Context<Self>,
    ) {
        let installed = (self.program(provider)).and_then(|program| program.installed.clone());
        let Some(installed) = installed else {
            self.set_setup(provider, Setup::NotInstalled, cx);
            return;
        };
        // A program signed in shows so until the answer, and its threads go on meanwhile.
        if !self.program(provider).is_some_and(Program::is_ready) {
            self.set_setup(provider, Setup::Checking, cx);
        }
        let asking = cx.background_spawn({
            let installed = installed.clone();
            async move { provider.status(&installed).await }
        });
        let timeout = cx.background_executor().timer(CHECK_TIMEOUT);
        let task = cx.spawn(async move |agents, cx| {
            // The question that loses the race is dropped, which ends its process.
            let answer = future::or(async { Some(asking.await) }, async {
                timeout.await;
                None
            })
            .await;
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
            // An app that quit in the meantime has nobody to tell.
            let told = agents.update(cx, |agents, cx| {
                if let Some(program) = agents.program_mut(provider) {
                    // A program that did not answer keeps the services it listed last.
                    if let Some((choices, services)) = ways {
                        program.choices = choices;
                        program.services = services;
                    }
                    program.signing_out = None;
                    program.signed_in = ready;
                }
                if ready && picks {
                    let settings = agents.settings.clone();
                    settings.update(cx, |settings, cx| settings.set_provider(provider, cx));
                }
                agents.set_setup(provider, setup, cx);
            });
            if told.is_err() || !ready {
                return;
            }
            if let Some(models) = listed {
                (agents.update(cx, |agents, cx| {
                    let settings = agents.settings.clone();
                    settings.update(cx, |settings, cx| settings.set_models(provider, models, cx));
                }))
                .ok();
                return;
            }
            // The menu shows the models before the first message starts an agent. Asked after
            // every check, as another account may offer others. Claude Code lists them from an
            // agent that is never sent a message, in a folder of no project.
            let folder = std::env::temp_dir();
            let listing =
                cx.background_spawn(async move { provider.models(installed, folder).await });
            let timeout = cx.background_executor().timer(CHECK_TIMEOUT);
            let listed = future::or(async { Some(listing.await) }, async {
                timeout.await;
                None
            })
            .await;
            (agents.update(cx, |agents, cx| match listed {
                Some(Ok(models)) => agents
                    .settings
                    .update(cx, |settings, cx| settings.set_models(provider, models, cx)),
                Some(Err(error)) => agents.models_not_listed(provider, &error.to_string(), cx),
                None => {
                    agents.models_not_listed(provider, "it did not answer within 30 seconds", cx)
                }
            }))
            .ok();
        });
        self.set_task(provider, task);
    }

    fn models_not_listed(&mut self, provider: Provider, error: &str, cx: &mut Context<Self>) {
        let name = provider.name();
        cx.emit(AgentsEvent::Notice(format!(
            "{name} did not list its models: {error}"
        )));
    }

    /// Downloads the pinned program, from where an earlier try stopped.
    fn download(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let name = provider.name();
        let (Some(downloads), Some(download)) = (self.downloads.clone(), provider.download())
        else {
            let message = match self.downloads {
                None => format!("There is no folder on this computer to keep {name} in."),
                Some(_) => format!("{name} does not run on this computer."),
            };
            self.set_setup(provider, Setup::DownloadFailed { message }, cx);
            return;
        };
        let size = download.size;
        self.set_setup(provider, Setup::Downloading { received: 0, size }, cx);
        let (sender, receiver) = smol::channel::unbounded();
        let installing = cx.background_spawn(async move {
            install::install(&download, &downloads, move |received| {
                // Closed only when the app quit, and then nobody reads it.
                sender.try_send(received).ok();
            })
            .await
        });
        let task = cx.spawn(async move |agents, cx| {
            // Ends with the download, which drops the sender.
            while let Ok(received) = receiver.recv().await {
                let downloading = Setup::Downloading { received, size };
                let shown =
                    agents.update(cx, |agents, cx| agents.set_setup(provider, downloading, cx));
                if shown.is_err() {
                    return;
                }
            }
            let installed = installing.await;
            (agents.update(cx, |agents, cx| agents.downloaded(provider, installed, cx))).ok();
        });
        self.set_task(provider, task);
    }

    fn downloaded(
        &mut self,
        provider: Provider,
        installed: Result<PathBuf, InstallError>,
        cx: &mut Context<Self>,
    ) {
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
                self.check(provider, None, false, cx);
            }
            Err(error) => {
                let message = error.sentence(provider.name());
                self.set_setup(provider, Setup::DownloadFailed { message }, cx);
            }
        }
    }

    /// Runs the provider's sign-in and waits for it, at most ten minutes. Its steps show as
    /// they come. Started again, it ends the one before.
    fn sign_in(&mut self, choice: SignInChoice, cx: &mut Context<Self>) {
        let provider = choice.provider();
        let installed = (self.program(provider)).and_then(|program| program.installed.clone());
        let Some(installed) = installed else {
            return;
        };
        let (steps, shown) = smol::channel::unbounded();
        let (answers, answered) = smol::channel::unbounded();
        let running = cx.background_spawn({
            let choice = choice.clone();
            async move { choice.run(&installed, steps, answered).await }
        });
        let timeout = cx.background_executor().timer(SIGN_IN_TIMEOUT);
        let task = cx.spawn(async move |agents, cx| {
            let showing = {
                let (agents, mut cx) = (agents.clone(), cx.clone());
                async move {
                    while let Ok(step) = shown.recv().await {
                        let shown = agents.update(&mut cx, |agents, cx| {
                            agents.sign_in_step(provider, step, cx);
                        });
                        if shown.is_err() {
                            break;
                        }
                    }
                    future::pending().await
                }
            };
            // The one that loses the race is dropped: a sign-in out of time ends its process.
            let ended = future::or(
                future::or(
                    async {
                        match running.await {
                            Ok(()) => SignInEnded::Finished,
                            Err(error) => SignInEnded::Failed(error),
                        }
                    },
                    async {
                        timeout.await;
                        SignInEnded::TimedOut
                    },
                ),
                showing,
            )
            .await;
            (agents.update(cx, |agents, cx| agents.signed_in(provider, ended, cx))).ok();
        });
        if let Some(program) = self.program_mut(provider) {
            program.signing_in = Some(choice);
            program.answers = Some(answers);
        }
        self.set_task(provider, task);
        self.set_setup(provider, Setup::SigningIn(SigningIn::default()), cx);
    }

    fn sign_in_step(&mut self, provider: Provider, step: SignInStep, cx: &mut Context<Self>) {
        // The provider waits on its page, which the composer has not seen yet.
        if let SignInStep::Page { url } | SignInStep::Code { url, .. } = &step {
            cx.open_url(url);
        }
        if let Some(Program {
            setup: Setup::SigningIn(signing_in),
            ..
        }) = self.program_mut(provider)
        {
            signing_in.show(step);
            cx.notify();
        }
    }

    /// Answers the question the sign-in of `provider` asked, with a pick or the text of a
    /// field.
    pub fn answer(&mut self, provider: Provider, answer: String, cx: &mut Context<Self>) {
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
        cx.notify();
    }

    fn signed_in(&mut self, provider: Provider, ended: SignInEnded, cx: &mut Context<Self>) {
        if let Some(program) = self.program_mut(provider) {
            program.answers = None;
        }
        // The program says whether it worked, also when it did not: a provider signed in
        // before stays signed in.
        let finished = matches!(ended, SignInEnded::Finished);
        self.check(provider, Some(ended.reason()), finished, cx);
    }

    fn cancel(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(program) = self.program_mut(provider) else {
            return;
        };
        // Dropping the task ends the download or the sign-in, and its process.
        program.task = None;
        program.answers = None;
        match program.setup {
            Setup::Downloading { .. } => self.set_setup(provider, Setup::NotInstalled, cx),
            _ => self.signed_in(provider, SignInEnded::Cancelled, cx),
        }
    }

    /// Signs the program out, for every app that shares its login, or for Pi one service.
    /// A sidebar whose thread runs on it ends its agent once it shows signed out.
    fn sign_out(&mut self, provider: Provider, service: Option<String>, cx: &mut Context<Self>) {
        let Some(installed) = self.ready(provider) else {
            return;
        };
        match &service {
            // Pi stays signed in to its other services meanwhile, and its threads go on.
            Some(service) => {
                if let Some(program) = self.program_mut(provider) {
                    program.signing_out = Some(service.clone());
                    cx.notify();
                }
            }
            None => {
                self.set_signed_in(provider, false, cx);
                self.set_setup(provider, Setup::Checking, cx);
            }
        }
        let signing_out =
            cx.background_spawn(
                async move { provider.sign_out(&installed, service.as_deref()).await },
            );
        let task = cx.spawn(async move |agents, cx| {
            let signed_out = signing_out.await;
            (agents.update(cx, |agents, cx| {
                if let Err(error) = signed_out {
                    cx.emit(AgentsEvent::Notice(format!("Could not sign out: {error}")));
                }
                agents.check(provider, None, false, cx);
            }))
            .ok();
        });
        self.set_task(provider, task);
    }

    /// Looks for a program the composer installed since the app started, on the `PATH` of
    /// the login shell read again: an installer may have added to it. Then asks the program
    /// whether it is signed in.
    fn look_again(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(read_before) = self.environment.clone() else {
            return;
        };
        let first_on_path = self.first_on_path.clone();
        self.set_setup(provider, Setup::Checking, cx);
        let looking = cx.background_spawn(async move {
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
            program.map(|program| Installed {
                program,
                environment,
            })
        });
        let task = cx.spawn(async move |agents, cx| {
            let installed = looking.await;
            (agents.update(cx, |agents, cx| {
                let Some(installed) = installed else {
                    let setup = Setup::NotInstalled;
                    return agents.set_setup(provider, setup, cx);
                };
                if let Some(program) = agents.program_mut(provider) {
                    program.installed = Some(installed);
                }
                agents.check(provider, None, false, cx);
            }))
            .ok();
        });
        self.set_task(provider, task);
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
    use gpui::TestAppContext;

    use super::*;

    /// A sign-in to another of Pi's providers leaves Pi signed in, so its threads go on.
    #[gpui::test]
    fn a_sign_in_leaves_a_signed_in_provider_signed_in(cx: &mut TestAppContext) {
        let installed = Installed {
            program: "/nonexistent/pi".into(),
            environment: HashMap::new(),
        };
        let agents = cx.new(|cx| {
            let settings = cx.new(|cx| AgentSettings::new(None, cx));
            Agents::with_programs(vec![(Provider::Pi, installed)], settings)
        });
        let service = Service::example("OpenRouter", None, None);
        let choice = service.with_key.expect("every example takes a key");
        agents.update(cx, |agents, cx| {
            agents.act(Provider::Pi, SetupAction::SignIn(choice), cx);
            assert!(matches!(
                agents.program(Provider::Pi).map(|program| &program.setup),
                Some(Setup::SigningIn(_))
            ));
            assert!(agents.ready(Provider::Pi).is_some());
        });
    }
}
