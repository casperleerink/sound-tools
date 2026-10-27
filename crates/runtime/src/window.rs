//! The application window: the title row with the project menu at its left and what the main
//! view puts in the title bar in its middle, the view of the project's main instance under it, the left panel beside it, and a
//! quiet line for errors and problems top-right, under the title row.
//!
//! The window is the project runtime. It names no extension type: the main area shows whatever
//! view the installed [`Views`] has for the main instance of the project. Nor does it name the
//! agent: the left panel is whatever
//! the installed [`LeftPanelSlot`] makes. Its own keys are the app's: undo, the panel, opening
//! and quitting. Musical keys, such as space to play, are the project's, through
//! [`GlobalKeys`].

pub mod audio_input;
pub mod live_input;
pub mod new_project;
pub mod other_apps;
mod project_menu;
pub mod recorders;
mod start;

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{
    AnyView, App, Bounds, Context, Entity, FocusHandle, Focusable, Global, KeyBinding,
    KeyBindingContextPredicate, KeyDownEvent, KeyUpEvent, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, SharedString, Subscription, Task, TitlebarOptions, WeakFocusHandle, Window,
    WindowBounds, WindowControlArea, WindowOptions, actions, div, point, prelude::*, px, size,
};
use plugin_host::{Plugins, WeakPlugins};
use sound_core::{
    Engine, EngineConfig, InstanceId, OutputDevice, OutputStream, Project, ProjectEvent,
    StreamTiming,
};
use sound_typescript::{Extensions, FirstLoad};
use sound_ui::components::button::{Button, ButtonSize, ButtonVariant};
use sound_ui::components::empty_state::EmptyState;
use sound_ui::components::indicator::{Indicator, IndicatorSize};
use sound_ui::components::notice::{Notice, NoticeTone};
use sound_ui::components::text_input::{self, TextInput};
use sound_ui::{
    ActiveTheme, Assets, Devices, GlobalKeys, Session, SpareKey, SpareKeys, Views, typography,
};

use audio_input::OpenInput;
use live_input::LiveInput;
use other_apps::{AppSounds, OtherApps};
pub use project_menu::{DeviceMenu, ListDevices};
use project_menu::{ProjectMenu, choose_new_project, open_another_project, start_again};
use recorders::Recorders;

use crate::app::ChosenDevices;
use crate::{TEMPLATES, Template, open_with_loaded, update, views};

actions!(
    sound_tools,
    [
        ToggleComputerKeys,
        Undo,
        Redo,
        FocusNext,
        FocusPrevious,
        ToggleLeftPanel,
        OpenCloseLeftPanel,
        NewProject,
        OpenProject,
        Quit
    ]
);

/// Room left of the panel icon: the traffic lights on macOS, where the other systems draw
/// nothing.
const TITLE_ROW_INSET: f32 = if cfg!(target_os = "macos") { 80. } else { 8. };
const TOP_ROW_HEIGHT: f32 = 48.;
/// The notices sit this far in from the right of the window and below the title row.
const NOTICE_INSET: f32 = 24.;
/// The widest a notice gets. A longer message wraps, to three lines at most.
const NOTICE_WIDTH: f32 = 400.;
/// The window of the design: a 13 to 14 inch MacBook, less its menu bar.
const WINDOW_WIDTH: f32 = 1470.;
const WINDOW_HEIGHT: f32 = 920.;
/// The smallest window: the project menu beside a tool's page with room for a card.
const MIN_WINDOW_WIDTH: f32 = 1100.;
const MIN_WINDOW_HEIGHT: f32 = 640.;
/// The left panel has one width, so the main area keeps the width it was designed for.
const LEFT_PANEL_WIDTH: f32 = 400.;

/// What the window shows in its left panel, given by the composition root, which knows the
/// agent: this module names no type of it. With none installed the window has no panel and
/// no icon for it.
#[derive(Clone)]
pub struct LeftPanelSlot {
    make: Rc<dyn Fn(Entity<Session>, &mut Window, &mut Context<Shell>) -> LeftPanel>,
    /// The file that remembers whether the panel is open, in the support folder of the
    /// machine and never in a project, where an agent could change it. `None` remembers
    /// nothing.
    remembered: Option<PathBuf>,
}

impl Global for LeftPanelSlot {}

impl LeftPanelSlot {
    pub fn new(
        remembered: Option<PathBuf>,
        make: impl Fn(Entity<Session>, &mut Window, &mut Context<Shell>) -> LeftPanel + 'static,
    ) -> Self {
        Self {
            make: Rc::new(make),
            remembered,
        }
    }

    pub fn install(self, cx: &mut App) {
        cx.set_global(self);
    }
}

/// The view in the left panel, and what the window needs to know of it.
pub struct LeftPanel {
    view: AnyView,
    /// What cmd-L focuses, such as the composer of the agent. Asked each time: what the view
    /// can focus changes, as when the composer appears.
    focus: Box<dyn Fn(&App) -> FocusHandle>,
    /// Whether it works or waits on the composer. The icon in the title row says so while the
    /// panel is closed, so a closed panel never hides a question.
    busy: bool,
    /// Where the window can write words for the agent, such as the merge of a template.
    composer: Option<Box<dyn Fn(&App) -> Entity<TextInput>>>,
    /// Sends the agent a message as if typed, and says whether it went.
    ask: Option<Asks>,
    _observing: Subscription,
}

type Asks = Box<dyn Fn(&str, &mut Window, &mut App) -> bool>;

impl LeftPanel {
    pub fn with_composer(mut self, composer: impl Fn(&App) -> Entity<TextInput> + 'static) -> Self {
        self.composer = Some(Box::new(composer));
        self
    }

    pub fn with_ask(mut self, ask: impl Fn(&str, &mut Window, &mut App) -> bool + 'static) -> Self {
        self.ask = Some(Box::new(ask));
        self
    }

