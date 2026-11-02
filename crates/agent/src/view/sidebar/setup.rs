//! Setting up the agent's program: finding it, its download, the sign-in and the sign-out,
//! and asking whether it is signed in.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{Context, prelude::*};
use smol::future;

use super::Sidebar;
use crate::environment::put_first_on_path;
use crate::install::{self, InstallError};
use crate::view::onboarding::{Setup, SetupAction};
use crate::{Installed, SignInChoice, login_shell_environment, program_on_path};

/// How long the browser may take to sign in before the sidebar gives up and asks again.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How long the program may take to say whether it is signed in.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

impl Sidebar {
    /// Reads the login shell in the background, then asks the program whether it is signed
    /// in, or offers **Set up** when there is none yet. The program is the one the provider's
    /// environment variable names, else the composer's own on the login shell's `PATH`, else
    /// the download in `agents`. `first_on_path` goes first on the agent's `PATH`: the folder
    /// of the programs the app ships, so the agent runs the same ones.
    pub(super) fn find_program(&mut self, first_on_path: Option<PathBuf>, cx: &mut Context<Self>) {
        let provider = self.provider;
        let downloaded = self
            .agents
            .clone()
            .zip(provider.download())
            .map(|(agents, download)| download.program(&agents));
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            let found = cx
                .background_spawn(async move {
                    let (mut environment, error) = match login_shell_environment().await {
                        Ok(environment) => (environment, None),
                        Err(error) => (std::env::vars_os().collect(), Some(error)),
                    };
                    let named = std::env::var_os(provider.program_variable()).map(PathBuf::from);
                    let program = named
                        .or_else(|| program_on_path(provider.command(), &environment))
                        .or(downloaded);
                    let present = program.as_ref().is_some_and(|program| program.is_file());
                    // Without it the agent still runs, with only the composer's own programs.
                    if let Some(folder) = &first_on_path
                        && let Err(error) = put_first_on_path(&mut environment, folder)
                    {
                        eprintln!("{} is not on the agent's PATH: {error}", folder.display());
                    }
                    let installed = program.map(|program| Installed {
                        program,
                        environment,
                    });
                    (installed, present, error)
                })
                .await;
            // A sidebar that went in the meantime has nobody to tell.
            sidebar
                .update(cx, |sidebar, cx| sidebar.found(found, cx))
                .ok();
        }));
    }

    /// After a failed turn: the program may have been signed out meanwhile, such as in a
    /// terminal. Only an answer of signed out changes anything; the failed turn says the rest.
    pub(super) fn check_still_signed_in(&mut self, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            return;
        };
        let provider = self.provider;
        let asking = cx.background_spawn(async move { provider.account(&installed).await });
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            match asking.await {
                Ok(None) => {
                    let reason = Some(format!("{} is signed out.", provider.name()));
                    sidebar
                        .update(cx, |sidebar, cx| {
                            sidebar.end_agent(cx);
                            sidebar.set_setup(Setup::SignedOut { reason }, cx);
                        })
                        .ok();
                }
                // Signed in, or no answer: the next message shows whether the agent runs.
                Ok(Some(_)) | Err(_) => {}
            }
        }));
    }

    fn found(
        &mut self,
        (installed, present, error): (Option<Installed>, bool, Option<io::Error>),
        cx: &mut Context<Self>,
    ) {
        if let Some(error) = error {
            self.conversation.notice(format!(
                "The agent runs without the settings of your shell: {error}."
            ));
            self.show(None, cx);
        }
        self.installed = installed;
        if present {
            self.check(None, cx);
        } else {
            self.set_setup(Setup::NotInstalled, cx);
        }
    }

    pub(super) fn set_setup(&mut self, setup: Setup, cx: &mut Context<Self>) {
        let ready = matches!(setup, Setup::Ready { .. });
        self.setup = setup;
        // The menu keeps the last account it showed until the next one is in.
        if ready {
            self.update_menu(cx);
            self.wait_for_quiet(cx);
        }
        cx.notify();
    }

    pub(super) fn act(&mut self, action: SetupAction, cx: &mut Context<Self>) {
        match action {
            SetupAction::Download => self.download(cx),
            SetupAction::Cancel => self.cancel(cx),
            SetupAction::SignIn(choice) => self.sign_in(choice, cx),
            SetupAction::OpenPageAgain => {
                if let Some(choice) = self.signing_in {
                    self.sign_in(choice, cx);
                }
            }
            SetupAction::Check => self.check(None, cx),
        }
    }

    /// Asks the program whether it is signed in. `reason` is what the composer reads when it
    /// is not, after a sign-in.
    fn check(&mut self, reason: Option<String>, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            self.set_setup(Setup::NotInstalled, cx);
            return;
        };
        let provider = self.provider;
        self.set_setup(Setup::Checking, cx);
        let asking = cx.background_spawn({
            let installed = installed.clone();
            async move { provider.account(&installed).await }
        });
        let timeout = cx.background_executor().timer(CHECK_TIMEOUT);
        let folder = self.session.read(cx).project().root().to_path_buf();
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
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
            // A sidebar that went in the meantime has nobody to tell.
            if sidebar
                .update(cx, |sidebar, cx| sidebar.set_setup(setup, cx))
                .is_err()
                || !ready
            {
                return;
            }
            // The menu shows the models before the first message starts an agent. Asked after
            // every check, as another account may offer others.
            let listing =
                cx.background_spawn(async move { provider.models(installed, folder).await });
            let timeout = cx.background_executor().timer(CHECK_TIMEOUT);
            let listed = future::or(async { Some(listing.await) }, async {
                timeout.await;
                None
            })
            .await;
            sidebar
                .update(cx, |sidebar, cx| match listed {
                    Some(Ok(models)) => sidebar
                        .settings
                        .update(cx, |settings, cx| settings.set_models(models, cx)),
                    Some(Err(error)) => sidebar.models_not_listed(&error.to_string(), cx),
                    None => sidebar.models_not_listed("it did not answer within 30 seconds", cx),
                })
                .ok();
        }));
    }

    fn models_not_listed(&mut self, error: &str, cx: &mut Context<Self>) {
        let name = self.provider.name();
        self.conversation
            .notice(format!("{name} did not list its models: {error}"));
        self.show(None, cx);
    }

    /// Downloads the pinned program, from where an earlier try stopped.
    fn download(&mut self, cx: &mut Context<Self>) {
        let name = self.provider.name();
        let (Some(agents), Some(download)) = (self.agents.clone(), self.provider.download()) else {
            let message = match self.agents {
                None => format!("There is no folder on this computer to keep {name} in."),
                Some(_) => format!("{name} does not run on this computer."),
            };
            self.set_setup(Setup::DownloadFailed { message }, cx);
            return;
        };
        let size = download.size;
        self.set_setup(Setup::Downloading { received: 0, size }, cx);
        let (sender, receiver) = smol::channel::unbounded();
        let installing = cx.background_spawn(async move {
            install::install(&download, &agents, move |received| {
                // Closed only when the sidebar went, and then nobody reads it.
                sender.try_send(received).ok();
            })
            .await
        });
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            // Ends with the download, which drops the sender.
            while let Ok(received) = receiver.recv().await {
                let downloading = Setup::Downloading { received, size };
                if sidebar
                    .update(cx, |sidebar, cx| sidebar.set_setup(downloading, cx))
                    .is_err()
                {
                    return;
                }
            }
            let installed = installing.await;
            sidebar
                .update(cx, |sidebar, cx| sidebar.downloaded(installed, cx))
                .ok();
        }));
    }

    fn downloaded(&mut self, installed: Result<PathBuf, InstallError>, cx: &mut Context<Self>) {
        match installed {
            Ok(program) => {
                // The login shell answered before **Set up** showed.
                let environment = self.installed.take().map_or_else(
                    || std::env::vars_os().collect(),
                    |installed| installed.environment,
                );
                self.installed = Some(Installed {
                    program,
                    environment,
                });
                self.check(None, cx);
            }
            Err(error) => {
                let message = error.sentence(self.provider.name());
                self.set_setup(Setup::DownloadFailed { message }, cx);
            }
        }
    }

    /// Runs the provider's sign-in in the browser and waits for it, at most ten minutes.
    /// Started again, it ends the one before.
    fn sign_in(&mut self, choice: SignInChoice, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            return;
        };
        self.signing_in = Some(choice);
        self.set_setup(Setup::SigningIn, cx);
        let running = cx.background_spawn(async move { choice.run(&installed).await });
        let timeout = cx.background_executor().timer(SIGN_IN_TIMEOUT);
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
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
            sidebar
                .update(cx, |sidebar, cx| sidebar.signed_in(ended, cx))
                .ok();
        }));
    }

    fn signed_in(&mut self, ended: SignInEnded, cx: &mut Context<Self>) {
        self.signing_in = None;
        let reason = Some(ended.reason());
        match ended {
            // The program says whether it worked.
            SignInEnded::Finished => self.check(reason, cx),
            SignInEnded::Failed(_) | SignInEnded::TimedOut | SignInEnded::Cancelled => {
                self.set_setup(Setup::SignedOut { reason }, cx);
            }
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        // Dropping the task ends the download or the sign-in, and its process.
        self.setup_task = None;
        self.signing_in = None;
        let setup = match self.setup {
            Setup::Downloading { .. } => Setup::NotInstalled,
            _ => Setup::SignedOut {
                reason: Some(SignInEnded::Cancelled.reason()),
            },
        };
        self.set_setup(setup, cx);
    }

    /// Signs the program out, for every app that shares its login, and ends the thread's
    /// agent.
    pub(super) fn sign_out(&mut self, cx: &mut Context<Self>) {
        let Some(installed) = self.installed.clone() else {
            return;
        };
        self.end_agent(cx);
        let provider = self.provider;
        let signing_out = cx.background_spawn(async move { provider.sign_out(&installed).await });
        self.setup_task = Some(cx.spawn(async move |sidebar, cx| {
            let signed_out = signing_out.await;
            sidebar
                .update(cx, |sidebar, cx| match signed_out {
                    Ok(()) => sidebar.check(None, cx),
                    Err(error) => {
                        let notice = format!("Could not sign out: {error}");
                        sidebar.conversation.notice(notice);
                        sidebar.show(None, cx);
                    }
                })
                .ok();
        }));
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
