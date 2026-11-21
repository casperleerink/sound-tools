//! A turn of the agent: its process, from the first send until it ends, the message and
//! what the agent did, its approvals, and the problems a turn left.

use std::io;
use std::time::{Instant, SystemTime};

use sound_core::{GROUPING_WINDOW, Problem, Project};

use super::{Agent, Chat, Done};
use crate::conversation::{Entry, Turn, TurnEnd, request_label};
use crate::jobs;
use crate::store::{Line, SavedThread, Write};
use crate::{
    AgentEvent, ApprovalAnswer, ApprovalMode, Attachment, Installed, Message, Provider, Thread,
    ThreadOptions, TurnOutcome,
};

impl Chat {
    /// Whether the composer's `text` can go now: it says something and every file it has
    /// attached is read.
    pub fn can_send(&self, text: &str) -> bool {
        !text.trim().is_empty() && self.attachments_reading == 0
    }

    /// Sends the composer's `text`, with what it has attached. Whether it went: then the
    /// composer empties.
    pub fn send(&mut self, text: &str, project: &mut Project) -> bool {
        let message = text.trim().to_string();
        if !self.can_send(&message) || !self.started() {
            return false;
        }
        self.history.reset();
        let (names, attachments) = std::mem::take(&mut self.attachments)
            .into_iter()
            .map(|attached| (attached.name, attached.attachment))
            .unzip();
        self.deliver(message, names, attachments, project);
        true
    }

    /// `message` sent as if typed, with nothing attached, leaving the composer as it is: from
    /// "Try again", from "Ask the agent to fix them", or from the window's list of files that
    /// are not live. The thread shows, over the instructions too. Whether it went; once it
    /// went, the composer has the keys, as after a message.
    pub fn ask(&mut self, message: &str, project: &mut Project) -> bool {
        self.close_instructions();
        let started = self.started();
        if started {
            self.deliver(message.to_string(), Vec::new(), Vec::new(), project);
        }
        started
    }

    /// Under a turn that failed: its message again, as it was sent.
    pub fn try_again(&mut self, project: &mut Project) -> bool {
        let last = self.conversation.entries().len().saturating_sub(1);
        match self.retry_message(last).map(str::to_string) {
            Some(message) => self.ask(&message, project),
            None => false,
        }
    }

    /// Whether the turn at `index` is the last thing of a thread that can go on, with nothing
    /// asked of the composer: what shows under it can send a message.
    pub(super) fn goes_on_after(&self, index: usize) -> bool {
        index + 1 == self.conversation.entries().len()
            && self.conversation.can_continue()
            && !self.conversation.is_working()
    }

    /// The message of the turn at `index` to send again: the last turn, which failed, of a
    /// thread that can go on, when its message had nothing attached (a saved thread keeps no
    /// pictures).
    pub(super) fn retry_message(&self, index: usize) -> Option<&str> {
        let entries = self.conversation.entries();
        let failed = matches!(
            entries.get(index),
            Some(Entry::Turn(Turn {
                end: Some(TurnEnd {
                    outcome: TurnOutcome::Failed { .. },
                    ..
                }),
                ..
            }))
        );
        if !failed || !self.goes_on_after(index) {
            return None;
        }
        match entries.get(index.checked_sub(1)?)? {
            Entry::Message { text, attachments } if attachments.is_empty() => Some(text),
            _ => None,
        }
    }

    /// Whether a message can go now, with the agent started for it.
    fn started(&mut self) -> bool {
        let ready = self.loading.is_none()
            && self.opening.is_none()
            && self.agents.settings().is_read()
            && self.conversation.can_continue();
        if self.conversation.is_working() || !ready {
            return false;
        }
        if self.agent.is_some() {
            return true;
        }
        let Some((provider, installed)) = self.ready_program() else {
            return false;
        };
        match self.start(provider, installed) {
            Ok(agent) => {
                self.attach(agent);
                true
            }
            Err(error) => {
                let name = provider.name();
                self.conversation
                    .notice(format!("{name} did not start: {error}"));
                false
            }
        }
    }