    pub fn new<V: Render + Focusable>(
        view: Entity<V>,
        busy: fn(&V) -> bool,
        cx: &mut Context<Shell>,
    ) -> Self {
        let observing = cx.observe(&view, move |shell, view, cx| {
            let now = busy(view.read(cx));
            if let Some(panel) = &mut shell.left_panel
                && panel.busy != now
            {
                panel.busy = now;
                cx.notify();
            }
        });
        Self {
            busy: busy(view.read(cx)),
            focus: {
                let view = view.clone();
                Box::new(move |cx| view.read(cx).focus_handle(cx))
            },
            view: view.into(),
            composer: None,
            ask: None,
            _observing: observing,
        }
    }
}

/// What the window of the real runtime has of the device: its timing, for the latency and for
/// where a take lands, how it opens the audio input, how it hears other apps, and how it lists
/// the devices to pick from. A test window has none of it, or simulated ones.
#[derive(Default)]
pub struct DeviceAccess {
    pub timing: Option<Arc<StreamTiming>>,
    pub open_input: Option<OpenInput>,
    pub app_sounds: Option<AppSounds>,
    pub list_devices: Option<ListDevices>,
}

/// The root view of the window.
pub struct Shell {
    session: Entity<Session>,
    /// The instance in the main area and its view. Both change when it comes or goes.
    main: Option<(InstanceId, AnyView)>,
    /// What the main view shows in the title bar, such as the transport of a song.
    title_bar: Option<Entity<sound_typescript::TitleBar>>,
    project_menu: Entity<ProjectMenu>,
    /// The machine's live input and the meter of the device, see [`LiveInput`].
    live: Entity<LiveInput>,
    focus_handle: FocusHandle,
    dismiss_focus: FocusHandle,
    fix_focus: FocusHandle,
    left_panel: Option<LeftPanel>,
    left_panel_open: bool,
    /// See [`LeftPanelSlot::remembered`].
    left_panel_file: Option<PathBuf>,
    /// The whole panel, to know whether the focus is in it.
    left_panel_area: FocusHandle,
    left_panel_icon_focus: FocusHandle,
    /// Where the focus was before the panel took it, where escape gives it back.
    return_focus: Option<WeakFocusHandle>,
    /// The last write of whether the panel is open. Each waits for the one before, so the
    /// file ends with the last.
    remembering: Task<()>,
    /// The taps of the other apps `project.json` hears, in a window that can open them.
    _other_apps: Option<Entity<OtherApps>>,
    recorders: Entity<Recorders>,
    /// A newer template's code for files the project changed, until the composer merges or
    /// dismisses it.
    template_update: Option<crate::templates::Update>,
}

impl Shell {
    /// Installs the view and device registries of the application, so that no caller can
    /// forget them.
    pub fn new(
        session: Entity<Session>,
        registries: (Views, Devices),
        device_name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let device = DeviceAccess::default();
        Self::with_device(session, registries, device_name, device, window, cx)
    }

    /// The window of the real runtime, which has a device, see [`DeviceAccess`]. A window
    /// without it measures no latency, cannot record audio and hears no other app; a test gives
    /// simulated ones.
    pub fn with_device(
        session: Entity<Session>,
        registries: (Views, Devices),
        device_name: SharedString,
        device: DeviceAccess,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let DeviceAccess {
            timing,
            open_input,
            app_sounds,
            list_devices,
        } = device;
        let (views, devices) = registries;
        views.install(cx);
        devices.install(cx);
        // The click is a processor in the engine, not project state: attaching it writes
        // nothing and adds no undo step. It starts off and silent.
        session.update(cx, |session, cx| {
            match metronome::Click::attach(session.engine()) {
                Ok(mut click) => {
                    session.attach_click(Box::new(move |engine, on| click.set_on(engine, on)))
                }
                Err(error) => session.report(error, cx),
            }
        });
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&session, window, |shell, _, event, window, cx| {
            let at_top = |id: &InstanceId| id.parent().is_none();
            // `main` in `project.json` may name another one.
            if matches!(event, ProjectEvent::Created(id) | ProjectEvent::Deleted(id) if at_top(id))
                || matches!(event, ProjectEvent::ProjectFileChanged)
            {
                shell.show_main_instance(window, cx);
            }
        })
        .detach();

