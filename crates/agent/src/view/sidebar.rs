//! The sidebar: the header with **New**, the onboarding until the agent is set up, then the
//! thread in a gpui `list` and the composer.
//!
//! One thread per sidebar. Its process starts on the first send and ends with **New**, or
//! when a recent thread opens from its menu. Every message is one request of the session, so
//! the agent's file writes for it are one undo step. The thread is saved on the machine as it
//! goes (`crate::store`), and the sidebar opens on the project's last one. The menu of **New**
//! also opens the composer's instructions to the agent in place of the thread
//! (`super::instructions`).
//!
//! This file holds the view itself: its fields, the menus and the settings, the instructions,
//! and the render. The rest is in `sidebar/`, as more `impl Sidebar`:
//! - `turn`: the agent's process, a turn from the send to its end, approvals, and the problems
//!   a turn left.
//! - `setup`: finding the program, its download, the sign-in and the sign-out.
//! - `threads`: the saved threads: the project's last one, **New**, **Open recent**, and
//!   writing them in order.

mod setup;
mod threads;
mod turn;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use gpui::{
    AnyElement, App, BoxShadow, Context, Entity, FocusHandle, Focusable, FollowMode, FontWeight,
    Global, KeyBinding, ListAlignment, ListState, Subscription, Task, Window, actions, div, hsla,
    list, point, prelude::*, px,
};
use sound_core::{INSTRUCTIONS_FILE, Problem, ProjectEvent};
use sound_ui::ActiveTheme;
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::dropdown_menu::{DropdownMenu, MenuPicked, Trigger};
use sound_ui::components::popover::{Align, Side};
use sound_ui::components::split_button::{SplitButton, SplitChoices};
use sound_ui::components::text_input::{Arrow, TextInput};

use super::entry;
use super::history::History;
use super::instructions::{self, InstructionsEditor, InstructionsEvent, Save, Scope};
use super::markdown::{self, Markdown};
use super::menu::{self, Choice};
use super::onboarding::{Onboarding, Setup, SetupAction};
use crate::conversation::{Conversation, Entry};
use crate::settings::{AgentSettings, AgentSettingsEvent};
use crate::store::{RecentThread, SavedThread, ThreadStore, Write};
use crate::{Account, ApprovalAnswer, Installed, Provider, SignInChoice, Thread};

actions!(agent_sidebar, [Stop]);

const KEY_CONTEXT: &str = "AgentSidebar";

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
    cx.bind_keys([
        KeyBinding::new("secondary-.", Stop, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-c", markdown::Copy, Some(markdown::KEY_CONTEXT)),
    ]);
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
    provider: Provider,
    /// Where downloaded programs are kept, `agents/` in the support folder. `None` when the
    /// machine has no support folder; a program named in the environment still runs.
    agents: Option<PathBuf>,
    setup: Setup,
    /// The program and its environment, once the login shell answered. The program may not be
    /// downloaded yet.
    installed: Option<Installed>,
    /// The way in of the running sign-in, which **Open the page again** starts again.
    signing_in: Option<SignInChoice>,
    /// The download, the sign-in or a question to the program. Dropping it cancels it.
    setup_task: Option<Task<()>>,
    /// **New**, with the recent threads and the instructions in its menu.
    new_button: Entity<SplitButton>,
    /// The model, and the account with **Sign out**, in the composer.
    model_menu: Entity<DropdownMenu>,
    /// The approval mode, beside the model.
    access_menu: Entity<DropdownMenu>,
    /// What the menus' selects say, the same for every sidebar of the app.
    settings: Entity<AgentSettings>,
    conversation: Conversation,
    list: ListState,
    /// The finished turns whose steps show, by entry.
    expanded: HashSet<usize>,
    /// The answer of each turn, parsed once per batch of events, by entry.
    answers: HashMap<usize, Vec<Markdown>>,
    /// The problems of the project when the last turn began.
    problems_before: Vec<Problem>,
    /// The turn that ended last and when, until the next message: the watcher may still
    /// apply its last writes, and their problems are the turn's too.
    just_ended: Option<(usize, Instant)>,
    /// The problems each finished turn left that were not there before it, by entry.
    problems_left: HashMap<usize, Vec<Problem>>,
    /// The turns whose problems show line by line, by entry.
    problems_open: HashSet<usize>,
    input: Entity<TextInput>,
    /// Where up and down in the composer are among the earlier messages.
    history: History,
    /// The sidebar itself, for when it has no composer.
    focus_handle: FocusHandle,
    send_focus: FocusHandle,
    /// Allow, Allow for this thread, Deny.
    approval_focus: [FocusHandle; 3],
    agent: Option<Agent>,
    /// The thread shown, as it is saved. `None` until its first message.
    saved: Option<SavedThread>,
    /// Hands what to keep of the thread to its writer, in order. `None` while nothing is
    /// saved, as in a snapshot.
    writes: Option<smol::channel::Sender<Write>>,
    /// Reads the last thread of the project. No message goes until it is in.
    loading: Option<Task<()>>,
    /// **New** was clicked while the last thread was read: the new thread wins over it.
    new_thread_while_loading: bool,
    /// The threads of the project, once it is read. `None` while nothing is saved.
    store: Option<ThreadStore>,
    /// The project's other threads, the last one shown first.
    recent: Vec<RecentThread>,
    /// Reads a recent thread the composer opened. No message goes until it is in.
    opening: Option<Task<()>>,
    /// The instructions shown in place of the thread.
    instructions: Option<(Entity<InstructionsEditor>, Subscription)>,
    /// Hands the instructions to their writer, in the order they were typed.
    saves: smol::channel::Sender<Save>,
    _subscriptions: Vec<Subscription>,
}

