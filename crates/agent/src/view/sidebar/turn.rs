//! A turn of the agent: its process, from the first send until it ends, the message and
//! what the agent did, its approvals, and the problems a turn left.

use std::io;
use std::time::{Duration, Instant, SystemTime};

use gpui::{App, Context, Task, prelude::*};
use sound_core::{GROUPING_WINDOW, Problem};

use super::{Agent, Sidebar};
use crate::conversation::request_label;
use crate::store::{Line, SavedThread, Write};
use crate::view::onboarding::Setup;
use crate::{
    AgentEvent, ApprovalAnswer, Attachment, Events, Installed, Message, Thread, ThreadOptions,
    TurnOutcome,
};

/// Events that come within one frame are applied together, so a fast stream of text measures
/// the growing entry once a frame and not once a token.
const FRAME: Duration = Duration::from_millis(16);

impl Sidebar {
    /// Shows the composer's message and opens its request. [`Self::send`] calls it before the
    /// message goes to the agent; a test calls it to replay a recorded turn with no process.
    pub fn begin(&mut self, message: &str, cx: &mut Context<Self>) {
        self.begin_at(message, Vec::new(), SystemTime::now(), cx);
    }

    /// [`Self::begin`] with the names of what the message has attached, and the turn started
    /// at `started`, so a snapshot shows how long it worked.
    pub fn begin_at(
        &mut self,
        message: &str,
        attachments: Vec<String>,
        started: SystemTime,
        cx: &mut Context<Self>,
    ) {
        let label = request_label(message);
        self.session
            .update(cx, |session, _| session.begin_request(&label));
        self.problems_before = self.session.read(cx).project().problems();
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
        self.show(None, cx);
    }