        let focus_handle = cx.focus_handle();
        // The keys of the window work from its key context, so the focus must stay inside it.
        // A control that goes away while it has the focus, such as a dismissed notice, would
        // leave it nowhere.
        cx.on_focus_lost(window, |shell, window, cx| {
            window.focus(&shell.focus_handle, cx)
        })
        .detach();
        // A key the computer keys hold comes up in the app that has the keys now.
        cx.observe_window_activation(window, |shell, window, cx| {
            if !window.is_window_active() {
                shell.live.update(cx, LiveInput::let_go_of_computer_keys);
                GlobalKeys::let_go(cx);
            }
        })
        .detach();
        let slot = cx.try_global::<LeftPanelSlot>().cloned();
        let left_panel = slot
            .as_ref()
            .map(|slot| (slot.make)(session.clone(), window, cx));
        let left_panel_file = slot.and_then(|slot| slot.remembered);
        // One word read once before the first frame, as the last project is read before the
        // window: drawing it closed and then opening it would flash.
        let left_panel_open = left_panel_file
            .as_deref()
            .is_none_or(crate::app::left_panel_was_open);
        let project_menu =
            cx.new(|cx| ProjectMenu::new(session.clone(), device_name, list_devices, window, cx));
        // For the notice of an update.
        cx.observe_global::<update::Ready>(|_, cx| cx.notify())
            .detach();
        let other_apps = app_sounds.map(|sounds| {
            let session = session.clone();
            cx.new(|cx| OtherApps::new(session, sounds, cx))
        });
        let live = cx.new(|cx| LiveInput::new(session.clone(), timing, open_input, cx));
        let recorders = cx.new(|cx| Recorders::new(session.clone(), live.clone(), cx));
        let mut shell = Self {
            project_menu,
            live,
            session,
            main: None,
            title_bar: None,
            focus_handle,
            dismiss_focus: cx.focus_handle().tab_stop(true),
            fix_focus: cx.focus_handle().tab_stop(true),
            left_panel,
            left_panel_open,
            left_panel_file,
            left_panel_area: cx.focus_handle(),
            left_panel_icon_focus: cx.focus_handle().tab_stop(true),
            return_focus: None,
            remembering: Task::ready(()),
            _other_apps: other_apps,
            recorders,
            template_update: None,
        };
        // Asked again after the open brought the rest up to date, which changes nothing more.
        let root = shell.session.read(cx).project().root().to_path_buf();
        match crate::templates::update(&root) {
            Ok(update) => shell.template_update = update,
            Err(error) => shell
                .session
                .update(cx, |session, cx| session.report(error, cx)),
        }
        // A plugin's window passes on the keys its plugin does not use, to play the computer
        // keys. The plain keys are always free there: it has no text field and no tool.
        let live = shell.live.downgrade();
        let pass = move |key: SpareKey<'_>, cx: &mut App| {
            // Fails only when the window is gone.
            (live.update(cx, |live, cx| match key {
                SpareKey::Down(event) => {
                    live.computer_key_down(event, true);
                }
                SpareKey::Up(event) => {
                    live.computer_key_up(&event.keystroke.key);
                }
                SpareKey::LetGo => live.let_go_of_computer_keys(cx),
            }))
            .ok();
        };
        SpareKeys::install(pass, cx);
        shell.show_main_instance(window, cx);
        // After the main view, which may take the focus: a page that hears keys does.
        if window.focused(cx).is_none() {
            window.focus(&shell.focus_handle, cx);
        }
        shell
    }

    pub fn main_view(&self) -> Option<&AnyView> {
        self.main.as_ref().map(|(_, view)| view)
    }

    pub fn project_menu(&self) -> &Entity<ProjectMenu> {
        &self.project_menu
    }

    /// The machine's live input: the keyboard, the computer keys, the audio input and the meter.
    pub fn live_input(&self) -> &Entity<LiveInput> {
        &self.live
    }

    /// The takes of the recorders of the project.
    pub fn recorders(&self) -> &Entity<Recorders> {
        &self.recorders
    }

    /// The view in the left panel, open or not.
    pub fn left_panel(&self) -> Option<&AnyView> {
        self.left_panel.as_ref().map(|panel| &panel.view)
    }

    pub fn left_panel_open(&self) -> bool {
        self.left_panel.is_some() && self.left_panel_open
    }

    /// cmd-L: opens the panel and focuses it, focuses it when it is open and the focus is
    /// elsewhere, and closes it when the focus is in it.
    fn toggle_left_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.left_panel_open && self.left_panel_area.contains_focused(window, cx) {
            self.set_left_panel_open(false, window, cx);
            self.give_focus_back(window, cx);
        } else {
            self.focus_left_panel(window, cx);
        }
    }

    /// Opens the panel when it is closed, and focuses it.
    fn focus_left_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = &self.left_panel else {
            return;
        };
        let focus = (panel.focus)(cx);
        self.remember_focus(window, cx);
        if !self.left_panel_open {
            self.set_left_panel_open(true, window, cx);
        }
        window.focus(&focus, cx);
    }

    /// The icon in the title row and cmd-B open and close the panel, and leave the focus where
    /// it is.
    fn open_or_close_left_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = !self.left_panel_open;
        if !open && self.left_panel_area.contains_focused(window, cx) {
            self.give_focus_back(window, cx);
        }
        self.set_left_panel_open(open, window, cx);
    }

    fn set_left_panel_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.left_panel_open = open;
        if open {
            widen_for_left_panel(window, cx);
        }
        if let Some(file) = self.left_panel_file.clone() {
            let previous = std::mem::replace(&mut self.remembering, Task::ready(()));
            let session = self.session.downgrade();
            self.remembering = cx.spawn(async move |_, cx| {
                previous.await;
                let written = cx
                    .background_spawn(async move { crate::app::remember_left_panel(&file, open) })
                    .await;
                // A window that has gone has nobody left to tell.
                if let (Err(error), Some(session)) = (written, session.upgrade()) {
                    let message = format!("{error:#}");
                    session.update(cx, |session, cx| session.report(message, cx));
                }
            });
        }
        cx.notify();
    }

    /// Keeps what has the focus outside the panel, for [`Self::give_focus_back`].
    fn remember_focus(&mut self, window: &Window, cx: &App) {
        if let Some(focused) = window.focused(cx)
            && !self.left_panel_area.contains(&focused, window)
        {
            self.return_focus = Some(focused.downgrade());
        }
    }

    /// Escape in the panel, and closing it: the focus goes back to where it was before the
    /// panel took it, else to the window, whose keys then work again.
    fn give_focus_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = self
            .return_focus
            .take()
            .and_then(|focus| focus.upgrade())
            .unwrap_or_else(|| self.focus_handle.clone());
        window.focus(&back, cx);
    }

    /// The panel left of the main area, when it is open.
    fn left_panel_element(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let panel = self.left_panel.as_ref().filter(|_| self.left_panel_open)?;
        Some(
            div()
                .id("left-panel")
                .track_focus(&self.left_panel_area)
                // Before the press moves the focus into the panel.
                .capture_any_mouse_down(cx.listener(|shell, _: &MouseDownEvent, window, cx| {
                    shell.remember_focus(window, cx)
                }))
                .on_action(cx.listener(|shell, _: &text_input::Cancel, window, cx| {
                    shell.give_focus_back(window, cx)
                }))
                .flex_none()
                .w(px(LEFT_PANEL_WIDTH))
                .h_full()
                .child(panel.view.clone()),
        )
    }

    /// The icon right of the traffic lights that opens and closes the panel. While the panel
    /// is closed and busy, it carries the lavender indicator.
    fn left_panel_icon(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let panel = self.left_panel.as_ref()?;
        let lavender = cx.theme().lavender;
        let busy = panel.busy && !self.left_panel_open;
        Some(
            div()
                .relative()
                .flex_none()
                .child(
                    Button::icon_only("left-panel", "panel-left")
                        .debug_selector(|| "left-panel-icon".to_string())
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Sm)
                        .focus_handle(&self.left_panel_icon_focus)
                        .on_click(cx.listener(|shell, _, window, cx| {
                            shell.open_or_close_left_panel(window, cx)
                        })),
                )
                .when(busy, |icon| {
                    icon.child(
                        div()
                            .debug_selector(|| "left-panel-busy".to_string())
                            .absolute()
                            .top(px(2.))
                            .right(px(2.))
                            .child(
                                Indicator::new("left-panel-busy")
                                    .size(IndicatorSize::Xs)
                                    .color(lavender)
                                    .pulse(true),
                            ),
                    )
                }),
        )
    }

    /// What the main area shows with no view: in an empty project, the ways to start one.
    fn empty_state(&self, cx: &mut Context<Self>) -> EmptyState {
        if self.session.read(cx).project().instances().next().is_some() {
            return EmptyState::new(
                "Nothing to show",
                "Nothing in this project has a view yet. Ask the agent for one.",
            );
        }
        let templates = (TEMPLATES.iter())
            .filter(|template| !template.files.is_empty())
            .map(|template| {
                new_project::template_row(template, cx)
                    .on_click(cx.listener(move |shell, _, _, cx| shell.start_from(template, cx)))
            });
        let ask = self.left_panel.as_ref().map(|_| {
            Button::new("ask-the-agent", "Ask the agent")
                .on_click(cx.listener(|shell, _, window, cx| shell.focus_left_panel(window, cx)))
        });
        EmptyState::new(
            "An empty project",
            "Ask the agent for the tools you want, or start from a template.",
        )
        .action(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .w(px(420.))
                .children(templates)
                .children(ask),
        )
    }

    /// Writes `template` into this empty project, which then shows it as it would a change
    /// of the agent's.
    fn start_from(&mut self, template: &'static Template, cx: &mut Context<Self>) {
        let root = self.session.read(cx).project().root().to_path_buf();
        let session = self.session.downgrade();
        cx.spawn(async move |_, cx| {
            let written = (cx.background_executor())
                .spawn(async move { crate::templates::write(&root, template) })
                .await;
            if let Err(error) = written {
                // The window is gone when this fails, and there is nobody left to tell.
                (session.update(cx, |session, cx| session.report(format!("{error:#}"), cx))).ok();
            }
        })
        .detach();
    }

    fn show_main_instance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = Views::main_instance(&self.session, cx);
        if id.as_ref() == self.main.as_ref().map(|(id, _)| id) {
            return;
        }
        self.main = id.and_then(|id| {
            let view = Views::view_of(&self.session, &id, window, cx)?;
            Some((id, view))
        });
        self.title_bar =
            (self.main.as_ref()).and_then(|(_, view)| sound_typescript::title_bar_of(view, cx));
        cx.notify();
    }

    /// What is wrong, in one line each: the last error, and files that are not live. And a
    /// render while it runs.
    fn notices(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let update = cx.try_global::<update::Ready>().map(update_notice);
        let session = self.session.read(cx);
        let rendering = session.rendering().map(|(notice, progress)| {
            Notice::new("render", notice.clone())
                .tone(NoticeTone::Info)
                .progress(progress)
                .max_w_full()
        });
        let problems = session.project().problems().len();
        let agent = (self.left_panel.as_ref()).is_some_and(|panel| panel.ask.is_some());
        let files = match (problems, agent) {
            (0, _) => None,
            (1, true) => Some("1 file is not live.".to_string()),
            (count, true) => Some(format!("{count} files are not live.")),
            (1, false) => Some("1 file is not live. Ask the agent to fix it.".to_string()),
            (count, false) => Some(format!(
                "{count} files are not live. Ask the agent to fix them."
            )),
        };
        let fix = agent.then(|| {
            let label = if problems == 1 {
                "Ask the agent to fix it"
            } else {
                "Ask the agent to fix them"
            };
            Button::new("problems-ask", label)
                .debug_selector(|| "problems-ask".to_string())
                .focus_handle(&self.fix_focus)
                .on_click(cx.listener(|shell, _, window, cx| shell.ask_to_fix(window, cx)))
        });
        let error = session.notice().cloned();
        // Top-right, under the title row: clear of the transport, and of the left panel and the
        // left edge of the main view, where a song has its track headers. Not in the
        // title row: there are the window buttons on Windows. A definite width, so that a
        // message wraps at the width it gets and the box is as tall as its lines: with only a
        // largest width the text was measured on one line and then painted on three. A notice is as wide as
        // its text up to this width.
        div()
            .absolute()
            .top(px(TOP_ROW_HEIGHT + NOTICE_INSET))
            .right(px(NOTICE_INSET))
            .w(px(NOTICE_WIDTH))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(8.))
            .children(rendering)
            .children(update)
            .children(self.template_notice(cx))
            .children(files.map(|files| {
                let notice = Notice::new("problems", files)
                    .tone(NoticeTone::Warning)
                    .max_w_full();
                match fix {
                    Some(fix) => notice.action(fix),
                    None => notice,
                }
            }))
            .children(error.map(|error| {
                Notice::new("error", error)
                    .max_w_full()
                    .dismiss_focus(&self.dismiss_focus)
                    .on_dismiss(cx.listener(|shell, _, _, cx| {
                        shell
                            .session
                            .update(cx, |session, cx| session.dismiss_notice(cx))
                    }))
            }))
    }

    /// A newer template changes files the project changed: the agent merges them, when there is
    /// one.
    fn template_notice(&self, cx: &mut Context<Self>) -> Option<Notice> {
        let update = self.template_update.as_ref()?;
        let files = match update.files.len() {
            1 => "a file".to_string(),
            count => format!("{count} files"),
        };
        let message = format!(
            "The {} template changes {files} this project changed too",
            update.template.name
        );
        let merge = (self.left_panel.as_ref())
            .is_some_and(|panel| panel.composer.is_some())
            .then(|| {
                Button::new("template-merge", "Merge with the agent")
                    .on_click(cx.listener(|shell, _, window, cx| shell.merge_template(window, cx)))
            });
        let notice = Notice::new("template", message)
            .tone(NoticeTone::Info)
            .max_w_full()
            .on_dismiss(cx.listener(|shell, _, _, cx| {
                shell.template_update = None;
                cx.notify();
            }));
        Some(match merge {
            Some(merge) => notice.action(merge),
            None => notice,
        })
    }

    /// Puts the old and new template beside the project's files and writes what to ask in the
    /// agent's composer, for the composer to send.
    fn merge_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(update) = self.template_update.take() else {
            return;
        };
        let root = self.session.read(cx).project().root().to_path_buf();
        let ask = match crate::templates::begin_merge(&root, &update) {
            Ok(ask) => ask,
            Err(error) => {
                self.session
                    .update(cx, |session, cx| session.report(error, cx));
                return;
            }
        };
        if let Some(composer) = self
            .left_panel
            .as_ref()
            .and_then(|panel| panel.composer.as_ref())
        {
            let input = composer(cx);
            input.update(cx, |input, cx| input.set_text(ask, cx));
        }
        self.focus_left_panel(window, cx);
        cx.notify();
    }

    /// Asks the agent to fix the files that are not live, as the line under its turn does: the
    /// panel opens on the thread and the message goes. When the agent cannot take a message
    /// now, the panel opens on what it is doing.
    fn ask_to_fix(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.left_panel_open {
            self.set_left_panel_open(true, window, cx);
        }
        let ask = (self.left_panel.as_ref()).and_then(|panel| panel.ask.as_ref());
        if !ask.is_some_and(|ask| ask(sound_agent::FIX_NOT_LIVE, window, cx)) {
            self.focus_left_panel(window, cx);
        }
        cx.notify();
    }

    /// The bare parts of the top row move the window, as a title bar does. On Windows they are
    /// its caption: Windows moves and maximizes the window itself.
    fn drag_region() -> gpui::Div {
        div()
            .h_full()
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
    }

    /// Minimize, maximize and close on Windows, where the title row takes the place of the
    /// system's title bar. Windows presses them itself, as its own buttons.
    fn window_controls(window: &Window) -> Option<impl IntoElement> {
        if !cfg!(target_os = "windows") {
            return None;
        }
        let maximize_icon = if window.is_maximized() {
            "copy"
        } else {
            "square"
        };
        let button = |id: &'static str, icon: &'static str, area: WindowControlArea| {
            Button::icon_only(id, icon)
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Sm)
                .window_control_area(area)
                // So the drag region around it is not hit under it: the first area hit wins.
                .occlude()
        };
        Some(
            Self::drag_region()
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .child(button("minimize", "minus", WindowControlArea::Min))
                .child(button("maximize", maximize_icon, WindowControlArea::Max))
                .child(button("close", "x", WindowControlArea::Close)),
        )
    }
}

