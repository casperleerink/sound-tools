//! The provider seam: the only place a new provider touches outside its own module.
//!
//! Everything here is provider-neutral. Each provider has one private module that holds its
//! flags and protocol types and maps them to [`AgentEvent`]. Adding one means a new
//! [`Provider`] variant and a new module; the compiler then points at every `match` that needs
//! an arm.

mod claude;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use smol::channel::{self, Receiver, Sender};

use crate::install::Download;

/// The coding agent CLI that runs a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Claude,
}

impl Provider {
    /// The program's name for the composer, as in "The agent runs Claude Code by Anthropic."
    pub fn name(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
        }
    }

    /// Who makes it.
    pub fn maker(self) -> &'static str {
        match self {
            Provider::Claude => "Anthropic",
        }
    }

    /// The program's name on the `PATH`, when the composer installed it themselves.
    pub fn command(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
        }
    }

    /// The pinned program for this computer, or `None` when the provider has no build for it.
    pub fn download(self) -> Option<Download> {
        match self {
            Provider::Claude => claude::download(),
        }
    }

    /// The environment variable that names a program to run instead of the download, for
    /// development and tests.
    pub fn program_variable(self) -> &'static str {
        match self {
            Provider::Claude => "SOUND_TOOLS_CLAUDE",
        }
    }

    /// The ways in the provider offers, in the order the composer reads them.
    pub fn sign_in_choices(self) -> Vec<SignInChoice> {
        match self {
            Provider::Claude => claude::SIGN_IN_CHOICES
                .map(|(label, arguments)| SignInChoice {
                    provider: self,
                    label,
                    arguments,
                })
                .to_vec(),
        }
    }

    /// The account the program is signed in to, or `None` when it is signed out. Fails when
    /// the program does not run; the error is the first line it wrote.
    pub async fn account(self, installed: &Installed) -> io::Result<Option<Account>> {
        match self {
            Provider::Claude => claude::account(installed).await,
        }
    }

    pub async fn sign_out(self, installed: &Installed) -> io::Result<()> {
        match self {
            Provider::Claude => claude::sign_out(installed).await,
        }
    }
}

/// Where the provider's program is, and the environment it runs in.
#[derive(Clone, Debug)]
pub struct Installed {
    pub program: PathBuf,
    /// Usually [`crate::login_shell_environment`]. The driver removes what would confuse the
    /// agent.
    pub environment: HashMap<OsString, OsString>,
}

/// One way to sign in that a provider offers, such as "Sign in with your Claude plan". Each
/// runs the provider's own flow in the browser, so the app never handles a credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignInChoice {
    provider: Provider,
    label: &'static str,
    /// What the provider's program runs it with, for its driver.
    arguments: &'static [&'static str],
}

impl SignInChoice {
    /// What its button says.
    pub fn label(self) -> &'static str {
        self.label
    }

    /// Runs the sign-in and waits for it to end. Ask [`Provider::account`] after: it may have
    /// ended with nobody signed in. Dropping the future cancels it.
    pub async fn run(self, installed: &Installed) -> io::Result<()> {
        match self.provider {
            Provider::Claude => claude::sign_in(installed, self.arguments).await,
        }
    }
}

/// How much the agent may do without asking. One setting for the machine, never saved in a
/// project, so the agent cannot raise its own access by editing a file there.
///
/// Serde gives its name in the settings file, see `crate::settings`: renaming a variant
/// resets the choice of every composer to the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    /// Asks before every edit and command.
    AskForEverything,
    /// Edits the project freely, asks before commands.
    #[default]
    AskBeforeCommands,
    /// Does anything without asking. Undo and git are the safety net.
    NeverAsk,
}

/// The composer's answer to an [`AgentEvent::ApprovalRequested`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalAnswer {
    Allow,
    /// Allows it, and the same kind of action, until the thread's process ends.
    AllowForThread,
    Deny,
}

