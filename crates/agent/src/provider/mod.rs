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

/// The coding agent CLI that runs a thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
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
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepId(pub String);

/// One question of the agent, answered with [`Thread::answer`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApprovalId(pub String);

/// What the agent did, in order. A turn runs from [`AgentEvent::TurnStarted`] to
/// [`AgentEvent::TurnEnded`], and every turn that starts ends, also when the process dies.
///
/// Serde gives the lines of a thread's saved log, see `crate::store`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
        /// The same while it runs, such as "Running cargo build".
        running_title: String,
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
    /// [`Session::Resume`] named a session the provider no longer has. The thread cannot
    /// continue; a new one can.
    SessionNotFound,
    /// It stopped by itself. The message is the first line of what it said, if anything.
    Failed { message: String },
}

/// The account the agent runs under.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub email: Option<String>,
    /// Such as "Claude Max".
    pub plan: Option<String>,
}

/// A model the composer can pick for a thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    /// What [`ThreadOptions::model`] takes.
    pub id: String,
    pub name: String,
    pub description: String,
}

/// What a [`Thread`] asks its driver to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
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

    /// A thread with no agent behind it: what it is asked to do comes out of the receiver.
    /// For a test of a view, which cannot run a process.
    pub fn without_agent() -> (Thread, Receiver<Command>) {
        let (sender, receiver) = channel::unbounded();
        (Thread { commands: sender }, receiver)
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