/// A newer release: Restart installs it, or Download opens its page when this app's folder
/// cannot take it or it did not install.
fn update_notice(ready: &update::Ready) -> Notice {
    let version = ready.version;
    let download = |page: &str| {
        let page = page.to_string();
        Button::new("update-download", "Download").on_click(move |_, _, cx| cx.open_url(&page))
    };
    let (message, tone, button) = match &ready.action {
        update::Action::Restart => (
            format!("Sound Tools {version} is ready"),
            NoticeTone::Info,
            Button::new("update-restart", "Restart").on_click(|_, _, cx| start_again(cx)),
        ),
        update::Action::Download { page } => (
            format!("Sound Tools {version} is ready"),
            NoticeTone::Info,
            download(page),
        ),
        update::Action::Failed { reason, page } => (
            format!("Sound Tools {version} did not install: {reason}"),
            NoticeTone::Error,
            download(page),
        ),
    };
    Notice::new("update", message)
        .tone(tone)
        .action(button)
        .max_w_full()
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, text) = (theme.gray_100, theme.gray_950);
        let main = match &self.main {
            Some((_, view)) => view.clone().into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(self.empty_state(cx))
                .into_any_element(),
        };

        div()
            .id("shell")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|shell, _: &ToggleComputerKeys, _, cx| {
                shell.live.update(cx, LiveInput::toggle_computer_keys)
            }))
            // Before the focused view hears it, so a key that plays does nothing else.
            .capture_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                let key = &event.keystroke.key;
                let free = keys_are_free(window) && !sound_typescript::takes_key(window, key);
                if (shell.live).update(cx, |live, _| live.computer_key_down(event, free)) {
                    cx.stop_propagation();
                }
            }))
            // Wherever the focus went since its key went down.
            .capture_key_up(cx.listener(|shell, event: &KeyUpEvent, _, cx| {
                let key = &event.keystroke.key;
                if shell.live.update(cx, |live, _| live.computer_key_up(key))
                    || GlobalKeys::key_up(&event.keystroke, cx)
                {
                    cx.stop_propagation();
                }
            }))
            // After the focused view, which may take the key itself, and only where the plain
            // keys are free: the keys of the project, such as space to play.
            .on_key_down(|event: &KeyDownEvent, window, cx| {
                if !event.is_held
                    && keys_are_free(window)
                    && GlobalKeys::key_down(&event.keystroke, cx)
                {
                    cx.stop_propagation();
                }
            })
            // macOS sends no key up while cmd is held.
            .on_modifiers_changed(cx.listener(|shell, event: &ModifiersChangedEvent, _, cx| {
                if event.modifiers.platform {
                    shell.live.update(cx, LiveInput::let_go_of_computer_keys);
                    GlobalKeys::let_go(cx);
                }
            }))
            .on_action(
                cx.listener(|shell, _: &Undo, _, cx| shell.session.update(cx, Session::undo)),
            )
            .on_action(
                cx.listener(|shell, _: &Redo, _, cx| shell.session.update(cx, Session::redo)),
            )
            .on_action(cx.listener(|shell, _: &ToggleLeftPanel, window, cx| {
                shell.toggle_left_panel(window, cx)
            }))
            .on_action(cx.listener(|shell, _: &OpenCloseLeftPanel, window, cx| {
                shell.open_or_close_left_panel(window, cx)
            }))
            .on_action(cx.listener(|shell, _: &NewProject, _, cx| {
                choose_new_project(shell.session.clone(), cx)
            }))
            .on_action(cx.listener(|shell, _: &OpenProject, _, cx| {
                shell.session.update(cx, |_, cx| open_another_project(cx))
            }))
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(background)
            .text_color(text)
            .font(typography::ui_font())
            .text_size(px(14.))
            // The title row: the project menu, then what the main view puts there, such as a
            // song's transport, in the middle of the room right of it. The air on both sides of the pill moves the window. In a narrow
            // window the air goes first, so the pill never covers the menu.
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(TOP_ROW_HEIGHT))
                    .child(Self::drag_region().flex_none().w(px(TITLE_ROW_INSET)))
                    .children(self.left_panel_icon(cx))
                    .child(div().flex_none().child(self.project_menu.clone()))
                    .child(Self::drag_region().flex_1())
                    .children(self.title_bar.clone())
                    .child(Self::drag_region().flex_1())
                    .children(Self::window_controls(window)),
            )
            // The panel comes first, so tab reaches it before the main area, as it reads.
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .children(self.left_panel_element(cx))
                    .child(div().flex_1().min_w_0().h_full().child(main)),
            )
            .child(self.notices(cx))
    }
}

