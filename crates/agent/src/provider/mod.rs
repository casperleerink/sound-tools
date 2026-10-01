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

use smol::channel::{self, Sender};

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
            Provider::Claude => claude::SignIn::ALL
                .map(|way| SignInChoice {
                    label: way.label(),
                    way: Way::Claude(way),
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
    pub label: &'static str,
    way: Way,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    Claude(claude::SignIn),
}

impl SignInChoice {
    /// Runs the sign-in and waits for it to end. Ask [`Provider::account`] after: it may have
    /// ended with nobody signed in. Dropping the future cancels it.
    pub async fn run(self, installed: &Installed) -> io::Result<()> {
        match self.way {
            Way::Claude(way) => claude::sign_in(installed, way).await,
        }
    }
}

/// How much the agent may do without asking. One setting for the machine, never saved in a
/// project, so the agent cannot raise its own access by editing a file there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

/// Which conversation the agent continues.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Session {
    New,
    /// The session id an earlier [`AgentEvent::Started`] gave.
    Resume(String),
}

/// What [`Thread::start`] needs.
#[derive(Clone, Debug)]
pub struct ThreadOptions {
    pub provider: Provider,
    /// The provider's program, such as the path of `claude`.
    pub program: PathBuf,
    /// The project folder. The agent works in it.
    pub folder: PathBuf,
    /// One of the ids in [`AgentEvent::Started`], or `None` for the provider's default.
    pub model: Option<String>,
    pub approval_mode: ApprovalMode,
    pub session: Session,
    /// The environment of the program, usually [`crate::login_shell_environment`]. The driver
    /// removes what would confuse the agent.
    pub environment: HashMap<OsString, OsString>,
}

/// One step of a turn, such as an edit or a command. The id is the provider's; a test that
/// feeds events with no process makes its own.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StepId(pub String);

/// One question of the agent, answered with [`Thread::answer`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ApprovalId(pub String);

/// What the agent did, in order. A turn runs from [`AgentEvent::TurnStarted`] to
/// [`AgentEvent::TurnEnded`], and every turn that starts ends, also when the process dies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentEvent {
    /// The agent is ready. It comes once, before the first turn ends.
    Started {
        /// Saved with the thread, to resume it with [`Session::Resume`].
        session_id: String,
        account: Account,
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
        /// "Ran cargo build".
        title: String,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    Done,
    Failed,
    /// The composer denied it.
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed,
    /// Stopped with [`Thread::interrupt`].
    Interrupted,
    Failed {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitReason {
    /// Every [`Thread`] handle was dropped, so the agent ended.
    Finished,
    /// [`Session::Resume`] named a session the provider no longer has. The thread cannot
    /// continue; a new one can.
    SessionNotFound,
    /// It stopped by itself. The message is the first line of what it said, if anything.
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
}

/// What a [`Thread`] asks its driver to do.
#[derive(Debug)]
enum Command {
    Send(String),
    Interrupt,
    Answer(ApprovalId, ApprovalAnswer),
    SetApprovalMode(ApprovalMode),
}

/// Sends to the agent of one thread. Cheap to clone, and every method returns at once, so an
/// entity can call it while it updates. Dropping every clone ends the agent.
#[derive(Clone, Debug)]
pub struct Thread {
    commands: Sender<Command>,
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
        let driver = match options.provider {
            Provider::Claude => Driver::Claude(claude::Events::start(options, receiver)?),
        };
        Ok((Thread { commands: sender }, Events { driver }))
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
