//! The chat of the sidebar, with no toolkit: the thread, its agent and the composer's rules.
//! A view draws what it reads here and calls a method for what the composer does.
//!
//! One thread at a time. Its process starts on the first send and ends with **New**, or when
//! a recent thread opens. Every message is one request of the project, so the agent's file
//! writes for it are one undo step. The thread is saved on the machine as it goes
//! (`crate::store`), and the chat opens on the project's last one. The menu of **New** also
//! opens the composer's instructions to the agent in place of the thread.
//!
//! The window calls [`Chat::poll`] on every frame, and [`Chat::heard`] with the project's
//! events. Background work wakes the window with the [`Waker`] it passed in.

mod attachments;
mod history;
mod instructions;
mod menus;
mod naming;
mod shown;
#[cfg(test)]
mod tests;
mod threads;
mod turn;
mod went_back;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use smol::channel::Sender;
use sound_core::{PointId, Problem, Project, ProjectEvent};

pub use self::attachments::{Attached, PASTED_IMAGE};
pub use self::history::Arrow;
pub use self::instructions::{INSTRUCTIONS, Instructions, Scope};
pub use self::menus::{
    APPROVAL_ANSWERS, AccessChoice, ModelGroup, ModelRow, NO_AGENT, SEARCH_MODELS,
};
use self::naming::Namer;
pub use self::shown::{Below, FIX_THEM, Mark, Meta, StepShown, TRY_AGAIN, TurnShown, Working};
pub use self::threads::{NEW, NEW_MENU, NEW_THREAD, OPEN_RECENT};
use self::went_back::WentBack;
use crate::agents::Agents;
use crate::conversation::Conversation;
use crate::jobs::{Inbox, Job, Waker};
use crate::store::{RecentThread, SavedThread, ThreadStore, Write};
use crate::{AgentEvent, Installed, Provider, Thread};

/// Over the composer while it is empty.
pub const COMPOSER_PLACEHOLDER: &str = "Ask for a change";
/// What asking the agent to fix the files that are not live sends, from under a turn or from
/// the window's list. The agent reads what is not live itself.
pub const FIX_NOT_LIVE: &str = "Fix the files that are not live.";
/// The button that goes to Settings while no agent is signed in.
pub const OPEN_SETTINGS: &str = "Open Settings";
/// In place of the composer once the thread can't go on, unless its last turn says so.
pub const CANNOT_CONTINUE: &str = "This thread can't continue.";

/// What the sidebar shows in place of a thread, or the thread with what is under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shows {
    /// The instructions, over the thread: see [`Chat::instructions`].
    Instructions,
    /// The agents are still looked for: the thread's shape in grey, pulsing, as large as the
    /// thread and composer will be, so nothing jumps.
    Looking,
    /// No agent is signed in: this sentence and [`OPEN_SETTINGS`].
    NoAgent(String),
    Thread(Footer),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Footer {
    Composer,
    /// The agent lost the thread's session: the thread stays to read, and **New thread** is
    /// the way on. `say` is [`CANNOT_CONTINUE`] unless the last turn says it already.
    CannotContinue {
        say: Option<&'static str>,
    },
}

/// The process of the thread, from the first send until **New** or until it ends.
struct Agent {
    provider: Provider,
    thread: Thread,
    /// Which agent of this chat it is, so events of one that ended are dropped.
    id: u64,
    /// Reads the process. Dropping it ends the process.
    _reading: Option<Job>,
}

/// What background work hands the chat.
enum Done {
    Loaded {
        store: ThreadStore,
        current: Result<Option<(SavedThread, Conversation)>, String>,
        recent: Result<Vec<RecentThread>, String>,
    },
    Opened(u64, Result<Option<(SavedThread, Conversation)>, String>),
    Event(u64, AgentEvent),
    Attached(Vec<Result<Attached, String>>),
    InstructionsRead(PathBuf, io::Result<String>),
    /// A line for the thread, such as a write that failed.
    Notice(String),
    /// A write of the thread or the instructions is done, or failed.
    Saved,
    /// The history was still since this generation.
    Quiet(Option<u64>),
    Named {
        start: PointId,
        tip: PointId,
        answer: Option<io::Result<String>>,
    },
}