/// The key context of the window root. Every binding of the window names it.
const KEY_CONTEXT: &str = "Shell";

/// Outside a text field, where keys are characters and cmd-z is not an undo of the project.
fn outside_text() -> String {
    format!("{KEY_CONTEXT} && !{}", text_input::KEY_CONTEXT)
}

/// Where the plain keys are free: outside a text field, and outside a tool of the project that
/// has the keys, which plays them.
fn plain_keys() -> String {
    format!("{} && !{}", outside_text(), sound_typescript::KEY_CONTEXT)
}

/// Whether the plain keys are free where the focus is: the computer keys and the keys of the
/// project may take them.
fn keys_are_free(window: &Window) -> bool {
    KeyBindingContextPredicate::parse(&plain_keys())
        .is_ok_and(|plain| plain.depth_of(&window.context_stack()).is_some())
}

/// The keys of the app. Undo works outside a text field. Tab moves the focus everywhere. The
/// computer keys and the keys of the project are no bindings: they need the key going up as
/// well, see the key listeners of [`Shell`]. `secondary` is cmd on macOS and ctrl on Windows
/// and Linux.
pub fn bind_keys(cx: &mut App) {
    let outside_text = outside_text();
    let outside_text = Some(outside_text.as_str());
    cx.bind_keys([
        KeyBinding::new("secondary-k", ToggleComputerKeys, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-z", Undo, outside_text),
        KeyBinding::new("shift-secondary-z", Redo, outside_text),
        KeyBinding::new("tab", FocusNext, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-l", ToggleLeftPanel, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-b", OpenCloseLeftPanel, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-n", NewProject, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-o", OpenProject, Some(KEY_CONTEXT)),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// Opening the left panel in a window too narrow for it and the main area makes the window
/// wider, when the screen has room on the right. Without room the main area gets narrower.
fn widen_for_left_panel(window: &mut Window, cx: &App) {
    let wanted = px(MIN_WINDOW_WIDTH + LEFT_PANEL_WIDTH);
    let bounds = window.bounds();
    if bounds.size.width >= wanted {
        return;
    }
    let Some(display) = window.display(cx) else {
        return;
    };
    if display.visible_bounds().right() - bounds.origin.x >= wanted {
        let height = window.viewport_size().height;
        window.resize(size(wanted, height));
    }
}

/// Gives the project menu of the app's window the recent projects. Off the UI thread: each is
/// a look at a folder, which a network drive that went away can hold up. A test window never
/// gets them, so a snapshot does not show what this machine opened.
fn show_recent_projects(window: gpui::WindowHandle<Shell>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let recent = cx
            .background_spawn(async { crate::app::recent_projects() })
            .await;
        // Fails only when the window is gone.
        window
            .update(cx, |shell, _, cx| {
                let menu = shell.project_menu().clone();
                menu.update(cx, |menu, cx| menu.set_recent_projects(recent, cx));
            })
            .ok();
    })
    .detach();
}

/// Drops a stream, or what holds one, on the background executor: Core Audio can take a while
/// to stop a stream and remove a tap, and the thread that draws must not wait for it.
fn drop_in_background(stream: impl Send + 'static, cx: &App) {
    cx.background_spawn(async move { drop(stream) }).detach();
}

/// The audio devices of this machine for the device menus of the app's window. A test window
/// lists none, so a snapshot does not show this machine's devices.
fn list_devices(chosen: ChosenDevices) -> ListDevices {
    Arc::new(move || DeviceMenu {
        // A failure leaves a list empty, and the menu offers the default.
        outputs: sound_core::output_devices().unwrap_or_default(),
        inputs: sound_core::input_devices().unwrap_or_default(),
        chosen: chosen.clone(),
    })
}

/// What the headless runtime prints at the end too, so a session in the window can be judged
/// the same way.
fn print_device_report(stream: &OutputStream) {
    let status = stream.status();
    println!("xruns: {}", status.xruns);
    println!("late callbacks: {}", status.late_callbacks);
    println!("slowest callback: {:?}", status.slowest_callback);
    println!("device output latency: {:?}", status.output_latency);
}

/// Opens the project, starts the device and runs the window until it closes. An error comes
/// back before any window, for the terminal that started it.
pub fn run(folder: &Path) -> Result<()> {
    // A window must not wait while a Sampler reads hundreds of samples, also not to open.
    sound_media::load_in_background();
    let opened = Opened::open(folder, Opened::load_tools(folder)?)?;
    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            init(cx);
            opened.show(cx);
        });
    Ok(())
}

