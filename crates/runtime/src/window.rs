//! The application window: the view of the project's main instance, the project menu top-left,
//! the floating transport bottom centre, and a quiet line for errors and problems.
//!
//! The window is the project runtime. It names no extension type: the main area shows whatever
//! view the installed [`Views`] has for the first instance at the top of the project.

mod project_menu;
pub mod recording;
pub mod tempo;
pub mod transport;

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::{
    AnyView, App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton,
    MouseDownEvent, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, actions,
    div, point, prelude::*, px, size,
};
use midi::{Latency, Lost};
use sound_core::{
    Engine, EngineConfig, InstanceId, OutputDevice, OutputStream, ProjectEvent, StreamTiming,
};
use sound_ui::components::empty_state::EmptyState;
use sound_ui::components::notice::{Notice, NoticeTone};
use sound_ui::{ActiveTheme, Assets, Session, Views, typography};

use project_menu::ProjectMenu;
pub use transport::TransportPill;

use crate::{open_or_create, views};

actions!(
    sound_tools,
    [
        TogglePlayback,
        ToggleRecording,
        Undo,
        Redo,
        FocusNext,
        FocusPrevious,
        Quit
    ]
);

/// Room for the traffic lights of a macOS window, left of the project menu.
const TRAFFIC_LIGHTS_WIDTH: f32 = 80.;
const TOP_ROW_HEIGHT: f32 = 48.;

/// The root view of the window.
pub struct Shell {
    session: Entity<Session>,
    /// The instance in the main area and its view. Both change when it comes or goes.
    main: Option<(InstanceId, AnyView)>,
    project_menu: Entity<ProjectMenu>,
    transport: Entity<TransportPill>,
    focus_handle: FocusHandle,
    dismiss_focus: FocusHandle,
}

impl Shell {
    /// Installs `views` as the registry of the application, so that no caller can forget it.
    pub fn new(
        session: Entity<Session>,
        views: Views,
        device_name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_device(session, views, device_name, None, window, cx)
    }

    /// The window of the real runtime, which has a device. Everything else passes `None` for
    /// the timing and measures no latency.
    pub fn with_device(
        session: Entity<Session>,
        views: Views,
        device_name: SharedString,
        timing: Option<Arc<StreamTiming>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        views.install(cx);
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&session, window, |shell, _, event, window, cx| {
            let at_top = |id: &InstanceId| id.parent().is_none();
            if matches!(event, ProjectEvent::Created(id) | ProjectEvent::Deleted(id) if at_top(id))
            {
                shell.show_main_instance(window, cx);
            }
        })
        .detach();

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        // The keys of the window work from its key context, so the focus must stay inside it.
        // A control that goes away while it has the focus, such as a dismissed notice, would
        // leave it nowhere.
        cx.on_focus_lost(window, |shell, window, cx| {
            window.focus(&shell.focus_handle, cx)
        })
        .detach();
        let mut shell = Self {
            project_menu: cx.new(|cx| ProjectMenu::new(session.clone(), device_name, cx)),
            transport: cx.new(|cx| TransportPill::with_device(session.clone(), timing, cx)),
            session,
            main: None,
            focus_handle,
            dismiss_focus: cx.focus_handle().tab_stop(true),
        };
        shell.show_main_instance(window, cx);
        shell
    }

    pub fn main_view(&self) -> Option<&AnyView> {
        self.main.as_ref().map(|(_, view)| view)
    }

    pub fn project_menu(&self) -> &Entity<ProjectMenu> {
        &self.project_menu
    }

    pub fn transport(&self) -> &Entity<TransportPill> {
        &self.transport
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
        cx.notify();
    }

    /// What is wrong, in one line each: the last error, and files that are not live.
    fn notices(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let session = self.session.read(cx);
        let problems = session.project().problems().len();
        let files = match problems {
            0 => None,
            1 => Some("1 file is not live, see problems.txt".to_string()),
            count => Some(format!("{count} files are not live, see problems.txt")),
        };
        let error = session.notice().cloned();
        div()
            .absolute()
            .left(px(24.))
            .bottom(px(24.))
            .max_w(px(400.))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(8.))
            .children(files.map(|files| Notice::new("problems", files).tone(NoticeTone::Warning)))
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

    /// The bare parts of the top row move the window, as a title bar does.
    fn drag_region() -> gpui::Div {
        div()
            .h_full()
            .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, text) = (theme.gray_100, theme.gray_950);
        let main = match &self.main {
            Some((_, view)) => view.clone().into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(EmptyState::new(
                    "Nothing to show",
                    "This project has no arrangement at the top of state/.",
                ))
                .into_any_element(),
        };

        div()
            .id("shell")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|shell, _: &TogglePlayback, _, cx| {
                shell.session.update(cx, Session::toggle_playback)
            }))
            .on_action(cx.listener(|shell, _: &ToggleRecording, _, cx| {
                shell.transport.update(cx, TransportPill::toggle_recording)
            }))
            .on_action(
                cx.listener(|shell, _: &Undo, _, cx| shell.session.update(cx, Session::undo)),
            )
            .on_action(
                cx.listener(|shell, _: &Redo, _, cx| shell.session.update(cx, Session::redo)),
            )
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
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(TOP_ROW_HEIGHT))
                    .child(Self::drag_region().w(px(TRAFFIC_LIGHTS_WIDTH)))
                    .child(self.project_menu.clone())
                    .child(Self::drag_region().flex_1()),
            )
            .child(div().flex_1().min_h_0().child(main))
            .child(
                div()
                    .absolute()
                    .bottom(px(24.))
                    .left_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(self.transport.clone()),
            )
            .child(self.notices(cx))
    }
}