    fn deliver(
        &mut self,
        message: String,
        names: Vec<String>,
        attachments: Vec<Attachment>,
        project: &mut Project,
    ) {
        self.begin_at(&message, names, SystemTime::now(), project);
        let message = Message {
            text: message,
            attachments,
            note: self.take_went_back_note(),
        };
        let sent = self.agent.as_ref().map(|agent| agent.thread.send(message));
        if let Some(Err(error)) = sent {
            let outcome = TurnOutcome::Failed {
                message: error.to_string(),
            };
            self.receive([AgentEvent::TurnEnded { outcome }], project);
        }
    }

    /// Shows the composer's message, with the names of what it has attached, and opens its
    /// request, as a send does before the message goes to the agent. With no agent, for a
    /// thread fed recorded events: `started` is when the turn began, so it shows how long it
    /// worked.
    pub fn begin_at(
        &mut self,
        message: &str,
        attachments: Vec<String>,
        started: SystemTime,
        project: &mut Project,
    ) {
        project.begin_request(&request_label(message));
        self.problems_before = project.problems();
        self.just_ended = None;
        self.conversation
            .send(message, attachments.clone(), started);
        let saved = self.saved.get_or_insert_with(SavedThread::fresh);
        let line = Line::Sent {
            at: started,
            message: message.to_string(),
            attachments,
        };
        let writes = [
            Write::Current(Some(saved.clone())),
            Write::Lines {
                thread: saved.id.clone(),
                lines: vec![line],
            },
        ];
        self.keep(writes);
    }

    /// What the agent did, in order. [`Self::poll`] hands the process's events here; a thread
    /// with no agent is fed recorded ones.
    pub fn receive(&mut self, events: impl IntoIterator<Item = AgentEvent>, project: &mut Project) {
        let now = SystemTime::now();
        let mut lines = Vec::new();
        let mut failed = false;
        for event in events {
            lines.extend(Line::of(&event, now));
            match &event {
                AgentEvent::Started { account, models } => {
                    let provider = self.provider();
                    (self.agents.settings_mut()).set_models(provider, models.clone());
                    // The account the running agent reports is the newest word on it.
                    if let Some(account) = account.clone() {
                        self.agents.signed_in_as(provider, account);
                    }
                }
                AgentEvent::TurnEnded {
                    outcome: TurnOutcome::Failed { .. },
                } => failed = true,
                _ => {}
            }
            let ends = matches!(
                event,
                AgentEvent::TurnEnded { .. } | AgentEvent::Exited { .. }
            );
            // Only the end of a turn that works ends its request: an exit after the turn
            // ended must not move the end of a request that finished long ago.
            let ends_turn = ends && self.conversation.is_working();
            if ends_turn {
                project.end_request();
            }
            if matches!(event, AgentEvent::Exited { .. }) {
                // The next message starts a new process.
                self.agent = None;
            }
            if let Some(index) = self.conversation.apply(event, now)
                && ends_turn
            {
                self.keep_problems_left(index, project);
            }
        }
        // A thread is saved from its first message on.
        if let Some(thread) = self.saved.as_ref().map(|saved| saved.id.clone())
            && !lines.is_empty()
        {
            self.keep([Write::Lines { thread, lines }]);
        }
        if failed {
            let provider = self.provider();
            self.agents.check_still_signed_in(provider);
        }
    }

    /// Keeps what problems the turn at `index` left that were not there when it began. Nothing
    /// goes to the agent: its docs tell it to read `problems.txt` itself.
    fn keep_problems_left(&mut self, index: usize, project: &Project) {
        let left: Vec<Problem> = (project.problems().into_iter())
            .filter(|problem| !self.problems_before.contains(problem))
            .collect();
        if left.is_empty() {
            self.problems_left.remove(&index);
        } else {
            self.problems_left.insert(index, left);
        }
        self.just_ended = Some((index, Instant::now()));
    }

    /// The watcher applies what it heard after its grouping window, so the last writes of a
    /// turn may come just after the turn ended. Their problems are that turn's too.
    pub(super) fn problems_changed(&mut self, project: &Project) {
        let Some((index, ended)) = self.just_ended else {
            return;
        };
        if ended.elapsed() > GROUPING_WINDOW {
            self.just_ended = None;
            return;
        }
        self.keep_problems_left(index, project);
        // The turn's own end still counts from when it ended.
        self.just_ended = Some((index, ended));
    }