impl Sidebar {
    /// Finds the program in the background, see [`Self::find_program`]. The threads of the
    /// project are kept in `threads`, `agent/threads` in the support folder of the machine;
    /// with `None` nothing is saved. `settings` are the app's, shared by every sidebar.
    pub fn new(
        session: Entity<sound_ui::Session>,
        agents: Option<PathBuf>,
        threads: Option<PathBuf>,
        settings: Entity<AgentSettings>,
        first_on_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut sidebar = Self::with(
            session,
            agents,
            Setup::Checking,
            threads,
            settings,
            window,
            cx,
        );
        sidebar.find_program(first_on_path, cx);
        sidebar
    }

    /// For a test or a snapshot: set up and signed in with `installed` as the program, or not
    /// installed with `None`. Nothing runs in the background.
    pub fn with_program(
        session: Entity<sound_ui::Session>,
        installed: Option<Installed>,
        threads: Option<PathBuf>,
        settings: Entity<AgentSettings>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let setup = match installed {
            Some(_) => Setup::Ready {
                account: Account::default(),
            },
            None => Setup::NotInstalled,
        };
        let mut sidebar = Self::with(session, None, setup, threads, settings, window, cx);
        sidebar.installed = installed;
        sidebar
    }

    fn with(
        session: Entity<sound_ui::Session>,
        agents: Option<PathBuf>,
        setup: Setup,
        threads: Option<PathBuf>,
        settings: Entity<AgentSettings>,
        window: &mut Window,
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
            // The handlers run inside the input's own update, and sending or recalling a
            // message changes it.
            input.set_on_submit({
                let weak = weak.clone();
                move |_, _, cx| {
                    let weak = weak.clone();
                    cx.defer(move |cx| {
                        // A sidebar that went in the meantime sends nothing.
                        weak.update(cx, |sidebar, cx| sidebar.send(cx)).ok();
                    });
                }
            });
            input.set_on_arrow_past_edge(move |_, arrow, _, cx| {
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |sidebar, cx| sidebar.recall(arrow, cx))
                        .ok();
                });
            });
        });
        let list = ListState::new(0, ListAlignment::Top, px(OVERDRAW));
        list.set_follow_mode(FollowMode::Tail);
        // Filled by `update_menu` below, as on every change.
        let new_menu = |name: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                DropdownMenu::new("", Vec::new(), cx)
                    .debug_name(name)
                    .trigger(Trigger::Ghost)
                    .side(Side::Top)
                    .align(Align::Start)
                    .width(menu::WIDTH)
                    .max_height(menu::MAX_HEIGHT)
            })
        };
        let model_menu = new_menu("model-menu", cx);
        let access_menu = new_menu("access-menu", cx);
        let new_button = cx.new(|cx| {
            let choices = SplitChoices {
                label: "New".into(),
                main_value: Choice::NewThread.value(),
                menu_label: "Recent threads and instructions".into(),
                // Filled by `update_new_menu` below, as on every change.
                entries: Vec::new(),
            };
            SplitButton::new("agent-new-thread", choices, cx)
                .icon("plus")
                .align_end(cx)
        });
        let subscriptions = vec![
            cx.subscribe_in(&model_menu, window, |sidebar, _, picked, window, cx| {
                sidebar.picked(picked, window, cx);
            }),
            cx.subscribe_in(&access_menu, window, |sidebar, _, picked, window, cx| {
                sidebar.picked(picked, window, cx);
            }),
            cx.subscribe_in(&new_button, window, |sidebar, _, picked, window, cx| {
                sidebar.picked(picked, window, cx);
            }),
            cx.observe(&settings, |sidebar, _, cx| {
                sidebar.update_menu(cx);
                cx.notify();
            }),
            cx.subscribe(&settings, Self::settings_event),
            cx.subscribe(&session, |sidebar, _, event: &ProjectEvent, cx| {
                if matches!(event, ProjectEvent::ProblemsChanged) {
                    sidebar.problems_changed(cx);
                }
            }),
            cx.observe(&input, |_, _, cx| cx.notify()),
        ];
        let mut conversation = Conversation::default();
        // Read before this sidebar was made: it says so too.
        if let Some(unreadable) = settings.read(cx).unreadable() {
            conversation.notice(unreadable);
        }
        let mut sidebar = Self {
            session,
            provider: Provider::Claude,
            agents,
            setup,
            installed: None,
            signing_in: None,
            setup_task: None,
            new_button,
            model_menu,
            access_menu,
            settings,
            conversation,
            list,
            expanded: HashSet::new(),
            answers: HashMap::new(),
            problems_before: Vec::new(),
            just_ended: None,
            problems_left: HashMap::new(),
            problems_open: HashSet::new(),
            input,
            history: History::default(),
            focus_handle: cx.focus_handle(),
            send_focus: cx.focus_handle().tab_stop(true),
            approval_focus: [(); 3].map(|_| cx.focus_handle().tab_stop(true)),
            agent: None,
            saved: None,
            writes: None,
            loading,
            new_thread_while_loading: false,
            store: None,
            recent: Vec::new(),
            opening: None,
            instructions: None,
            saves: save_instructions(cx),
            _subscriptions: subscriptions,
        };
        sidebar.update_menu(cx);
        sidebar.update_new_menu(cx);
        sidebar
    }

    /// For a conversation that is another one now: what the sidebar keeps by entry is not
    /// its.
    fn forget_entries(&mut self) {
        self.expanded.clear();
        self.answers.clear();
        self.problems_left.clear();
        self.problems_open.clear();
        self.just_ended = None;
        self.history.reset();
    }

    /// A change of the settings applies at once: the running agent hears it now, from its
    /// next tool on. A request that fails comes back as an error line.
    fn settings_event(
        &mut self,
        _: Entity<AgentSettings>,
        event: &AgentSettingsEvent,
        cx: &mut Context<Self>,
    ) {
        let sent = match event {
            AgentSettingsEvent::Read => {
                if let Some(unreadable) = self.settings.read(cx).unreadable() {
                    let notice = unreadable.to_string();
                    self.conversation.notice(notice);
                    self.show(None, cx);
                }
                return;
            }
            AgentSettingsEvent::NotSaved(error) => {
                self.conversation.notice(error.clone());
                self.show(None, cx);
                return;
            }
            AgentSettingsEvent::ApprovalModeChanged(mode) => self
                .agent
                .as_ref()
                .map(|agent| agent.thread.set_approval_mode(*mode)),
            AgentSettingsEvent::ModelChanged(model) => self
                .agent
                .as_ref()
                .map(|agent| agent.thread.set_model(model.clone())),
        };
        if let Some(Err(error)) = sent {
            self.conversation.notice(error.to_string());
            self.show(None, cx);
        }
    }

    fn picked(&mut self, picked: &MenuPicked, window: &mut Window, cx: &mut Context<Self>) {
        let Some(choice) = Choice::of(&picked.0) else {
            return;
        };
        match choice {
            Choice::ApprovalMode(mode) => self
                .settings
                .update(cx, |settings, cx| settings.set_approval_mode(mode, cx)),
            Choice::Model(model) => self
                .settings
                .update(cx, |settings, cx| settings.set_model(model, cx)),
            Choice::SignOut => self.sign_out(cx),
            Choice::NewThread => self.new_thread(window, cx),
            Choice::OpenThread(id) => self.open_thread(id, window, cx),
            Choice::Instructions(scope) => self.open_instructions(scope, window, cx),
        }
    }

    fn update_new_menu(&mut self, cx: &mut Context<Self>) {
        let all_projects = self.settings.read(cx).instructions_file().is_some();
        let entries = menu::new_entries(&self.recent, all_projects);
        let menu = self.new_button.read(cx).menu().clone();
        menu.update(cx, |menu, cx| menu.set_entries(entries, cx));
    }

    fn update_menu(&mut self, cx: &mut Context<Self>) {
        let account = match &self.setup {
            Setup::Ready { account } => account.clone(),
            _ => Account::default(),
        };
        let shared = self.settings.read(cx);
        let (settings, models) = (shared.settings(), shared.models());
        let label = menu::label(settings, models);
        let entries = menu::model_entries(self.provider, &account, settings, models);
        let (access_icon, access_label) = menu::access_label(settings);
        let access_entries = menu::access_entries(settings);
        let (logo, brand) = menu::logo(self.provider);
        self.model_menu.update(cx, |menu, cx| {
            menu.set_icon(logo, Some(brand), cx);
            menu.set_label(label, cx);
            menu.set_entries(entries, cx);
        });
        self.access_menu.update(cx, |menu, cx| {
            menu.set_icon(access_icon, None, cx);
            menu.set_label(access_label, cx);
            menu.set_entries(access_entries, cx);
        });
    }

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// The composer, for a test or a snapshot that reads or types in it.
    pub fn composer(&self) -> &Entity<TextInput> {
        &self.input
    }

    /// The composer's model menu, with the account, for a snapshot that shows it open.
    pub fn model_menu(&self) -> &Entity<DropdownMenu> {
        &self.model_menu
    }

    /// The composer's menu of approval modes, for a snapshot that shows it open or picks in it.
    pub fn access_menu(&self) -> &Entity<DropdownMenu> {
        &self.access_menu
    }

    /// The menu of **New**, for a snapshot that shows it open.
    pub fn new_menu(&self, cx: &App) -> Entity<DropdownMenu> {
        self.new_button.read(cx).menu().clone()
    }

    /// The session the next process of the thread resumes: the thread's own once its agent
    /// has started, so a thread opened again goes on where it left off. `None` starts a new
    /// one.
    pub fn resume(&self) -> Option<String> {
        self.saved.as_ref()?.session_id.clone()
    }

    /// Whether the agent works or waits on the composer, so a closed sidebar can say so.
    pub fn is_busy(&self) -> bool {
        self.conversation.is_working()
    }

    /// Tells the list what changed: entries added at the end, and `changed` and what comes
    /// after it measured again, for an entry that grew.
    fn show(&mut self, changed: Option<usize>, cx: &mut Context<Self>) {
        let shown = self.list.item_count();
        if let Some(changed) = changed {
            // Parsed again when it next shows.
            self.answers.retain(|index, _| *index < changed);
        }
        if let Some(index) = changed.filter(|index| *index < shown) {
            self.list.remeasure_items(index..shown);
        }
        let count = self.conversation.entries().len();
        if count > shown {
            self.list.splice(shown..shown, count - shown);
        }
        cx.notify();
    }

    /// Shows the instructions in `scope` in place of the thread.
    fn open_instructions(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let file = match scope {
            Scope::Project => Some(
                self.session
                    .read(cx)
                    .project()
                    .root()
                    .join(INSTRUCTIONS_FILE),
            ),
            Scope::AllProjects => self.settings.read(cx).instructions_file(),
        };
        let Some(file) = file else {
            return;
        };
        self.close_instructions(window, cx);
        let saves = self.saves.clone();
        let editor = cx.new(|cx| InstructionsEditor::new(scope, file, saves, window, cx));
        let subscription = cx.subscribe_in(&editor, window, |sidebar, _, event, window, cx| {
            match event {
                InstructionsEvent::Close => {}
                InstructionsEvent::Unreadable(message) => {
                    sidebar.conversation.notice(message.clone());
                    sidebar.show(None, cx);
                }
            }
            sidebar.close_instructions(window, cx);
        });
        self.instructions = Some((editor, subscription));
        cx.notify();
    }

    /// Saves what the instructions hold and shows the thread again.
    fn close_instructions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, _)) = self.instructions.take() else {
            return;
        };
        editor.update(cx, |editor, cx| editor.close(cx));
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }

    /// Opens or closes the turn at `index` in `set`: its steps or its problems.
    fn toggle(
        &mut self,
        set: fn(&mut Self) -> &mut HashSet<usize>,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        let set = set(self);
        if !set.remove(&index) {
            set.insert(index);
        }
        self.list.remeasure_items(index..index + 1);
        cx.notify();
    }

    /// Opens the steps and the problems of the turn at `index`, as clicks on their lines do.
    /// For a snapshot.
    pub fn open_details(&mut self, index: usize, cx: &mut Context<Self>) {
        self.expanded.insert(index);
        self.problems_open.insert(index);
        self.list.remeasure_items(index..index + 1);
        cx.notify();
    }

    /// Up on the first row of the composer, or down on its last.
    fn recall(&mut self, arrow: Arrow, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().to_string();
        let messages: Vec<&str> = self.conversation.messages().collect();
        if let Some(recalled) = self.history.recall(&messages, &text, arrow) {
            self.input
                .update(cx, |input, cx| input.set_text(recalled, cx));
        }
    }

    fn render_entry(&mut self, index: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let content = match self.conversation.entries().get(index) {
            Some(Entry::Message(text)) => entry::message(text, cx),
            Some(Entry::Notice(text)) => entry::notice(text, cx),
            Some(Entry::Turn(turn)) => {
                let below = match (turn.approval(), self.problems_left.get(&index)) {
                    (Some(approval), _) => Some(self.approval_row(&approval.title, cx)),
                    (None, Some(problems)) => {
                        let open = self.problems_open.contains(&index);
                        let toggle = cx.listener(move |sidebar, _, _, cx| {
                            sidebar.toggle(|sidebar| &mut sidebar.problems_open, index, cx);
                        });
                        Some(entry::problems(problems, index, open, toggle, cx))
                    }
                    (None, None) => None,
                };
                let toggle = cx.listener(move |sidebar, _, _, cx| {
                    sidebar.toggle(|sidebar| &mut sidebar.expanded, index, cx);
                });
                let steps_open = self.expanded.contains(&index);
                let answer = self
                    .answers
                    .entry(index)
                    .or_insert_with(|| entry::answer(turn));
                entry::turn(turn, index, answer, steps_open, toggle, below, cx)
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
            .child(entry::title("approval", title))
            .child(div().flex().flex_wrap().gap(px(8.)).children(buttons))
            .into_any_element()
    }

    /// The title, and **New** at the right. Over the instructions: their title, and the way
    /// back at the left.
    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.instructions.is_some();
        let header = div()
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
                    .child(if editing { "Instructions" } else { "Agent" }),
            );
        if !editing {
            return header.child(
                div()
                    .absolute()
                    .right(px(16.))
                    .child(self.new_button.clone()),
            );
        }
        header.child(
            div().absolute().left(px(16.)).child(
                Button::icon_only("instructions-back", "arrow-left")
                    .debug_selector(|| "agent-instructions-back".to_string())
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Xs)
                    .on_click(cx.listener(|sidebar, _, window, cx| {
                        sidebar.close_instructions(window, cx);
                    })),
            ),
        )
    }

    fn composer_box(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, fill, divider) = (theme.alpha_at(0.10), theme.gray_200, theme.gray_400);
        let (blue, stop) = (theme.blue, theme.gray_950);
        let working = self.conversation.is_working();
        let can_send = !self.input.read(cx).text().trim().is_empty();
        // While a turn runs the send button stops it: one turn at a time.
        let button = if working {
            Button::icon_only("stop", "square")
                .debug_selector(|| "agent-stop".to_string())
                .variant(ButtonVariant::Solid(stop))
                .on_click(cx.listener(|sidebar, _, _, cx| sidebar.stop(cx)))
        } else if can_send {
            Button::icon_only("send", "arrow-up")
                .variant(ButtonVariant::Solid(blue))
                .on_click(cx.listener(|sidebar, _, _, cx| sidebar.send(cx)))
        } else {
            Button::icon_only("send", "arrow-up")
                .variant(ButtonVariant::Subtle)
                .disabled(true)
        };
        // The access pill always says the mode, so it is never a surprise.
        div().flex_none().px(px(12.)).pb(px(12.)).pt(px(8.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .pt(px(14.))
                .px(px(14.))
                .pb(px(10.))
                .rounded(px(20.))
                .border_1()
                .border_color(border)
                .bg(fill)
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.4),
                    offset: point(px(0.), px(8.)),
                    blur_radius: px(24.),
                    spread_radius: px(-8.),
                    inset: false,
                }])
                .child(div().min_h(px(44.)).child(self.input.clone()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        // The pills sit their wash, not their words, on the box's edge.
                        .ml(px(-6.))
                        .child(self.model_menu.clone())
                        .child(div().flex_none().w(px(1.)).h(px(16.)).bg(divider))
                        .child(self.access_menu.clone())
                        .child(div().flex_1())
                        .child(
                            button
                                .size(ButtonSize::Md)
                                .rounded(true)
                                .focus_handle(&self.send_focus),
                        ),
                ),
        )
    }

    /// In place of the composer once the agent lost the session: the thread stays to read,
    /// and **+** is the way on, as **New** is.
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