/// What [`Thread::start`] needs.
#[derive(Clone, Debug)]
pub struct ThreadOptions {
    pub provider: Provider,
    /// The provider's program, such as the path of `claude`, and its environment.
    pub installed: Installed,
    /// The project folder. The agent works in it.
    pub folder: PathBuf,
    /// One of the ids in [`AgentEvent::Started`], or `None` for the provider's default.
    pub model: Option<String>,
    pub approval_mode: ApprovalMode,
    /// The session to continue, from an earlier [`Thread::session_id`], or `None` for a new
    /// one.
    pub resume: Option<String>,
    /// A file of the composer's instructions for every project, added to the agent's system
    /// prompt. The project's own are in the folder, where the agent reads them itself.
    pub instructions: Option<PathBuf>,
}

/// One step of a turn, such as an edit or a command. The id is the provider's; a test that
/// feeds events with no process makes its own.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepId(pub String);

/// One question of the agent, answered with [`Thread::answer`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApprovalId(pub String);

/// What the agent did, in order. A turn runs from [`AgentEvent::TurnStarted`] to
/// [`AgentEvent::TurnEnded`], and every turn that starts ends, also when the process dies.
///
/// Serde gives the lines of a thread's saved log, see `crate::store`, so the names of these
/// variants and of their fields, and of the types in them, are a file format: renaming one
/// breaks the threads saved before. A new field gets `#[serde(default)]`, so older lines
/// still read. `store/tests.rs` pins one line of each.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEvent {
    /// The agent is ready. It comes once, before the first turn ends. Never saved: it shows
    /// nothing in the thread. The session is [`Thread::session_id`].
    #[serde(skip)]
    Started {
        account: Account,
        /// What the composer can pick, the provider's default first.
        models: Vec<Model>,
    },
    /// A message was sent and the agent works on it.
    TurnStarted,
    /// More text of the answer, while it streams.
    TextDelta {
        text: String,
    },
    /// The whole text of one block of the answer. It replaces the deltas since the last
    /// block, and can come with no deltas before it.
    TextDone {
        text: String,
    },
    StepStarted {
        id: StepId,
        /// One line in the past tense, such as "Edited state/arrangement/bass/verse-a.json" or
        /// "Ran \`cargo build\`". Text between backticks shows as code, here and in the
        /// title of an approval.
        title: String,
        /// The same while it runs, such as "Running \`cargo build\`".
        running_title: String,
        /// The same asked for, such as "Run \`cargo build\`", for a step the composer denied:
        /// it never ran. Empty in a thread saved before it came.
        #[serde(default)]
        request_title: String,
    },
    StepDone {
        id: StepId,
        outcome: StepOutcome,
    },
    /// The agent waits for an answer. An approval still open when the turn ends is void.
    ApprovalRequested {
        id: ApprovalId,
        /// One line of what the agent wants to do, such as "Run `cargo build`".
        title: String,
    },
    TurnEnded {
        outcome: TurnOutcome,
    },
    /// A request to the agent failed, such as a stop or a change of the approval mode. The
    /// thread goes on.
    Error {
        message: String,
    },
    /// The process ended. It is the last event.
    Exited {
        reason: ExitReason,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Done,
    Failed,
    /// The composer denied it.
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    /// Stopped with [`Thread::interrupt`].
    Interrupted,
    Failed {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    /// Every [`Thread`] handle was dropped, so the agent ended.
    Finished,
    /// [`ThreadOptions::resume`] named a session the provider no longer has. The thread cannot
    /// continue; a new one can.
    SessionNotFound,
    /// It stopped by itself. The message is the whole sentence the sidebar shows, such as
    /// "Claude Code stopped: " and the first line of what it said.
    Failed { message: String },
}

/// The account the agent runs under.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Account {
    pub email: Option<String>,
    /// Such as "Claude Max".
    pub plan: Option<String>,
}

/// A model the composer can pick for a thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    /// What [`ThreadOptions::model`] takes.
    pub id: String,
    pub name: String,
    pub description: String,
    /// The model it runs, for the composer's menu button: "Opus 5.5" for a default named
    /// "Default (recommended)".
    pub short_name: String,
}

