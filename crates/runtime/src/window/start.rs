//! The app started with no folder, as the Finder starts it. It opens the last project, or
//! shows the macOS folder panel when there is none: the panel has a New Folder button, and a
//! new empty folder becomes a new, empty project. Cancel quits. The start window also makes a
//! new project from a template.
//!
//! When a project does not open, a small window says why and offers the panel again, since an
//! app started from the Finder has no terminal to print to.

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Global, PathPromptOptions, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};
use sound_ui::components::button::Button;
use sound_ui::components::empty_state::EmptyState;
use sound_ui::{ActiveTheme, typography};

use super::Opened;
use crate::app::check_project_folder;

const START_WIDTH: f32 = 520.;
const START_HEIGHT: f32 = 280.;

/// The folder panel of macOS, for a project folder.
pub(super) fn folder_prompt() -> PathPromptOptions {
    PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open".into()),
    }
}

/// A project is on its way to its window. One at a time: a second would open in this process
/// too, with its own lock and quit handler, and take the MIDI keyboard from the first.
struct Opening(bool);

impl Global for Opening {}

fn opening(cx: &App) -> bool {
    cx.try_global::<Opening>().is_some_and(|opening| opening.0)
}

/// Opens the project in `folder` in its window, or says why it did not open. Its tools load on
/// the background executor first. Does nothing while another project opens.
pub(super) fn open_or_explain(folder: PathBuf, cx: &mut App) {
    if opening(cx) {
        return;
    }
    cx.set_global(Opening(true));
    let tools = cx.background_spawn({
        let folder = folder.clone();
        async move { Opened::load_tools(&folder) }
    });
    cx.spawn(async move |cx| {
        let opened = tools.await.and_then(|tools| Opened::open(&folder, tools));
        cx.update(|cx| {
            cx.set_global(Opening(false));
            match opened {
                Ok(opened) => {
                    opened.show(cx);
                    close_start_windows(cx);
                }
                Err(error) => {
                    let name = folder.file_name().unwrap_or(folder.as_os_str());
                    let message = format!("“{}” did not open: {error:#}", name.to_string_lossy());
                    eprintln!("error: {message}");
                    explain(message, cx);
                }
            }
        });
    })
    .detach();
}

/// Shows the folder panel and opens the folder picked in it. Cancel quits, unless the start
/// window is open: then the composer is back at it. Does nothing while a project opens.
pub(super) fn choose_project(cx: &mut App) {
    if opening(cx) {
        return;
    }
    cx.activate(true);
    let picked = cx.prompt_for_paths(folder_prompt());
    cx.spawn(async move |cx| {
        let folder = picked_folder(picked.await.ok());
        cx.update(|cx| match folder {
            Ok(Some(folder)) => match check_project_folder(&folder) {
                Ok(()) => open_or_explain(folder, cx),
                Err(error) => explain(error.to_string(), cx),
            },
            Ok(None) => {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            }
            Err(error) => explain(error, cx),
        });
    })
    .detach();
}

/// Asks where a new project goes, writes `template` there and opens it. Does nothing while a
/// project opens.
fn new_project(template: &'static crate::Template, cx: &mut App) {
    if opening(cx) {
        return;
    }
    let home = std::env::home_dir().unwrap_or_default();
    let picked = cx.prompt_for_new_path(&home, Some("New project"));
    cx.spawn(async move |cx| {
        let folder = match picked.await {
            Ok(Ok(Some(folder))) => folder,
            Ok(Ok(None)) | Err(_) => return,
            Ok(Err(error)) => {
                cx.update(|cx| explain(format!("The save panel did not open: {error:#}"), cx));
                return;
            }
        };
        let written = cx
            .background_spawn({
                let folder = folder.clone();
                async move {
                    check_project_folder(&folder)?;
                    crate::templates::write(&folder, template)
                }
            })
            .await;
        cx.update(|cx| match written {
            Ok(()) => open_or_explain(folder, cx),
            Err(error) => explain(format!("{error:#}"), cx),
        });
    })
    .detach();
}

/// The one folder of an answer of the panel, `None` when it was cancelled. A panel that went
/// away without an answer is a cancel too.
pub(super) fn picked_folder(
    answer: Option<anyhow::Result<Option<Vec<PathBuf>>>>,
) -> Result<Option<PathBuf>, String> {
    match answer {
        Some(Ok(paths)) => Ok(paths.and_then(|paths| paths.into_iter().next())),
        Some(Err(error)) => Err(format!("The folder panel did not open: {error:#}")),
        None => Ok(None),
    }
}

/// Says why no project is open, in the start window.
fn explain(message: String, cx: &mut App) {
    let message = SharedString::from(message);
    let start = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<StartView>());
    if let Some(start) = start {
        let shown = start.update(cx, |start, _, cx| {
            start.message = message;
            cx.notify();
        });
        if let Err(error) = shown {
            eprintln!("error: {error:#}");
        }
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui::Bounds::centered(
            None,
            size(px(START_WIDTH), px(START_HEIGHT)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Sound Tools".into()),
            ..Default::default()
        }),
        // Linux matches the window to sound-tools.desktop by it, for the icon.
        app_id: Some(crate::app::TOOL_NAME.into()),
        ..Default::default()
    };
    match cx.open_window(options, |_, cx| cx.new(|_| StartView { message })) {
        Ok(window) => {
            // Closing it quits, unless it closed because a project window took its place.
            let id = window.window_id();
            cx.on_window_closed(move |cx, closed| {
                if closed == id && cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        }
        Err(error) => {
            eprintln!("error: the window did not open: {error:#}");
            cx.quit();
        }
    }
}

fn close_start_windows(cx: &mut App) {
    for window in cx.windows() {
        if let Some(start) = window.downcast::<StartView>()
            && let Err(error) = start.update(cx, |_, window, _| window.remove_window())
        {
            eprintln!("error: {error:#}");
        }
    }
}

/// Why no project is open, and the way to pick one.
struct StartView {
    message: SharedString,
}

impl Render for StartView {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.gray_100)
            .text_color(theme.gray_950)
            .font(typography::ui_font())
            .text_size(px(14.))
            .child(
                EmptyState::new("Open a project", self.message.clone())
                    .max_w(px(START_WIDTH - 48.))
                    .action(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("choose-folder", "Choose folder…")
                                    .on_click(|_, _, cx| choose_project(cx)),
                            )
                            .child(Button::new("new-project", "New project…").on_click(
                                |_, _, cx| {
                                    let on_pick =
                                        |template, cx: &mut App| new_project(template, cx);
                                    super::new_project::choose_template(Rc::new(on_pick), cx)
                                },
                            )),
                    ),
            )
    }
}