    fn start(&mut self, provider: Provider, installed: Installed) -> io::Result<Agent> {
        let settings = self.agents.settings();
        // The flag needs a file that exists. One look, beside starting a process.
        let instructions =
            (settings.instructions_file()).filter(|instructions| instructions.is_file());
        let (thread, mut events) = Thread::start(ThreadOptions {
            provider,
            installed,
            folder: self.folder.clone(),
            model: settings.settings().model(provider).map(str::to_string),
            approval_mode: settings.settings().approval_mode,
            resume: self.resume(),
            instructions,
        })?;
        self.agents_started += 1;
        let id = self.agents_started;
        let outbox = self.inbox.outbox();
        let reading = jobs::spawn(async move {
            while let Some(event) = events.next().await {
                outbox.send(Done::Event(id, event));
            }
        });
        Ok(Agent {
            provider,
            thread,
            id,
            _reading: Some(reading),
        })
    }

    /// Talks to `thread` from now on, in place of starting a process at the next send: a
    /// thread with no agent ([`Thread::without_agent`]), whose events [`Self::receive`] is
    /// fed.
    pub fn connect(&mut self, thread: Thread) {
        self.agents_started += 1;
        let agent = Agent {
            provider: self.provider(),
            thread,
            id: self.agents_started,
            _reading: None,
        };
        self.attach(agent);
    }

    /// Talks to `agent` from now on. Its session is the thread's: the next message saves it,
    /// before the agent has said anything, so a thread quit early still resumes. A thread
    /// keeps its provider: the picker offers only its models from now on.
    fn attach(&mut self, agent: Agent) {
        let saved = self.saved.get_or_insert_with(SavedThread::fresh);
        saved.session_id = Some(agent.thread.session_id().to_string());
        saved.provider = agent.provider;
        self.agent = Some(agent);
    }

    /// The send button while the agent works, and cmd-.: stops the turn.
    pub fn stop(&mut self) {
        if !self.conversation.is_working() {
            return;
        }
        let stopped = self.agent.as_ref().map(|agent| agent.thread.interrupt());
        if let Some(Err(error)) = stopped {
            self.conversation.notice(error.to_string());
        }
    }

    /// Answers the question shown. Nothing is modal: the question waits as the last thing of
    /// the thread while music and editing go on.
    pub fn answer(&mut self, answer: ApprovalAnswer) {
        let Some((approval, _)) = self.conversation.answered() else {
            return;
        };
        let answered = (self.agent.as_ref()).map(|agent| agent.thread.answer(approval, answer));
        if let Some(Err(error)) = answered {
            self.conversation.notice(error.to_string());
        }
    }

    /// Applies at once, also to a running agent from its next tool on, and to every thread
    /// after.
    pub fn set_approval_mode(&mut self, mode: ApprovalMode) {
        if !self.agents.settings_mut().set_approval_mode(mode) {
            return;
        }
        let sent = self
            .agent
            .as_ref()
            .map(|agent| agent.thread.set_approval_mode(mode));
        if let Some(Err(error)) = sent {
            self.conversation.notice(error.to_string());
        }
    }

    /// Picks a model of `provider`, for this thread and the next ones. A running agent of the
    /// same provider takes it at once.
    pub fn pick_model(&mut self, provider: Provider, model: &str) {
        if self
            .thread_provider()
            .is_some_and(|locked| locked != provider)
        {
            return;
        }
        let changed = (self.agents.settings_mut()).set_model(provider, Some(model.to_string()));
        let sent = (self.agent.as_ref())
            .filter(|agent| changed && agent.provider == provider)
            .map(|agent| agent.thread.set_model(model));
        if let Some(Err(error)) = sent {
            self.conversation.notice(error.to_string());
        }
    }

    /// Ends the thread's process. A dropped process says nothing more, so the turn it was
    /// working on ends here as stopped, and its request with it.
    pub(super) fn end_agent(&mut self, project: &mut Project) {
        self.agent = None;
        if self.conversation.is_working() {
            let outcome = TurnOutcome::Interrupted;
            self.receive([AgentEvent::TurnEnded { outcome }], project);
        }
    }
}
