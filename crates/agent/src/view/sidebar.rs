//! The sidebar: the header with **+**, the thread in a gpui `list`, and the composer.
//!
//! One thread per sidebar. Its process starts on the first send and ends with **+**. Every
//! message is one request of the session, so the agent's file writes for it are one undo step.
//! The thread is saved on the machine as it goes (`crate::store`), and the sidebar opens on the
//! project's last one.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use gpui::{
    AnyElement, App, BoxShadow, Context, Entity, FocusHandle, Focusable, FollowMode, FontWeight,
    Global, KeyBinding, ListAlignment, ListState, Subscription, Task, Window, actions, div, hsla,
    list, point, prelude::*, px,
};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::text_input::TextInput;

use super::entry;
use crate::conversation::{Conversation, Entry, request_label};
use crate::store::{Line, SavedThread, ThreadStore, Write};
use crate::{
    AgentEvent, ApprovalAnswer, ApprovalMode, Events, Provider, Session, Thread, ThreadOptions,
    TurnOutcome, login_shell_environment, program_on_path,
};

actions!(agent_sidebar, [Stop]);

const KEY_CONTEXT: &str = "AgentSidebar";

/// Events that come within one frame are applied together, so a fast stream of text measures
/// the growing entry once a frame and not once a token.
const FRAME: Duration = Duration::from_millis(16);

/// How far the list draws beyond what shows, so a short scroll needs no measuring.
const OVERDRAW: f32 = 800.;

struct BindingsInstalled;
impl Global for BindingsInstalled {}

/// Binds the keys of the sidebar once per process, as the text input does.
fn install_bindings(cx: &mut App) {
    if cx.has_global::<BindingsInstalled>() {
        return;
    }
    cx.set_global(BindingsInstalled);
    cx.bind_keys([KeyBinding::new("cmd-.", Stop, Some(KEY_CONTEXT))]);
}

/// Where Claude Code is, and the environment it runs in.
#[derive(Clone, Debug)]
pub struct Installed {
    pub program: PathBuf,
    pub environment: HashMap<OsString, OsString>,
}

enum Claude {
    /// The login shell is still asked for its environment.
    Looking,
    Missing,
    Found(Installed),
}

/// The process of the thread, from the first send until **+** or until it ends.
struct Agent {
    thread: Thread,
    /// Reads the process on the background executor. Dropping it ends the process.
    _reading: Task<()>,
    /// Hands what was read to the sidebar, once a frame.
    _delivering: Task<()>,
}

pub struct Sidebar {
    session: Entity<sound_ui::Session>,
    claude: Claude,
    conversation: Conversation,
    list: ListState,
    /// The finished turns whose steps show, by entry.
    expanded: HashSet<usize>,
    input: Entity<TextInput>,
    /// The sidebar itself, for when it has no composer.
    focus_handle: FocusHandle,
    new_thread_focus: FocusHandle,
    send_focus: FocusHandle,
    /// Allow, Allow for this thread, Deny.
    approval_focus: [FocusHandle; 3],
    agent: Option<Agent>,
    /// The thread shown, as it is saved. `None` until its first message.
    thread: Option<SavedThread>,
    /// Hands what to keep of the thread to its writer, in order. `None` while nothing is
    /// saved, as in a snapshot.
    writes: Option<smol::channel::Sender<Write>>,
    /// Reads the last thread of the project. No message goes until it is in.
    loading: Option<Task<()>>,
    _input: Subscription,
    _finding: Option<Task<()>>,
}

impl Sidebar {
    /// Finds `claude` on the `PATH` of the login shell, in the background. The threads of
    /// the project are kept in `threads`, `agent/threads` in the support folder of the
    /// machine; with `None` nothing is saved.
    pub fn new(
        session: Entity<sound_ui::Session>,
        threads: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut sidebar = Self::with(session, Claude::Looking, threads, cx);
        sidebar._finding = Some(cx.spawn(async move |sidebar, cx| {
            let found = cx
                .background_spawn(async {
                    let (environment, error) = match login_shell_environment().await {
                        Ok(environment) => (environment, None),
                        Err(error) => (std::env::vars_os().collect(), Some(error)),
                    };
                    let program = program_on_path("claude", &environment);
                    let installed = program.map(|program| Installed {
                        program,
                        environment,
                    });
                    (installed, error)
                })
                .await;
            // A sidebar that went in the meantime has nobody to tell.
            sidebar
                .update(cx, |sidebar, cx| sidebar.found(found, cx))
                .ok();
        }));
        sidebar
    }