pub struct Chat {
    agents: Agents,
    /// The project's folder, where the agent runs.
    folder: PathBuf,
    /// Whether the thread's provider was signed in at the last poll.
    ready: bool,
    conversation: Conversation,
    /// The finished turns whose steps show, by entry.
    steps_open: HashSet<usize>,
    /// The problems of the project when the last turn began.
    problems_before: Vec<Problem>,
    /// The turn that ended last and when, until the next message: the watcher may still
    /// apply its last writes, and their problems are the turn's too.
    just_ended: Option<(usize, Instant)>,
    /// The problems each finished turn left that were not there before it, by entry.
    problems_left: HashMap<usize, Vec<Problem>>,
    /// The turns whose problems show line by line, by entry.
    problems_open: HashSet<usize>,
    /// What goes with the next message.
    attachments: Vec<Attached>,
    /// Reads of attachments not in yet. No message goes until they are.
    attachments_reading: usize,
    /// Where up and down in the composer are among the earlier messages.
    history: history::History,
    /// Where the composer went back to in the project history, until the next message.
    went_back: Option<WentBack>,
    namer: Namer,
    agent: Option<Agent>,
    /// How many agents this chat started, for the id of the next.
    agents_started: u64,
    /// The thread shown, as it is saved. `None` until its first message.
    saved: Option<SavedThread>,
    /// Hands what to keep of the thread to its writer, in order. `None` while nothing is
    /// saved, as in a picture.
    writes: Option<Sender<Write>>,
    /// Reads the last thread of the project. No message goes until it is in.
    loading: Option<Job>,
    /// **New** was chosen while the last thread was read: the new thread wins over it.
    new_thread_while_loading: bool,
    /// The threads of the project, once it is read. `None` while nothing is saved.
    store: Option<ThreadStore>,
    /// The project's other threads, the last one shown first.
    recent: Vec<RecentThread>,
    /// Reads a recent thread the composer opened, by the number of the opening. No message
    /// goes until it is in.
    opening: Option<(u64, Job)>,
    openings: u64,
    /// The instructions shown in place of the thread.
    instructions: Option<Instructions>,
    /// Hands the instructions to their writer, in the order they were typed.
    instruction_saves: Sender<instructions::Save>,
    /// Writes handed to a writer and not done yet.
    saving: usize,
    inbox: Inbox<Done>,
}

impl Chat {
    /// A chat on `project`, whose thread runs on one of `agents`. The threads of the project
    /// are kept in `threads`, `agent/threads` in the support folder of the machine; with
    /// `None` nothing is saved. `waker` asks the window for a frame, whose [`Self::poll`]
    /// takes what came.
    pub fn new(agents: Agents, project: &Project, threads: Option<PathBuf>, waker: Waker) -> Self {
        let inbox = Inbox::new(waker);
        let folder = project.root().to_path_buf();
        let loading = threads.map(|threads| threads::load(threads, folder.clone(), inbox.outbox()));
        let mut conversation = Conversation::default();
        // Read before this chat was made: it says so too.
        if let Some(unreadable) = agents.settings().unreadable() {
            conversation.notice(unreadable);
        }
        let instruction_saves = instructions::writer(inbox.outbox());
        let mut chat = Self {
            agents,
            folder,
            ready: false,
            conversation,
            steps_open: HashSet::new(),
            problems_before: Vec::new(),
            just_ended: None,
            problems_left: HashMap::new(),
            problems_open: HashSet::new(),
            attachments: Vec::new(),
            attachments_reading: 0,
            history: history::History::default(),
            went_back: None,
            namer: Namer::default(),
            agent: None,
            agents_started: 0,
            saved: None,
            writes: None,
            loading,
            new_thread_while_loading: false,
            store: None,
            recent: Vec::new(),
            opening: None,
            openings: 0,
            instructions: None,
            instruction_saves,
            saving: 0,
            inbox,
        };
        chat.ready = chat.ready_program().is_some();
        chat.wait_for_quiet(project);
        chat
    }

    pub fn agents(&self) -> &Agents {
        &self.agents
    }

    /// For the Agents section of Settings. The chat follows what changes at its next poll.
    pub fn agents_mut(&mut self) -> &mut Agents {
        &mut self.agents
    }

    /// Takes what came in from the background since the last frame: the agent's events, a
    /// thread read, files read, the agents' news. Gives the pages to open in the browser.
    pub fn poll(&mut self, project: &mut Project) -> Vec<String> {
        let polled = self.agents.poll();
        for notice in polled.notices {
            self.conversation.notice(notice);
        }
        while let Some(done) = self.inbox.take() {
            self.done(done, project);
        }
        self.follow(project);
        self.naming_heard(project);
        polled.open_urls
    }

    fn done(&mut self, done: Done, project: &mut Project) {
        match done {
            Done::Loaded {
                store,
                current,
                recent,
            } => self.loaded(store, current, recent),
            Done::Opened(opening, opened) => {
                if self.opening.as_ref().is_some_and(|(id, _)| *id == opening) {
                    self.opened(opened);
                }
            }
            Done::Event(agent, event) => {
                if self
                    .agent
                    .as_ref()
                    .is_some_and(|running| running.id == agent)
                {
                    self.receive([event], project);
                }
            }
            Done::Attached(read) => self.attached(read),
            Done::InstructionsRead(file, text) => self.instructions_read(file, text),
            Done::Notice(notice) => self.conversation.notice(notice),
            Done::Saved => self.saving = self.saving.saturating_sub(1),
            Done::Quiet(generation) => self.quiet(generation, project),
            Done::Named { start, tip, answer } => self.named(start, tip, answer, project),
        }
    }