/// Writes the instructions in turn, so the last text typed is the one on disk. Detached: it
/// ends when the sidebar does, after the writes still queued. A failed one shows in the thread.
fn save_instructions(cx: &mut Context<Sidebar>) -> smol::channel::Sender<Save> {
    let (sender, receiver) = smol::channel::unbounded::<Save>();
    cx.spawn(async move |sidebar, cx| {
        while let Ok((file, text)) = receiver.recv().await {
            let written = cx
                .background_spawn(async move { instructions::write(&file, &text) })
                .await;
            if let Err(error) = written {
                // A sidebar that went has nobody to tell.
                sidebar
                    .update(cx, |sidebar, cx| {
                        sidebar.conversation.notice(error);
                        sidebar.show(None, cx);
                    })
                    .ok();
            }
        }
    })
    .detach();
    sender
}

impl Focusable for Sidebar {
    /// The composer, which cmd-L focuses, or the sidebar itself while it has none, so cmd-L
    /// and escape still work there.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if let Some((editor, _)) = &self.instructions {
            return editor.focus_handle(cx);
        }
        match self.setup {
            Setup::Ready { .. } if self.conversation.can_continue() => self.input.focus_handle(cx),
            _ => self.focus_handle.clone(),
        }
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        // The hairline of the arrangement's lines, so the edges weigh the same.
        let (hairline, text) = (theme.alpha_at(0.05), theme.gray_950);
        let body = match (&self.instructions, &self.setup) {
            (Some((editor, _)), _) => editor.clone().into_any_element(),
            (
                None,
                setup @ (Setup::Checking
                | Setup::NotInstalled
                | Setup::Downloading { .. }
                | Setup::DownloadFailed { .. }
                | Setup::SignedOut { .. }
                | Setup::SigningIn
                | Setup::Stopped { .. }),
            ) => Onboarding::new(self.provider, setup.clone())
                .on_action(
                    cx.listener(|sidebar, action: &SetupAction, _, cx| sidebar.act(*action, cx)),
                )
                .into_any_element(),
            (None, Setup::Ready { .. }) => div()
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
                        body.child(self.composer_box(cx))
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
            // On the background of the window; the top edge parts it from the title row.
            .border_t_1()
            .border_r_1()
            .border_color(hairline)
            .text_color(text)
            .child(self.header(cx))
            .child(body)
    }
}