    /// With `claude` given, or known to be missing, for a test or a snapshot.
    pub fn with_claude(
        session: Entity<sound_ui::Session>,
        installed: Option<Installed>,
        threads: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        let claude = installed.map_or(Claude::Missing, Claude::Found);
        Self::with(session, claude, threads, cx)
    }

    fn with(
        session: Entity<sound_ui::Session>,
        claude: Claude,
        threads: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Self {
        install_bindings(cx);
        let loading = threads.map(|threads| {
            let project = session.read(cx).project().root().to_path_buf();
            Self::load(threads, project, cx)
        });
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder("Ask for a change")
                .multi_line(8)
                .bare(true)
        });
        let weak = cx.weak_entity();
        input.update(cx, |input, _| {
            input.set_on_submit(move |_, _, cx| {
                let weak = weak.clone();
                // The handler runs inside the input's own update, and sending clears it.
                cx.defer(move |cx| {
                    // A sidebar that went in the meantime sends nothing.
                    weak.update(cx, |sidebar, cx| sidebar.send(cx)).ok();
                });
            });
        });
        let list = ListState::new(0, ListAlignment::Top, px(OVERDRAW));
        list.set_follow_mode(FollowMode::Tail);
        Self {
            session,
            claude,
            conversation: Conversation::default(),
            list,
            expanded: HashSet::new(),
            _input: cx.observe(&input, |_, _, cx| cx.notify()),
            input,
            focus_handle: cx.focus_handle(),
            new_thread_focus: cx.focus_handle().tab_stop(true),
            send_focus: cx.focus_handle().tab_stop(true),
            approval_focus: [(); 3].map(|_| cx.focus_handle().tab_stop(true)),
            agent: None,
            thread: None,
            writes: None,
            loading,
            _finding: None,
        }
    }

    /// Reads the current thread of `project` from `threads` in the background.
    fn load(threads: PathBuf, project: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |sidebar, cx| {
            let (store, current) = cx
                .background_spawn(async move {
                    let store = ThreadStore::new(&threads, &project);
                    let current = store.current();
                    (store, current)
                })
                .await;
            // A sidebar that went in the meantime shows nothing.
            sidebar
                .update(cx, |sidebar, cx| sidebar.loaded(store, current, cx))
                .ok();
        })
    }

    fn loaded(
        &mut self,
        store: ThreadStore,
        current: Result<Option<(SavedThread, Conversation)>, String>,
        cx: &mut Context<Self>,
    ) {
        self.loading = None;
        self.writes = Some(write_in_order(store, cx));
        match current {
            Ok(Some((thread, conversation))) => {
                self.thread = Some(thread);
                // Only notices came meanwhile: no message goes while it loads.
                let meanwhile = std::mem::replace(&mut self.conversation, conversation);
                self.conversation.append(meanwhile);
            }
            Ok(None) => {}
            Err(error) => self
                .conversation
                .notice(format!("The saved thread could not be read: {error}")),
        }
        self.expanded.clear();
        self.list.reset(self.conversation.entries().len());
        cx.notify();
    }

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// What the next process of the thread continues: the thread's own session once its
    /// agent has started, so a thread opened again resumes where it left off.
    pub fn next_session(&self) -> Session {
        self.thread
            .as_ref()
            .map_or(Session::New, SavedThread::session)
    }

    fn keep(&self, writes: impl IntoIterator<Item = Write>) {
        let Some(sender) = &self.writes else {
            return;
        };
        for write in writes {
            // The writer ends only with the sidebar, which owns the sender.
            sender.try_send(write).ok();
        }
    }

    /// Whether the agent works or waits on the composer, so a closed sidebar can say so.
    pub fn is_busy(&self, _: &App) -> bool {
        self.conversation.is_working()
    }

    /// Shows the composer's message and opens its request. [`Self::send`] calls it before the
    /// message goes to the agent; a test calls it to replay a recorded turn with no process.
    pub fn begin(&mut self, message: &str, cx: &mut Context<Self>) {
        self.begin_at(message, SystemTime::now(), cx);
    }

    /// [`Self::begin`] with the turn started at `started`, so a snapshot shows how long it
    /// worked.
    pub fn begin_at(&mut self, message: &str, started: SystemTime, cx: &mut Context<Self>) {
        let label = request_label(message);
        self.session
            .update(cx, |session, _| session.begin_request(&label));
        self.conversation.send(message, started);
        let thread = self.thread.get_or_insert_with(SavedThread::fresh);
        let line = Line::Sent {
            at: started,
            message: message.to_string(),
        };
        let writes = [
            Write::Current(Some(thread.clone())),
            Write::Lines {
                thread: thread.id.clone(),
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
        for event in events {
            lines.extend(Line::of(&event, now));
            let ends = matches!(
                event,
                AgentEvent::TurnEnded { .. } | AgentEvent::Exited { .. }
            );
            // Only the end of a turn that works ends its request: an exit after the turn
            // ended must not move the end of a request that finished long ago.
            if ends && self.conversation.is_working() {
                self.session.update(cx, |session, _| session.end_request());
            }
            if matches!(event, AgentEvent::Exited { .. }) {
                // The next message starts a new process.
                self.agent = None;
            }
            if let Some(index) = self.conversation.apply(event, now) {
                changed = Some(changed.map_or(index, |earlier| earlier.min(index)));
            }
        }
        // A thread is saved from its first message on.
        if let Some(thread) = &self.thread
            && !lines.is_empty()
        {
            let thread = thread.id.clone();
            self.keep([Write::Lines { thread, lines }]);
        }
        self.show(changed, cx);
    }

    /// Tells the list what changed: entries added at the end, and `changed` and what comes
    /// after it measured again, for an entry that grew.
    fn show(&mut self, changed: Option<usize>, cx: &mut Context<Self>) {
        let shown = self.list.item_count();
        if let Some(index) = changed.filter(|index| *index < shown) {
            self.list.remeasure_items(index..shown);
        }
        let count = self.conversation.entries().len();
        if count > shown {
            self.list.splice(shown..shown, count - shown);
        }
        cx.notify();
    }

    fn found(
        &mut self,
        (installed, error): (Option<Installed>, Option<io::Error>),
        cx: &mut Context<Self>,
    ) {
        if let Some(error) = error {
            self.conversation.notice(format!(
                "The agent runs without the settings of your shell: {error}."
            ));
        }
        self.claude = installed.map_or(Claude::Missing, Claude::Found);
        self.show(None, cx);
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let message = self.input.read(cx).text().trim().to_string();
        let ready = self.loading.is_none() && self.conversation.can_continue();
        if message.is_empty() || self.conversation.is_working() || !ready {
            return;
        }
        if self.agent.is_none() {
            let Claude::Found(installed) = &self.claude else {
                return;
            };
            match self.start(installed.clone(), cx) {
                Ok(agent) => self.attach(agent),
                Err(error) => {
                    self.conversation
                        .notice(format!("Claude Code did not start: {error}"));
                    self.show(None, cx);
                    return;
                }
            }
        }
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.begin(&message, cx);
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
        let (thread, events) = Thread::start(ThreadOptions {
            provider: Provider::Claude,
            program: installed.program,
            folder,
            // The CLI's own default until the model picker comes.
            model: None,
            approval_mode: ApprovalMode::AskBeforeCommands,
            session: self.next_session(),
            environment: installed.environment,
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
        let thread = self.thread.get_or_insert_with(SavedThread::fresh);
        thread.session_id = Some(agent.thread.session_id().to_string());
        self.agent = Some(agent);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.conversation.is_working() {
            return;
        }
        let stopped = self.agent.as_ref().map(|agent| agent.thread.interrupt());
        if let Some(Err(error)) = stopped {
            self.conversation.notice(error.to_string());
            self.show(None, cx);
        }
    }

    fn answer(&mut self, answer: ApprovalAnswer, cx: &mut Context<Self>) {
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

    /// Drops the thread and its process, and starts empty.
    fn new_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agent = None;
        // The old one stays in the store, and no thread is current until the next message.
        self.thread = None;
        self.keep([Write::Current(None)]);
        if self.conversation.is_working() {
            self.session.update(cx, |session, _| session.end_request());
        }
        self.conversation = Conversation::default();
        self.expanded.clear();
        self.list.reset(0);
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    fn toggle_steps(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&index) {
            self.expanded.insert(index);
        }
        self.list.remeasure_items(index..index + 1);
        cx.notify();
    }

    fn render_entry(&mut self, index: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.conversation.entries().get(index) {
            Some(Entry::Message(text)) => entry::message(text, cx),
            Some(Entry::Notice(text)) => entry::notice(text, cx),
            Some(Entry::Turn(turn)) => {
                let approval = turn
                    .approval
                    .as_ref()
                    .map(|approval| self.approval_row(&approval.title, cx));
                let toggle = cx.listener(move |sidebar, _, _, cx| sidebar.toggle_steps(index, cx));
                let expanded = self.expanded.contains(&index);
                entry::turn(turn, index, expanded, toggle, approval, cx)
            }
            None => div().into_any_element(),
        };
        div()
            .px(px(24.))
            .pb(px(24.))
            .when(index == 0, |item| item.pt(px(8.)))
            .child(content)
            .into_any_element()
    }

    /// The question the agent waits on, as the last thing of the thread. Nothing is modal:
    /// music and editing go on while it waits.
    fn approval_row(&self, title: &str, cx: &mut Context<Self>) -> AnyElement {
        let lavender = cx.theme().lavender;
        let answers = [
            ("allow", "Allow", ApprovalAnswer::Allow),
            (
                "allow-for-thread",
                "Allow for this thread",
                ApprovalAnswer::AllowForThread,
            ),
            ("deny", "Deny", ApprovalAnswer::Deny),
        ];
        let buttons =
            answers
                .into_iter()
                .zip(&self.approval_focus)
                .map(|((id, label, answer), focus)| {
                    let variant = match answer {
                        ApprovalAnswer::Allow => ButtonVariant::SubtleColor(lavender),
                        ApprovalAnswer::AllowForThread | ApprovalAnswer::Deny => {
                            ButtonVariant::GhostColor(lavender)
                        }
                    };
                    Button::new(id, label)
                        .debug_selector(move || format!("approval-{id}"))
                        .variant(variant)
                        .size(ButtonSize::Sm)
                        .focus_handle(focus)
                        .on_click(cx.listener(move |sidebar, _, _, cx| sidebar.answer(answer, cx)))
                });
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_color(lavender)
            .child(
                div()
                    .text_size(px(15.))
                    .line_height(px(22.))
                    .child(title.to_string()),
            )
            .child(div().flex().flex_wrap().gap(px(8.)).children(buttons))
            .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(56.))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Agent"),
            )
            .child(
                div().absolute().right(px(16.)).child(
                    Button::icon_only("new-thread", "plus")
                        .debug_selector(|| "agent-new-thread".to_string())
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Xs)
                        .focus_handle(&self.new_thread_focus)
                        .on_click(
                            cx.listener(|sidebar, _, window, cx| sidebar.new_thread(window, cx)),
                        ),
                ),
            )
    }

    fn composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, fill) = (theme.alpha_at(0.10), theme.gray_50);
        let working = self.conversation.is_working();
        let can_send = !self.input.read(cx).text().trim().is_empty();
        // While a turn runs the send button stops it: one turn at a time.
        let button = if working {
            Button::icon_only("stop", "square")
                .debug_selector(|| "agent-stop".to_string())
                .on_click(cx.listener(|sidebar, _, _, cx| sidebar.stop(cx)))
        } else {
            Button::icon_only("send", "arrow-up")
                .disabled(!can_send)
                .on_click(cx.listener(|sidebar, _, _, cx| sidebar.send(cx)))
        };
        div().flex_none().p(px(24.)).pt(px(8.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .p(px(16.))
                .rounded(px(16.))
                .border_1()
                .border_color(border)
                .bg(fill)
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.25),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(24.),
                    spread_radius: px(-8.),
                    inset: false,
                }])
                .child(self.input.clone())
                .child(
                    div().flex().justify_end().child(
                        button
                            .variant(ButtonVariant::Subtle)
                            .size(ButtonSize::Sm)
                            .rounded(true)
                            .focus_handle(&self.send_focus),
                    ),
                ),
        )
    }

    /// In place of the composer once the agent lost the session: the thread stays to read,
    /// and **+** is the way on.
    fn cannot_continue(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .p(px(24.))
            .pt(px(8.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.))
                    .text_color(cx.theme().gray_700)
                    .child("This thread can't continue. Start a new one."),
            )
            .child(
                Button::icon_only("new-thread-instead", "plus")
                    .debug_selector(|| "agent-cannot-continue".to_string())
                    .variant(ButtonVariant::Subtle)
                    .size(ButtonSize::Sm)
                    .rounded(true)
                    .focus_handle(&self.send_focus)
                    .on_click(cx.listener(|sidebar, _, window, cx| sidebar.new_thread(window, cx))),
            )
    }
}