/// The key context of the window root. Every binding of the window names it.
const KEY_CONTEXT: &str = "Shell";

/// The keys of the window. Space, record and undo belong to a focused text field first: there
/// they are characters and cmd-z is not an undo of the project. Tab moves the focus everywhere.
pub fn bind_keys(cx: &mut App) {
    let outside_text = Some("Shell && !TextInput");
    cx.bind_keys([
        KeyBinding::new("space", TogglePlayback, outside_text),
        KeyBinding::new("r", ToggleRecording, outside_text),
        KeyBinding::new("cmd-z", Undo, outside_text),
        KeyBinding::new("shift-cmd-z", Redo, outside_text),
        KeyBinding::new("tab", FocusNext, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
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

/// What a key press cost, measured over the session. It covers the wait for the next audio
/// block and the output latency the device reports, not the keyboard and its cable.
fn print_midi_report(latency: Latency, lost: Lost) {
    if latency.count() == 0 {
        return;
    }
    println!(
        "midi to sound over {} messages: mean {:?}, shortest {:?}, longest {:?}, jitter {:?}",
        latency.count(),
        latency.mean(),
        latency.shortest(),
        latency.longest(),
        latency.spread()
    );
    if lost.any() {
        println!(
            "midi lost: {} messages never played, {} missing from a take",
            lost.input, lost.reports
        );
    }
}

/// Opens the project, starts the device and runs the window until it closes.
pub fn run(folder: &Path) -> Result<()> {
    let device = OutputDevice::default_output()?;
    let device_name = device.name()?;
    let config = EngineConfig::new(device.sample_rate(), device.channels());
    let (control, engine) = Engine::new(config);
    let (mut project, plugins) = open_or_create(folder, control)?;
    project.watch()?;
    for notice in plugins.take_notices() {
        println!("plugin scan: {notice}");
    }
    // From here only the project holds the plugins, so that dropping the project ends them and
    // saves the state of every one. A handle kept here would outlive the project: this
    // function returns after the application has quit.
    let weak_plugins = plugins.downgrade();
    drop(plugins);
    let stream = Rc::new(device.start(engine)?);
    let timing = stream.timing().clone();
    let title = project
        .root()
        .file_name()
        .context("the project folder has no name")?
        .to_string_lossy()
        .into_owned();
    println!("device: {device_name}, {} Hz", config.sample_rate);

    gpui_platform::application()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            sound_ui::init(cx);
            let session = cx.new(|cx| Session::new(project, cx));
            bind_keys(cx);

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
            // The plugins of the project: the main-thread callbacks they ask for, and the
            // state they say changed, written into the project. One poll per session poll.
            cx.spawn({
                // Nothing strong is held: the plugins must go when the project goes, because
                // that is what saves the state of every one of them.
                let (session, plugins) = (session.downgrade(), weak_plugins.clone());
                async move |cx| {
                    loop {
                        cx.background_executor()
                            .timer(sound_ui::POLL_INTERVAL)
                            .await;
                        let (Some(session), Some(plugins)) = (session.upgrade(), plugins.upgrade())
                        else {
                            break;
                        };
                        let problems =
                            session.read_with(cx, |session, _| plugins.poll(session.project()));
                        for problem in problems {
                            session.update(cx, |session, cx| session.report(problem, cx));
                        }
                    }
                }
            })
            .detach();
            // The state of every plugin reaches the project when the project is dropped, which
            // GPUI does with the views before any of this runs. See the plugin host.
            cx.on_app_quit(move |_| {
                print_device_report(&stream);
                async {}
            })
            .detach();
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1440.), px(900.)),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some(title.into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(16.))),
                }),
                ..Default::default()
            };
            let opened = cx.open_window(options, |window, cx| {
                cx.new(|cx| {
                    let views = views();
                    let name = device_name.into();
                    Shell::with_device(session.clone(), views, name, Some(timing), window, cx)
                })
            });
            let shell = match opened {
                Ok(window) => {
                    cx.activate(true);
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
                .and_then(|shell| shell.transport().read(cx).midi_input());
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
        });
    Ok(())
}