/// The id of a new session. It is ours to pick, so it is known before the agent says
/// anything, and a thread quit before then can still be resumed.
fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// What a [`Thread`] asks its driver to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Send(String),
    Interrupt,
    Answer(ApprovalId, ApprovalAnswer),
    SetApprovalMode(ApprovalMode),
    /// One of the ids in [`AgentEvent::Started`].
    SetModel(String),
}

/// Sends to the agent of one thread. Cheap to clone, and every method returns at once, so an
/// entity can call it while it updates. Dropping every clone ends the agent.
#[derive(Clone, Debug)]
pub struct Thread {
    commands: Sender<Command>,
    session_id: String,
}

/// The [`Events`] of the thread were dropped, which stops the agent.
#[derive(Debug)]
pub struct ThreadClosed;

impl fmt::Display for ThreadClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the agent of this thread has stopped")
    }
}

impl std::error::Error for ThreadClosed {}

impl Thread {
    /// Starts the agent's process. Fails only when the program cannot run; everything after
    /// that comes as [`Events`].
    pub fn start(options: ThreadOptions) -> io::Result<(Thread, Events)> {
        let (sender, receiver) = channel::unbounded();
        let session_id = options.resume.clone().unwrap_or_else(new_session_id);
        let driver = match options.provider {
            Provider::Claude => {
                Driver::Claude(claude::Events::start(options, &session_id, receiver)?)
            }
        };
        let thread = Thread {
            commands: sender,
            session_id,
        };
        Ok((thread, Events { driver }))
    }

    /// A thread with no agent behind it, in the session `resume` names or a new one: what it
    /// is asked to do comes out of the receiver. For a test of a view, which cannot run a
    /// process.
    pub fn without_agent(resume: Option<String>) -> (Thread, Receiver<Command>) {
        let (sender, receiver) = channel::unbounded();
        let thread = Thread {
            commands: sender,
            session_id: resume.unwrap_or_else(new_session_id),
        };
        (thread, receiver)
    }

    /// The session the agent works in: the one [`ThreadOptions::resume`] named, or the new
    /// one.
    /// Saved with the thread from its first message, to resume it later.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Sends a message of the composer. A turn starts. Send only between turns: a message
    /// sent while a turn runs becomes a turn of its own, queued, which the events do not
    /// show apart.
    pub fn send(&self, text: impl Into<String>) -> Result<(), ThreadClosed> {
        self.command(Command::Send(text.into()))
    }

    /// Stops the running turn. It ends with [`TurnOutcome::Interrupted`].
    pub fn interrupt(&self) -> Result<(), ThreadClosed> {
        self.command(Command::Interrupt)
    }

    /// Answers an approval. An answer to a void one does nothing.
    pub fn answer(&self, approval: ApprovalId, answer: ApprovalAnswer) -> Result<(), ThreadClosed> {
        self.command(Command::Answer(approval, answer))
    }

    /// Applies from the next action of the agent, with no restart.
    pub fn set_approval_mode(&self, mode: ApprovalMode) -> Result<(), ThreadClosed> {
        self.command(Command::SetApprovalMode(mode))
    }

    /// Applies from the next message, with no restart. A model the provider does not know
    /// comes back as [`AgentEvent::Error`], and the thread keeps the one it had.
    pub fn set_model(&self, model: impl Into<String>) -> Result<(), ThreadClosed> {
        self.command(Command::SetModel(model.into()))
    }

    fn command(&self, command: Command) -> Result<(), ThreadClosed> {
        self.commands.try_send(command).map_err(|_| ThreadClosed)
    }
}

/// What the agent of one thread does. Poll [`Events::next`] for as long as the thread is open:
/// it also writes what the [`Thread`] sends. Dropping it kills the process and everything
/// the agent started.
#[derive(Debug)]
pub struct Events {
    driver: Driver,
}

#[derive(Debug)]
enum Driver {
    Claude(claude::Events),
}

impl Events {
    /// The next event, or `None` after [`AgentEvent::Exited`].
    ///
    /// Cancel-safe: dropping the future before it is ready loses nothing, so it can race a
    /// timer, as a view that takes the events once per frame does.
    pub async fn next(&mut self) -> Option<AgentEvent> {
        match &mut self.driver {
            Driver::Claude(events) => events.next().await,
        }
    }
}
