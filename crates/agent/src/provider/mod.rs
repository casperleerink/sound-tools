//! The provider seam: the only place a new provider touches outside its own module.
//!
//! Everything here is provider-neutral. Each provider has one private module that holds its
//! flags and protocol types and maps them to [`AgentEvent`]. Adding one means a new
//! [`Provider`] variant and a new module; the compiler then points at every `match` that needs
//! an arm.

mod action;
mod claude;
mod jsonl;
mod pi;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use smol::channel::{self, Receiver, Sender};

use self::pi::PiSignIn;
use crate::install::Download;

/// The coding agent CLI that runs a thread.
///
/// Serde gives its name in the settings and the saved threads: renaming a variant loses the
/// composer's pick and the threads of that provider.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Claude,
    Pi,
}

impl Provider {
    /// In the order the sidebar offers them.
    pub const ALL: [Provider; 2] = [Provider::Claude, Provider::Pi];

    /// The program's name for the composer, as in "The agent runs Claude Code by Anthropic."
    pub fn name(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Pi => "Pi",
        }
    }

    /// Who makes it.
    pub fn maker(self) -> &'static str {
        match self {
            Provider::Claude => "Anthropic",
            Provider::Pi => "Earendil",
        }
    }

    /// The program's name on the `PATH`, when the composer installed it themselves.
    pub fn command(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Pi => "pi",
        }
    }

    /// The pinned program for this computer, or `None` when the provider has no build for it
    /// or is not downloaded: Pi runs only where the composer installed it.
    pub fn download(self) -> Option<Download> {
        match self {
            Provider::Claude => claude::download(),
            Provider::Pi => None,
        }
    }

    /// The environment variable that names a program to run instead of the download, for
    /// development and tests.
    pub fn program_variable(self) -> &'static str {
        match self {
            Provider::Claude => "SOUND_TOOLS_CLAUDE",
            Provider::Pi => "SOUND_TOOLS_PI",
        }
    }

    /// Whether the program is signed in, and the ways in it offers. Fails when the program
    /// does not run; the error is the first line it wrote.
    pub async fn status(self, installed: &Installed) -> io::Result<Status> {
        match self {
            Provider::Claude => Ok(Status {
                account: claude::account(installed).await?,
                choices: claude_choices(),
                models: None,
            }),
            Provider::Pi => pi::status(installed).await,
        }
    }

    /// What the composer can pick, the default first, before any thread has started. Claude
    /// starts an agent in `folder` that is never sent a message, and ends it once it is ready.
    pub async fn models(self, installed: Installed, folder: PathBuf) -> io::Result<Vec<Model>> {
        if self == Provider::Pi {
            return pi::models(&installed).await;
        }
        // Held to the end: dropping every handle closes the agent before it answers.
        let (_thread, mut events) = Thread::start(ThreadOptions {
            provider: self,
            installed,
            folder,
            model: None,
            approval_mode: ApprovalMode::default(),
            resume: None,
            instructions: None,
        })?;
        while let Some(event) = events.next().await {
            match event {
                AgentEvent::Started { models, .. } => return Ok(models),
                // Nothing else was asked, so an error is the answer to starting.
                AgentEvent::Error { message }
                | AgentEvent::Exited {
                    reason: ExitReason::Failed { message },
                } => return Err(io::Error::other(message)),
                _ => {}
            }
        }
        Err(io::Error::other("it ended before it was ready"))
    }

    /// Signs the program out. Pi has a sign-in per provider, which the composer manages in
    /// Pi.
    pub async fn sign_out(self, installed: &Installed) -> io::Result<()> {
        match self {
            Provider::Claude => claude::sign_out(installed).await,
            Provider::Pi => Err(io::Error::other("Pi signs out in Pi itself.")),
        }
    }

    /// One question to the provider's small model, with no tools, no project and nothing kept.
    /// The answer is its text. Dropping the future ends the program.
    pub async fn ask_small_model(
        self,
        installed: &Installed,
        instructions: &str,
        question: &str,
    ) -> io::Result<String> {
        match self {
            Provider::Claude => claude::ask_small_model(installed, instructions, question).await,
            Provider::Pi => pi::ask_small_model(installed, instructions, question).await,
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

/// Whether a provider's program is signed in, and how it signs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    /// `None` when it is signed out.
    pub account: Option<Account>,
    /// In the order the composer reads them.
    pub choices: Vec<SignInChoice>,
    /// What it can run, the default first, when the answer says. Claude Code lists them from
    /// an agent of their own, see [`Provider::models`].
    pub models: Option<Vec<Model>>,
}

/// One way to sign in that a provider offers, such as "Sign in with your Claude plan". Each
/// runs the provider's own flow, so the credential stays in the provider's own store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInChoice {
    provider: Provider,
    label: String,
    way: Way,
}

/// The ways in Claude Code offers, the same on every computer.
fn claude_choices() -> Vec<SignInChoice> {
    (claude::SIGN_IN_CHOICES.iter())
        .map(|(label, arguments)| SignInChoice {
            provider: Provider::Claude,
            label: label.to_string(),
            way: Way::Claude(arguments),
        })
        .collect()
}

/// What the provider's program runs a sign-in with, for its driver.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Way {
    Claude(&'static [&'static str]),
    Pi(PiSignIn),
}

