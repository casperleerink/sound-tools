//! The agents of the app: each provider's program found on the machine, and how far it is set
//! up. One for the app, shared by the Settings page, which sets them up, and every sidebar,
//! which runs a thread on one that is ready.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{AppContext as _, Context, Entity, EventEmitter, Task};
use smol::future;

use super::onboarding::{Setup, SetupAction};
use crate::environment::put_first_on_path;
use crate::install::{self, InstallError};
use crate::settings::AgentSettings;
use crate::{Account, Installed, Provider, SignInChoice, login_shell_environment, program_on_path};

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
    /// Where the models are listed with the provider's default first.
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
    pub setup: Setup,
    /// The way in of the running sign-in, which **Open the page again** starts again.
    signing_in: Option<SignInChoice>,
    /// The download, the sign-in or a question to the program. Dropping it cancels it.
    task: Option<Task<()>>,
}

impl Program {
    fn new(provider: Provider, installed: Option<Installed>, setup: Setup) -> Self {
        Program {
            provider,
            installed,
            setup,
            signing_in: None,
            task: None,
        }
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.setup, Setup::Ready { .. })
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
            settings,
        };
        agents.find(first_on_path, cx);
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
                Program::new(provider, Some(installed), Setup::Ready { account })
            })
            .collect();
        if programs.is_empty() {
            programs.push(Program::new(Provider::Claude, None, Setup::NotInstalled));
        }
        Agents {
            downloads: None,
            programs,
            finding: None,
            settings,
        }
    }

    pub fn programs(&self) -> &[Program] {
        &self.programs
    }

    pub fn program(&self, provider: Provider) -> Option<&Program> {
        self.programs
            .iter()
            .find(|program| program.provider == provider)
    }

    fn program_mut(&mut self, provider: Provider) -> Option<&mut Program> {
        self.programs
            .iter_mut()
            .find(|program| program.provider == provider)
    }

    /// The program of `provider`, once it is signed in.
    pub fn ready(&self, provider: Provider) -> Option<Installed> {
        let program = self.program(provider)?;
        program.installed.clone().filter(|_| program.is_ready())
    }

    /// Whether the login shell is still read, or every program still asked: nothing to show
    /// but the shape of a thread yet.
    pub fn is_checking(&self) -> bool {
        self.finding.is_some()
            || (self.programs.iter()).all(|program| program.setup == Setup::Checking)
    }

    fn find(&mut self, first_on_path: Option<PathBuf>, cx: &mut Context<Self>) {
        let downloads = self.downloads.clone();
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
        // A provider shows once its program is on the machine, or when it can download it.
        self.programs = (found.programs.into_iter())
            .filter(|(provider, _, present)| *present || provider.download().is_some())
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
            if self
                .program(provider)
                .is_some_and(|program| program.setup == Setup::Checking)
            {
                self.check(provider, None, cx);
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

    fn set_task(&mut self, provider: Provider, task: Task<()>) {
        if let Some(program) = self.program_mut(provider) {
            program.task = Some(task);
        }
    }

    /// The account the running agent of `provider` reports, the newest word on it.
    pub fn signed_in_as(&mut self, provider: Provider, account: Account, cx: &mut Context<Self>) {
        if self.program(provider).is_some_and(Program::is_ready) {
            self.set_setup(provider, Setup::Ready { account }, cx);
        }
    }

    pub fn act(&mut self, provider: Provider, action: SetupAction, cx: &mut Context<Self>) {
        match action {
            SetupAction::Download => self.download(provider, cx),
            SetupAction::Cancel => self.cancel(provider, cx),
            SetupAction::SignIn(choice) => self.sign_in(choice, cx),
            SetupAction::OpenPageAgain => {
                let choice = self
                    .program(provider)
                    .and_then(|program| program.signing_in);
                if let Some(choice) = choice {
                    self.sign_in(choice, cx);
                }
            }
            SetupAction::SignOut => self.sign_out(provider, cx),
            SetupAction::Check => self.check(provider, None, cx),
        }
    }

    /// After a failed turn: the program may have been signed out meanwhile, such as in a
    /// terminal. Only an answer of signed out changes anything; the failed turn says the rest.
    pub fn check_still_signed_in(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(installed) = self.ready(provider) else {
            return;
        };
        let asking = cx.background_spawn(async move { provider.account(&installed).await });
        let task = cx.spawn(async move |agents, cx| {
            // Signed in, or no answer: the next message shows whether the agent runs.
            if let Ok(None) = asking.await {
                let reason = Some(format!("{} is signed out.", provider.name()));
                (agents.update(cx, |agents, cx| {
                    agents.set_setup(provider, Setup::SignedOut { reason }, cx);
                }))
                .ok();
            }
        });
        self.set_task(provider, task);
    }

    /// Asks the program whether it is signed in. `reason` is what the composer reads when it
    /// is not, after a sign-in.
    fn check(&mut self, provider: Provider, reason: Option<String>, cx: &mut Context<Self>) {
        let Some(installed) =
            (self.program(provider)).and_then(|program| program.installed.clone())
        else {
            self.set_setup(provider, Setup::NotInstalled, cx);
            return;
        };
        self.set_setup(provider, Setup::Checking, cx);
        let asking = cx.background_spawn({
            let installed = installed.clone();
            async move { provider.account(&installed).await }
        });
        let timeout = cx.background_executor().timer(CHECK_TIMEOUT);
        let task = cx.spawn(async move |agents, cx| {
            // The question that loses the race is dropped, which ends its process.
            let answer = future::or(async { Some(asking.await) }, async {
                timeout.await;
                None
            })
            .await;
            let setup = match answer {
                Some(Ok(Some(account))) => Setup::Ready { account },
                Some(Ok(None)) => Setup::SignedOut { reason },
                Some(Err(error)) => Setup::Stopped {
                    message: error.to_string(),
                },
                None => Setup::Stopped {
                    message: "it did not answer within 30 seconds.".to_string(),
                },
            };
            let ready = matches!(setup, Setup::Ready { .. });
            // An app that quit in the meantime has nobody to tell.
            let told = agents.update(cx, |agents, cx| agents.set_setup(provider, setup, cx));
            if told.is_err() || !ready {
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
                    .update(cx, |settings, cx| settings.set_models(models, cx)),
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
                if (agents.update(cx, |agents, cx| agents.set_setup(provider, downloading, cx)))
                    .is_err()
                {
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
                self.check(provider, None, cx);
            }
            Err(error) => {
                let message = error.sentence(provider.name());
                self.set_setup(provider, Setup::DownloadFailed { message }, cx);
            }
        }
    }

    /// Runs the provider's sign-in in the browser and waits for it, at most ten minutes.
    /// Started again, it ends the one before.
    fn sign_in(&mut self, choice: SignInChoice, cx: &mut Context<Self>) {
        let provider = choice.provider();
        let Some(installed) =
            (self.program(provider)).and_then(|program| program.installed.clone())
        else {
            return;
        };
        let running = cx.background_spawn(async move { choice.run(&installed).await });
        let timeout = cx.background_executor().timer(SIGN_IN_TIMEOUT);
        let task = cx.spawn(async move |agents, cx| {
            // The one that loses the race is dropped: a sign-in out of time ends its process.
            let ended = future::or(
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
            )
            .await;
            (agents.update(cx, |agents, cx| agents.signed_in(provider, ended, cx))).ok();
        });
        if let Some(program) = self.program_mut(provider) {
            program.signing_in = Some(choice);
            program.task = Some(task);
        }
        self.set_setup(provider, Setup::SigningIn, cx);
    }

    fn signed_in(&mut self, provider: Provider, ended: SignInEnded, cx: &mut Context<Self>) {
        if let Some(program) = self.program_mut(provider) {
            program.signing_in = None;
        }
        let reason = Some(ended.reason());
        match ended {
            // The program says whether it worked.
            SignInEnded::Finished => self.check(provider, reason, cx),
            SignInEnded::Failed(_) | SignInEnded::TimedOut | SignInEnded::Cancelled => {
                self.set_setup(provider, Setup::SignedOut { reason }, cx);
            }
        }
    }

    fn cancel(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(program) = self.program_mut(provider) else {
            return;
        };
        // Dropping the task ends the download or the sign-in, and its process.
        program.task = None;
        program.signing_in = None;
        let setup = match program.setup {
            Setup::Downloading { .. } => Setup::NotInstalled,
            _ => Setup::SignedOut {
                reason: Some(SignInEnded::Cancelled.reason()),
            },
        };
        self.set_setup(provider, setup, cx);
    }

    /// Signs the program out, for every app that shares its login. A sidebar whose thread
    /// runs on it ends its agent once it shows signed out.
    fn sign_out(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(installed) = self.ready(provider) else {
            return;
        };
        self.set_setup(provider, Setup::Checking, cx);
        let signing_out = cx.background_spawn(async move { provider.sign_out(&installed).await });
        let task = cx.spawn(async move |agents, cx| {
            let signed_out = signing_out.await;
            (agents.update(cx, |agents, cx| {
                if let Err(error) = signed_out {
                    cx.emit(AgentsEvent::Notice(format!("Could not sign out: {error}")));
                }
                agents.check(provider, None, cx);
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
    /// The line the composer reads when the sign-in ended with nobody signed in.
    fn reason(&self) -> String {
        match self {
            SignInEnded::Finished => "The sign-in did not finish.".to_string(),
            SignInEnded::Failed(error) => format!("The sign-in failed: {error}"),
            SignInEnded::TimedOut => "The sign-in took over 10 minutes.".to_string(),
            SignInEnded::Cancelled => "The sign-in was cancelled.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_out_of_time_failed_or_cancelled_says_why() {
        let reasons = [
            SignInEnded::TimedOut,
            SignInEnded::Cancelled,
            SignInEnded::Failed(io::Error::other("OAuth error: access denied")),
        ]
        .map(|ended| ended.reason());
        assert_eq!(
            reasons,
            [
                "The sign-in took over 10 minutes.",
                "The sign-in was cancelled.",
                "The sign-in failed: OAuth error: access denied",
            ]
        );
    }
}