/// The app with no folder, as the Finder starts it: the last project, or the folder panel
/// when there is none. See [`start`].
pub fn run_app() {
    sound_media::load_in_background();
    gpui_platform::application()
        .with_assets(Assets)
        .run(|cx: &mut App| {
            init(cx);
            match crate::app::last_project() {
                Some(folder) => start::open_or_explain(folder, cx),
                None => start::choose_project(cx),
            }
        });
}

fn init(cx: &mut App) {
    sound_ui::init(cx);
    bind_keys(cx);
    // Without a support folder the panel still works, and opens at every start.
    let support = match crate::app::support_folder() {
        Ok(support) => Some(support),
        Err(error) => {
            eprintln!("error: {error:#}");
            None
        }
    };
    if let Some(support) = &support {
        crate::update::check_in_background(support, cx);
    }
    crate::agent_panel(support, cx).install(cx);
}

/// A project that is open and plays on the chosen output, ready for its window.
struct Opened {
    project: Project,
    /// The project's own tools, for the window to keep live.
    extensions: Option<Extensions>,
    plugins: WeakPlugins,
    stream: OutputStream,
    device_name: String,
    chosen: ChosenDevices,
}

impl Opened {
    /// The project's own tools, for [`Opened::open`]. Waits for Bun: not on the thread that
    /// draws.
    fn load_tools(folder: &Path) -> Result<Option<FirstLoad>> {
        // An experiment: every project the window opens can have tools of its own, written
        // while it runs, so the folder for them is always there.
        std::fs::create_dir_all(folder.join(sound_typescript::FOLDER))?;
        crate::load_tools(folder)
    }

