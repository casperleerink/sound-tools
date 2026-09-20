//! The application window: the view of the project's main instance, the project menu top-left,
//! the floating transport bottom centre, and a quiet line for errors and problems.
//!
//! The window is the project runtime. It names no extension type: the main area shows whatever
//! view [`Views`] has for the first instance at the top of the project.

mod project_menu;
mod transport;

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use gpui::{
    AnyView, App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton,
    MouseDownEvent, SharedString, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions,
    actions, div, point, prelude::*, px, size,
};
use sound_core::{
    Engine, EngineConfig, InstanceId, OutputDevice, OutputStream, Project, ProjectEvent,
};
use sound_ui::components::empty_state::EmptyState;
use sound_ui::components::notice::{Notice, NoticeTone};
use sound_ui::{ActiveTheme, Assets, Session, Views, typography};

use project_menu::ProjectMenu;
use transport::TransportPill;

use crate::{open_or_create, views};

actions!(
    sound_tools,
    [TogglePlayback, Undo, Redo, FocusNext, FocusPrevious, Quit]
);

/// Room for the traffic lights of a macOS window, left of the project menu.
const TRAFFIC_LIGHTS_WIDTH: f32 = 72.;
const TOP_ROW_HEIGHT: f32 = 48.;

/// The root view of the window.
pub struct Shell {
    session: Entity<Session>,
    views: Views,
    /// The instance in the main area and its view. Both change when it comes or goes.
    main: Option<(InstanceId, AnyView)>,
    project_menu: Entity<ProjectMenu>,
    transport: Entity<TransportPill>,
    focus_handle: FocusHandle,
}

impl Shell {
    pub fn new(
        session: Entity<Session>,
        views: Views,
        device_name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
        let mut shell = Self {
            project_menu: cx.new(|cx| ProjectMenu::new(session.clone(), device_name, cx)),
            transport: cx.new(|cx| TransportPill::new(session.clone(), cx)),
            session,
            views,
            main: None,
            focus_handle,
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

    fn show_main_instance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.views.main_instance(&self.session, cx);
        if id.as_ref() == self.main.as_ref().map(|(id, _)| id) {
            return;
        }
        self.main = id.and_then(|id| {
            let view = self.views.view_of(&self.session, &id, window, cx)?;
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
            .track_focus(&self.focus_handle)
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

/// Key bindings and the actions that need no window. They hold the session weakly: when the
/// window closes, the session and with it the project must go, so the project folder is left
/// as a clean close leaves it.
fn bind_actions(session: WeakEntity<Session>, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", TogglePlayback, None),
        KeyBinding::new("cmd-z", Undo, None),
        KeyBinding::new("shift-cmd-z", Redo, None),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // A session that is gone means the app is closing. There is nothing left to act on.
    let on_session = move |cx: &mut App, action: fn(&mut Session, &mut Context<Session>)| {
        if let Some(session) = session.upgrade() {
            session.update(cx, action);
        }
    };
    cx.on_action({
        let on_session = on_session.clone();
        move |_: &TogglePlayback, cx| on_session(cx, Session::toggle_playback)
    });
    cx.on_action({
        let on_session = on_session.clone();
        move |_: &Undo, cx| {
            on_session(cx, |session, cx| {
                session.edit(cx, Project::undo);
            })
        }
    });
    cx.on_action(move |_: &Redo, cx| {
        on_session(cx, |session, cx| {
            session.edit(cx, Project::redo);
        })
    });
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// What the headless runtime prints at the end too, so a session in the window can be judged
/// the same way.
fn print_device_report(stream: &OutputStream) {
    let status = stream.status();
    println!("xruns: {}", status.xruns);
    println!("late callbacks: {}", status.late_callbacks);
    println!("slowest callback: {:?}", status.slowest_callback);
}

/// Opens the project, starts the device and runs the window until it closes.
pub fn run(folder: &Path) -> Result<()> {
    let device = OutputDevice::default_output()?;
    let device_name = device.name()?;
    let config = EngineConfig::new(device.sample_rate(), device.channels());
    let (control, engine) = Engine::new(config);
    let mut project = open_or_create(folder, control)?;
    project.watch()?;
    let stream = Rc::new(device.start(engine)?);
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
            bind_actions(session.downgrade(), cx);

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
                cx.new(|cx| Shell::new(session, views(), device_name.into(), window, cx))
            });
            match opened {
                Ok(_) => cx.activate(true),
                Err(error) => {
                    eprintln!("error: the window did not open: {error}");
                    cx.quit();
                }
            }
        });
    Ok(())
}