impl SignInChoice {
    /// Ways in to show with no program, for the gallery and the snapshots: Claude Code's
    /// own, or for Pi these labels in the browser and with a key.
    pub fn examples(provider: Provider, in_browser: &[&str], with_key: &[&str]) -> Vec<Self> {
        if provider == Provider::Claude {
            return claude_choices();
        }
        let choice = |label: &&str, with_key| SignInChoice {
            provider,
            label: label.to_string(),
            way: Way::Pi(PiSignIn {
                provider: label.to_lowercase(),
                with_key,
            }),
        };
        (in_browser.iter().map(|label| choice(label, false)))
            .chain(with_key.iter().map(|label| choice(label, true)))
            .collect()
    }

    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// What its button says: the way in, or the name of a provider that takes a key.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether it asks for a key, not for an account in the browser.
    pub fn takes_key(&self) -> bool {
        matches!(&self.way, Way::Pi(way) if way.with_key)
    }

    /// Runs the sign-in and waits for it to end. What it shows the composer goes to `steps`,
    /// and the answer to a [`SignInQuestion`] comes back on `answers` with the question's id.
    /// Ask [`Provider::status`] after: it may have ended with nobody signed in. Dropping the
    /// future cancels it.
    pub async fn run(
        &self,
        installed: &Installed,
        steps: Sender<SignInStep>,
        answers: Receiver<(String, String)>,
    ) -> io::Result<()> {
        match &self.way {
            // Claude Code opens the browser itself, and its page needs no answer here.
            Way::Claude(arguments) => claude::sign_in(installed, arguments).await,
            Way::Pi(way) => pi::sign_in(installed, way, steps, answers).await,
        }
    }
}

/// What a running sign-in shows the composer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInStep {
    /// The provider's page to sign in on. The sidebar opens it in the browser.
    Page {
        url: String,
    },
    /// A code to enter on the provider's page.
    Code {
        url: String,
        code: String,
    },
    /// What it does meanwhile, in the provider's words.
    Note(String),
    Question(SignInQuestion),
}

/// A question of the sign-in that waits for the composer, such as an API key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInQuestion {
    /// What the answer goes back with.
    pub id: String,
    pub message: String,
    pub answer: Answer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The answer is the id of one.
    Pick(Vec<Pick>),
    /// A field, hidden as it is typed when it is `secret`.
    Text { secret: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub id: String,
    pub label: String,
}

/// How much the agent may do without asking.
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
    /// Does anything without asking. Undo is the safety net.
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
        /// `None` when the provider says nothing of it.
        account: Option<Account>,
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
        /// One line of what it did, such as "Edited state/song/bass/verse-a.json", or for a
        /// command what the agent says it does ("Ran a command" when it says nothing).
        title: String,
        /// The same while it runs.
        running_title: String,
        /// The same asked for, for a step the composer denied: it never ran. Empty in a thread
        /// saved before it came.
        #[serde(default)]
        request_title: String,
        /// The command a command step runs, whole, which the opened steps show under its words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
        /// What it does, for its icon and the summary of a finished turn. `Other` in a thread
        /// saved before it came.
        #[serde(default)]
        kind: StepKind,
    },
    StepDone {
        id: StepId,
        outcome: StepOutcome,
    },
    /// The agent waits for an answer. An approval still open when the turn ends is void.
    ApprovalRequested {
        id: ApprovalId,
        /// One line of what the agent wants to do, such as "Edit state/bass.json", or "Run".
        title: String,
        /// The command a "Run" asks to run, whole, which the question shows as it is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// The path is as the title says it, so two steps on one file name the same path.
    Read {
        path: String,
    },
    /// An edit or a write.
    Change {
        path: String,
    },
    Run,
    Search,
    #[default]
    Other,
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
    /// Who serves it, for a provider that runs the models of many, such as "OpenRouter".
    /// The menu groups the models by it.
    pub source: Option<String>,
}

/// The id of a new session. It is ours to pick, so it is known before the agent says
/// anything, and a thread quit before then can still be resumed.
fn new_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// What the composer sends: the text, and what is attached to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub attachments: Vec<Attachment>,
    /// What the agent reads before the text that the composer did not write, such as that
    /// they went back in the history. The thread shows only the text.
    pub note: Option<String>,
}

impl From<String> for Message {
    fn from(text: String) -> Self {
        Message {
            text,
            attachments: Vec::new(),
            note: None,
        }
    }
}

impl From<&str> for Message {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attachment {
    /// A picture the agent sees.
    Image(Image),
    /// Any other file. The agent gets its path and reads it itself; nothing is copied.
    File(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub format: ImageFormat,
    pub bytes: Vec<u8>,
}

/// The formats a picture can be sent in. Any other picture goes as a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

/// What a [`Thread`] asks its driver to do.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Send(Message),
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
            Provider::Claude => Driver::Claude(claude::start(options, &session_id, receiver)?),
            Provider::Pi => Driver::Pi(pi::start(options, &session_id, receiver)?),
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
    /// one. Saved with the thread from its first message, to resume it later.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Sends a message of the composer. A turn starts. Send only between turns: a message
    /// sent while a turn runs becomes a turn of its own, queued, which the events do not
    /// show apart.
    pub fn send(&self, message: impl Into<Message>) -> Result<(), ThreadClosed> {
        self.command(Command::Send(message.into()))
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
    Pi(pi::Events),
}

impl Events {
    /// The next event, or `None` after [`AgentEvent::Exited`].
    ///
    /// Cancel-safe: dropping the future before it is ready loses nothing, so it can race a
    /// timer, as a view that takes the events once per frame does.
    pub async fn next(&mut self) -> Option<AgentEvent> {
        match &mut self.driver {
            Driver::Claude(events) => events.next().await,
            Driver::Pi(events) => events.next().await,
        }
    }
}