    fn open(folder: &Path, tools: Option<FirstLoad>) -> Result<Self> {
        let chosen = ChosenDevices::read();
        let device = OutputDevice::open(chosen.output.as_deref())?;
        let device_name = device.name()?;
        let config = EngineConfig::new(device.sample_rate(), device.channels());
        let (control, engine) = Engine::new(config);
        // The scan of this machine runs on a thread of its own from here, so no plugin is ever
        // looked at on the thread that draws. A project that names a plugin the scan has not
        // reached yet opens and plays everything else, and the plugin comes in when it turns
        // up: the tick below runs its behaviour again.
        let plugins = crate::plugins(false)?;
        plugins.start_scanning();
        let (mut project, extensions) = open_with_loaded(folder, control, plugins.clone(), tools)?;
        project.watch()?;
        // From here only the project holds the plugins, so that dropping the project ends them
        // and saves the state of every one. A handle kept here would outlive the project: the
        // application keeps this until it quits.
        let weak_plugins = plugins.downgrade();
        drop(plugins);
        let stream = device.start(engine)?;
        println!("device: {device_name}, {} Hz", config.sample_rate);
        // What the Finder opens next time. Not being able to remember it costs nothing now.
        if let Err(error) = crate::app::remember_project(project.root()) {
            eprintln!("error: {error:#}");
        }
        Ok(Self {
            project,
            extensions,
            plugins: weak_plugins,
            stream,
            device_name,
            chosen,
        })
    }