    /// What the agent did, in order. The process hands its events here once a frame; a test
    /// calls it with recorded ones.
    pub fn receive(
        &mut self,
        events: impl IntoIterator<Item = AgentEvent>,
        cx: &mut Context<Self>,
    ) {
        let now = SystemTime::now();
        let mut changed: Option<usize> = None;
        let mut lines = Vec::new();
        let mut failed = false;
        for event in events {
            lines.extend(Line::of(&event, now));
            match &event {
                AgentEvent::Started {
                    account, models, ..
                } => {
                    let models = models.clone();
                    // Every sidebar's menu follows, by observing the settings.
                    self.settings
                        .update(cx, |settings, cx| settings.set_models(models, cx));
                    // The account the running agent reports is the newest word on it.
                    if matches!(self.setup, Setup::Ready { .. }) {
                        let account = account.clone();
                        self.set_setup(Setup::Ready { account }, cx);
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
                self.session.update(cx, |session, _| session.end_request());
            }
            if matches!(event, AgentEvent::Exited { .. }) {
                // The next message starts a new process.
                self.agent = None;
            }
            if let Some(index) = self.conversation.apply(event, now) {
                changed = Some(changed.map_or(index, |earlier| earlier.min(index)));
                if ends_turn {
                    self.keep_problems_left(index, cx);
                }
            }
        }
        // A thread is saved from its first message on.
        if let Some(saved) = &self.saved
            && !lines.is_empty()
        {
            let thread = saved.id.clone();
            self.keep([Write::Lines { thread, lines }]);
        }
        self.show(changed, cx);
        if failed {
            self.check_still_signed_in(cx);
        }
    }

    /// Keeps what problems the turn at `index` left that were not there when it began. Nothing
    /// goes to the agent: its docs tell it to read `problems.txt` itself.
    fn keep_problems_left(&mut self, index: usize, cx: &App) {
        let left: Vec<Problem> = self
            .session
            .read(cx)
            .project()
            .problems()
            .into_iter()
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
    pub(super) fn problems_changed(&mut self, cx: &mut Context<Self>) {
        let Some((index, ended)) = self.just_ended else {
            return;
        };
        if ended.elapsed() > GROUPING_WINDOW {
            self.just_ended = None;
            return;
        }
        self.keep_problems_left(index, cx);
        // The turn's own end still counts from when it ended.
        self.just_ended = Some((index, ended));
        self.list.remeasure_items(index..index + 1);
        cx.notify();
    }

    /// The composer's message, with what it has attached.
    pub(super) fn send(&mut self, cx: &mut Context<Self>) {
        let message = self.input.read(cx).text().trim().to_string();
        if message.is_empty() || self.attachments_reading > 0 || !self.started(cx) {
            return;
        }
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.history.reset();
        let (names, attachments) = std::mem::take(&mut self.attachments)
            .into_iter()
            .map(|attached| (attached.read.name, attached.read.attachment))
            .unzip();
        self.deliver(message, names, attachments, cx);
    }

    /// `message` with nothing attached, and whether it went. The composer keeps what is in it.
    pub(super) fn send_text(&mut self, message: String, cx: &mut Context<Self>) -> bool {
        let started = self.started(cx);
        if started {
            self.deliver(message, Vec::new(), Vec::new(), cx);
        }
        started
    }

    /// Whether a message can go now, with the agent started for it.
    fn started(&mut self, cx: &mut Context<Self>) -> bool {
        let ready = self.loading.is_none()
            && self.opening.is_none()
            && self.settings.read(cx).is_read()
            && self.conversation.can_continue();
        if self.conversation.is_working() || !ready {
            return false;
        }
        if self.agent.is_some() {
            return true;
        }
        let (Setup::Ready { .. }, Some(installed)) = (&self.setup, &self.installed) else {
            return false;
        };
        match self.start(installed.clone(), cx) {
            Ok(agent) => {
                self.attach(agent);
                true
            }
            Err(error) => {
                let name = self.provider.name();
                self.conversation
                    .notice(format!("{name} did not start: {error}"));
                self.show(None, cx);
                false
            }
        }
    }

    fn deliver(
        &mut self,
        message: String,
        names: Vec<String>,
        attachments: Vec<Attachment>,
        cx: &mut Context<Self>,
    ) {
        self.begin_at(&message, names, SystemTime::now(), cx);
        let message = Message {
            text: message,
            attachments,
            note: self.take_went_back_note(cx),
        };
        let sent = self.agent.as_ref().map(|agent| agent.thread.send(message));
        if let Some(Err(error)) = sent {
            let outcome = TurnOutcome::Failed {
                message: error.to_string(),
            };
            self.receive([AgentEvent::TurnEnded { outcome }], cx);
        }
    }

    fn start(&self, installed: Installed, cx: &mut Context<Self>) -> io::Result<Agent> {
        let folder = self.session.read(cx).project().root().to_path_buf();
        let settings = self.settings.read(cx).settings().clone();
        // The flag needs a file that exists. One look, beside starting a process.
        let instructions = (self.settings.read(cx).instructions_file())
            .filter(|instructions| instructions.is_file());
        let (thread, events) = Thread::start(ThreadOptions {
            provider: self.provider,
            installed,
            folder,
            model: settings.model.clone(),
            approval_mode: settings.approval_mode,
            resume: self.resume(),
            instructions,
        })?;
        let (sender, receiver) = smol::channel::unbounded();
        let reading = cx.background_spawn(read(events, sender));
        let delivering = cx.spawn(async move |sidebar, cx| {
            while let Ok(first) = receiver.recv().await {
                let mut batch = vec![first];
                while let Ok(event) = receiver.try_recv() {
                    batch.push(event);
                }
                if sidebar
                    .update(cx, |sidebar, cx| sidebar.receive(batch, cx))
                    .is_err()
                {
                    break;
                }
                // What comes during the wait is the next batch.
                cx.background_executor().timer(FRAME).await;
            }
        });
        Ok(Agent {
            thread,
            _reading: reading,
            _delivering: delivering,
        })
    }

    /// Talks to `thread` from now on, in place of starting a process at the next send. For a
    /// test with [`Thread::without_agent`], which hands the events to [`Self::receive`].
    pub fn connect(&mut self, thread: Thread) {
        self.attach(Agent {
            thread,
            _reading: Task::ready(()),
            _delivering: Task::ready(()),
        });
    }

    /// Talks to `agent` from now on. Its session is the thread's: the next message saves it,
    /// before the agent has said anything, so a thread quit early still resumes.
    fn attach(&mut self, agent: Agent) {
        let saved = self.saved.get_or_insert_with(SavedThread::fresh);
        saved.session_id = Some(agent.thread.session_id().to_string());
        self.agent = Some(agent);
    }

    pub(super) fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.conversation.is_working() {
            return;
        }
        let stopped = self.agent.as_ref().map(|agent| agent.thread.interrupt());
        if let Some(Err(error)) = stopped {
            self.conversation.notice(error.to_string());
            self.show(None, cx);
        }
    }

    pub(super) fn answer(&mut self, answer: ApprovalAnswer, cx: &mut Context<Self>) {
        let Some((approval, turn)) = self.conversation.answered() else {
            return;
        };
        let answered = self
            .agent
            .as_ref()
            .map(|agent| agent.thread.answer(approval, answer));
        if let Some(Err(error)) = answered {
            self.conversation.notice(error.to_string());
        }
        self.show(Some(turn), cx);
    }

    /// Ends the thread's process. A dropped process says nothing more, so the turn it was
    /// working on ends here as stopped, and its request with it.
    pub(super) fn end_agent(&mut self, cx: &mut Context<Self>) {
        self.agent = None;
        if self.conversation.is_working() {
            let outcome = TurnOutcome::Interrupted;
            self.receive([AgentEvent::TurnEnded { outcome }], cx);
        }
    }
}

/// Every event of the process, until it ends or the sidebar stops listening.
async fn read(mut events: Events, sender: smol::channel::Sender<AgentEvent>) {
    while let Some(event) = events.next().await {
        if sender.send(event).await.is_err() {
            break;
        }
    }
}