/// Hands each write to the background in turn, so the files get them in order. Detached: it
/// ends when the sidebar does, after the writes still queued.
fn write_in_order(store: ThreadStore, cx: &mut Context<Sidebar>) -> smol::channel::Sender<Write> {
    let (sender, receiver) = smol::channel::unbounded::<Write>();
    cx.spawn(async move |sidebar, cx| {
        let mut told = false;
        while let Ok(write) = receiver.recv().await {
            let store = store.clone();
            let written = cx
                .background_spawn(async move { store.write(&write) })
                .await;
            // Said once: a full disk would otherwise add a line at every event.
            if let Err(error) = written
                && !told
            {
                told = true;
                // A sidebar that went has nobody to tell.
                sidebar
                    .update(cx, |sidebar, cx| {
                        let notice = format!("This thread is not saved: {error}");
                        sidebar.conversation.notice(notice);
                        sidebar.show(None, cx);
                    })
                    .ok();
            }
        }
    })
    .detach();
    sender
}

/// Every event of the process, until it ends or the sidebar stops listening.
async fn read(mut events: Events, sender: smol::channel::Sender<AgentEvent>) {
    while let Some(event) = events.next().await {
        if sender.send(event).await.is_err() {
            break;
        }
    }
}

impl Focusable for Sidebar {
    /// The composer, which cmd-L focuses, or the sidebar itself while it has none, so cmd-L
    /// and escape still work there.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.claude {
            Claude::Found(_) if self.conversation.can_continue() => self.input.focus_handle(cx),
            Claude::Found(_) | Claude::Looking | Claude::Missing => self.focus_handle.clone(),
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, border, text, muted) = (
            theme.gray_50,
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.gray_700,
        );
        let body = match &self.claude {
            Claude::Looking => div().flex_1().into_any_element(),
            Claude::Missing => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .px(px(24.))
                .text_size(px(14.))
                .text_color(muted)
                .child("Claude Code is not installed.")
                .into_any_element(),
            Claude::Found(_) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    list(self.list.clone(), cx.processor(Self::render_entry))
                        .flex_1()
                        .min_h_0(),
                )
                .map(|body| {
                    if self.conversation.can_continue() {
                        body.child(self.composer(cx))
                    } else {
                        body.child(self.cannot_continue(cx))
                    }
                })
                .into_any_element(),
        };
        div()
            .id("agent-sidebar")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|sidebar, _: &Stop, _, cx| sidebar.stop(cx)))
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .border_r_1()
            .border_color(border)
            .text_color(text)
            .child(self.header(cx))
            .child(body)
    }
}