    /// Opens the window of the project, and ends the application with it.
    fn show(self, cx: &mut App) {
        let Self {
            project,
            extensions,
            plugins: weak_plugins,
            stream,
            device_name,
            chosen,
        } = self;
        let title = project
            .root()
            .file_name()
            .unwrap_or(project.root().as_os_str())
            .to_string_lossy()
            .into_owned();
        let stream = Rc::new(stream);
        let timing = stream.timing().clone();
        let session = cx.new(|cx| Session::new(project, cx));
        // A turn of a pinned knob in a plugin's window that is still open when the project
        // closes is written as one undo step, as the headless loop does with `Plugins::close`.
        // The release of the session is the last moment the project is at hand.
        cx.observe_release(&session, {
            let plugins = weak_plugins.clone();
            move |session, _| {
                if let Some(plugins) = plugins.upgrade() {
                    for error in session.closing(|project| plugins.end_turns(project)) {
                        eprintln!("error: {error}");
                    }
                }
            }
        })
        .detach();

        // A lost device must reach the composer. The stream reports it on its own thread.
        cx.spawn({
            let (session, stream) = (session.downgrade(), stream.clone());
            async move |cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let Some(session) = session.upgrade() else {
                        break;
                    };
                    for error in stream.take_errors() {
                        session.update(cx, |session, cx| session.report(error, cx));
                    }
                }
            }
        })
        .detach();
        // The background work of the extensions: the sounds of the Drum pads, the instruments
        // of the Samplers and the plugins of the project. One tick per session poll.
        sound_ui::on_every_poll(cx, {
            // Nothing strong is held: the plugins must go when the project goes, because
            // that is what saves the state of every one of them.
            let (session, plugins) = (session.downgrade(), weak_plugins.clone());
            // What the scan had found the last time a frame was asked for.
            let mut scanned = 0;
            move |cx| {
                let (Some(session), Some(plugins)) = (session.upgrade(), plugins.upgrade()) else {
                    return false;
                };
                tick(&session, &plugins, &mut scanned, cx);
                true
            }
        });
        // The state of every plugin reaches the project when the project is dropped, which
        // GPUI does with the views before any of this runs. See the plugin host.
        cx.on_app_quit({
            let plugins = weak_plugins.clone();
            move |cx| {
                // Before anything of the application is torn down: a plugin must not be
                // left holding the view of a window that is going.
                if let Some(plugins) = plugins.upgrade() {
                    plugins.close_all_windows(cx);
                }
                print_device_report(&stream);
                async {}
            }
        })
        .detach();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)),
                cx,
            ))),
            window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(16.), px(16.))),
            }),
            // Only the drag regions of the title row move the window. Otherwise macOS moves it
            // on a drag anywhere in its title bar strip, such as on the tempo in the pill.
            // Windows moves it only over them when it is movable.
            is_movable: !cfg!(target_os = "macos"),
            // Linux matches the window to sound-tools.desktop by it, for the icon.
            app_id: Some(crate::app::TOOL_NAME.into()),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                let (mut views, mut devices) = views(weak_plugins.clone());
                if let Some(extensions) = extensions {
                    sound_typescript::start_window(
                        extensions,
                        &session,
                        &mut views,
                        &mut devices,
                        cx,
                    );
                }
                let registries = (views, devices);
                let name = device_name.into();
                let device = DeviceAccess {
                    timing: Some(timing),
                    open_input: Some(audio_input::system_input(chosen.input.clone())),
                    // Only macOS has taps. Elsewhere every connection from an app is a
                    // problem that says so, and nothing is opened.
                    app_sounds: cfg!(target_os = "macos").then(AppSounds::system),
                    list_devices: Some(list_devices(chosen.clone())),
                };
                Shell::with_device(session.clone(), registries, name, device, window, cx)
            })
        });
        // The application ends with the main window, not with the last one: a plugin's own
        // window is a window of this application too, and one that is open when the
        // composer closes the project must not keep the process alive behind it.
        if let Ok(shell) = &opened {
            let main = shell.window_id();
            cx.on_window_closed(move |cx, closed| {
                if closed == main {
                    cx.quit();
                }
            })
            .detach();
        }
        let shell = match opened {
            Ok(window) => {
                cx.activate(true);
                show_recent_projects(window, cx);
                window
            }
            Err(error) => {
                eprintln!("error: the window did not open: {error}");
                cx.quit();
                return;
            }
        };
        // Every MIDI input port of the machine, read into the engine. The list is looked
        // at again every second, so a keyboard plugged in later works without a restart.
        let input = shell
            .read(cx)
            .ok()
            .and_then(|shell| shell.live_input().read(cx).midi_input());
        if let Some(input) = input {
            cx.spawn({
                let session = session.downgrade();
                async move |cx| {
                    let mut ports = midi::Ports::new(input);
                    loop {
                        // Nothing strong is held across the wait: a handle to the session
                        // that outlived the window would keep the project open, and its
                        // lock and `problems.txt` with it.
                        let Some(session) = session.upgrade() else {
                            break;
                        };
                        match ports.refresh() {
                            Ok(opened) => {
                                for name in opened {
                                    println!("midi in: {name}");
                                }
                            }
                            Err(error) => {
                                session.update(cx, |session, cx| session.report(error, cx));
                            }
                        }
                        drop(session);
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                    }
                }
            })
            .detach();
        }
    }
}

/// One [`crate::tick`] on the project of the session, with what it could not do as the notice,
/// and then what the window does for the plugin host. It is not an edit. The window does it
/// once per session poll; a test calls it when it settles. `scanned` is what the scan had
/// found the last time a frame was asked for.
pub fn tick(session: &Entity<Session>, plugins: &Plugins, scanned: &mut u64, cx: &mut App) {
    session.update(cx, |session, cx| {
        let (problems, errors) = session.background(cx, |project| crate::tick(project, plugins));
        for problem in problems {
            session.report(problem, cx);
        }
        // A bundle the scan could not read. It arrives while the scan runs, on its own
        // thread, so it is taken here and not once before the window.
        for notice in plugins.take_notices() {
            session.report(format!("plugin scan: {notice}"), cx);
        }
        for error in errors {
            session.report(error, cx);
        }
        // The picker shows what is known and says so quietly while a scan runs, so a frame
        // is drawn again while one does, and once more on the poll that sees the scan learn
        // something or end: that is when a menu filled while it ran is filled again.
        let generation = plugins.scan_generation();
        if plugins.scan_is_running() || generation != *scanned {
            *scanned = generation;
            cx.notify();
        }
    });
    // The window work that needs the application: the windows of plugins that have gone, and
    // a window whose plugin asked for another size.
    plugins.settle_windows(cx);
    // A plugin's window that opened or closed, which includes one the plugin itself closed, or
    // a plugin whose parameters or their text changed. The card that shows it is drawn again.
    if plugins.take_card_change() {
        session.update(cx, |_, cx| cx.notify());
    }
}