    /// What the window heard from the project: problems that changed, and a step back in the
    /// history.
    pub fn heard(&mut self, event: &ProjectEvent, project: &Project) {
        match event {
            ProjectEvent::ProblemsChanged => self.problems_changed(project),
            ProjectEvent::WentBack(point) => self.went_back(*point, project),
            _ => {}
        }
    }

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// What the sidebar shows now.
    pub fn shows(&self) -> Shows {
        if self.instructions.is_some() {
            return Shows::Instructions;
        }
        if self.ready_program().is_none() {
            if self.agents.is_checking() {
                return Shows::Looking;
            }
            return Shows::NoAgent(self.no_agent());
        }
        if self.conversation.can_continue() {
            return Shows::Thread(Footer::Composer);
        }
        let say = (!self.conversation.loss_said()).then_some(CANNOT_CONTINUE);
        Shows::Thread(Footer::CannotContinue { say })
    }

    /// Whether the composer shows, which cmd-L focuses; else the sidebar itself takes the
    /// focus, so cmd-L and escape still work there.
    pub fn has_composer(&self) -> bool {
        self.shows() == Shows::Thread(Footer::Composer)
    }

    /// While no agent is signed in: why, in one sentence.
    fn no_agent(&self) -> String {
        let stopped = (self.agents.program(self.provider()))
            .is_some_and(|program| matches!(program.setup, crate::Setup::Stopped { .. }));
        match (stopped, self.thread_provider()) {
            (true, _) => "The agent did not start. Settings says why.".to_string(),
            // A thread keeps its provider: **New** starts one on another.
            (false, Some(provider)) => format!(
                "This thread runs on {}, which is not signed in.",
                provider.name()
            ),
            (false, None) => "Set up an agent to start.".to_string(),
        }
    }

    /// Whether writes of the thread or of the instructions are still on their way to disk. A
    /// window that quits waits for them.
    pub fn is_saving(&self) -> bool {
        self.saving > 0
    }

    /// Whether a thread is still read: the project's last one, or a recent one opened. No
    /// message goes until it is in.
    pub fn is_loading(&self) -> bool {
        self.loading.is_some() || self.opening.is_some()
    }

    /// Whether the agent works or waits on the composer, so a closed panel can say so.
    pub fn is_working(&self) -> bool {
        self.conversation.is_working()
    }

    /// The session the next process of the thread resumes: the thread's own once its agent
    /// has started, so a thread opened again goes on where it left off. `None` starts a new
    /// one.
    pub fn resume(&self) -> Option<String> {
        self.saved.as_ref()?.session_id.clone()
    }

    /// The provider the thread runs on: the one its agent started on, else the composer's
    /// pick while it is signed in, else the first that is.
    fn provider(&self) -> Provider {
        if let Some(provider) = self.thread_provider() {
            return provider;
        }
        let picked = self.agents.settings().settings().provider;
        if self.agents.ready(picked).is_some() {
            return picked;
        }
        (self.agents.programs().iter())
            .find(|program| program.is_ready())
            .map_or(picked, |program| program.provider)
    }

    /// The provider whose session the thread is, once its agent started: a thread keeps it.
    fn thread_provider(&self) -> Option<Provider> {
        if let Some(agent) = &self.agent {
            return Some(agent.provider);
        }
        let saved = self.saved.as_ref()?;
        saved.session_id.as_ref().map(|_| saved.provider)
    }

    /// The thread's provider and its program, once it is signed in.
    fn ready_program(&self) -> Option<(Provider, Installed)> {
        let provider = self.provider();
        Some((provider, self.agents.ready(provider)?))
    }

    /// Follows the thread's provider: its agent ends once it is signed out, and naming starts
    /// once it is signed in.
    fn follow(&mut self, project: &mut Project) {
        let ready = self.ready_program().is_some();
        if self.ready && !ready {
            self.end_agent(project);
        }
        // Naming waits for an agent to name with.
        if ready && !self.ready {
            self.wait_for_quiet(project);
        }
        self.ready = ready;
    }

    /// For a thread that is another one now: what the chat keeps by entry is not its.
    fn forget_entries(&mut self) {
        self.steps_open.clear();
        self.problems_left.clear();
        self.problems_open.clear();
        self.just_ended = None;
        self.history.reset();
    }

    /// Opens or closes the steps of the turn at `index`, under its "Worked for" line.
    pub fn toggle_steps(&mut self, index: usize) {
        if !self.steps_open.remove(&index) {
            self.steps_open.insert(index);
        }
    }

    /// Opens or closes the lines of the problems the turn at `index` left.
    pub fn toggle_problems(&mut self, index: usize) {
        if !self.problems_open.remove(&index) {
            self.problems_open.insert(index);
        }
    }

    /// Up on the first row of the composer, or down on its last: the text it shows next, or
    /// `None` to leave `text` as it is.
    pub fn recall(&mut self, text: &str, arrow: Arrow) -> Option<String> {
        let messages: Vec<&str> = self.conversation.messages().collect();
        self.history.recall(&messages, text, arrow)
    }
}
