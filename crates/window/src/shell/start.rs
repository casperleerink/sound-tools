//! The window with no project: when the app starts with no folder and there is no last
//! project, or it did not open. It says why, lists the recent projects, and opens a project
//! folder or makes a new project. Closing it quits.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use anyhow::Result;
use rune::{Cx, El, Key, KeyEvent, Tx, col, row, text};
use rune_kit::ButtonVariant;
use serde_json::json;
use sound_core::Project;

use super::dialog::button;
use super::{Host, Projects};
use crate::theme::Theme;

const SIZE: (f32, f32) = (760.0, 560.0);
const MIN_SIZE: (f32, f32) = (560.0, 480.0);
const WIDTH: f32 = 440.0;
/// What the start state says when nothing went wrong.
const WHY: &str = "Pick the folder of a project, or start a new one.";

pub(crate) struct Start {
    theme: Theme,
    shell: Projects,
    /// Why no project is open, such as the last one that did not.
    message: Option<String>,
    /// The keys go to the first choice once it shows.
    focused: bool,
}

impl Host for Start {
    fn shell(&self) -> &Projects {
        &self.shell
    }

    fn shell_mut(&mut self) -> &mut Projects {
        &mut self.shell
    }

    fn project(&self) -> Option<&Project> {
        None
    }

    fn tell(&mut self, message: String) {
        self.message = Some(message);
    }
}

impl Start {
    /// With the recent projects and saved templates of `support`; None reads none.
    pub(crate) fn new(support: Option<PathBuf>, headless: bool, message: Option<String>) -> Self {
        Self {
            theme: Theme::of(Path::new("")).0,
            shell: Projects::new(support, headless, None),
            message,
            focused: false,
        }
    }

    /// The key of what the keys start on: the last project, else the folder panel.
    fn first(&self) -> String {
        match self.shell.recent().is_empty() {
            true => "choose-folder".into(),
            false => "recent-0".into(),
        }
    }

    fn open(&mut self, folder: &Path, tx: &mut Tx) {
        if let Err(error) = self.shell.open(folder, tx) {
            self.message = Some(format!("{error:#}"));
        }
    }
}

/// Runs the start state, with Rune's `arguments` such as `--snap`, and why no project opened.
pub(crate) fn run(arguments: Vec<String>, message: Option<String>) -> Result<()> {
    let headless = !arguments.is_empty();
    let tree = (arguments.iter())
        .position(|argument| argument == "--tree")
        .and_then(|at| arguments.get(at + 1))
        .map(PathBuf::from);
    let start = Start::new(Projects::machine(), headless, message);
    let spec = rune::AppSpec {
        min_size: Some(MIN_SIZE),
        poll: Some(poll),
        on_key: Some(on_key),
        fonts: &crate::FONTS,
        families: ("Inter Display", "ui-monospace"),
        ..rune::AppSpec::new(
            "Sound Tools",
            SIZE,
            || unreachable!("the state is made before"),
            view,
        )
    };
    rune::App::new(spec)
        .chrome(rune::Chrome {
            traffic_lights: Some(crate::app::WINDOW_BUTTONS),
            drag_regions: true,
            ..rune::Chrome::DEFAULT
        })
        .tab_visits_controls(true)
        .args(arguments)
        .init(move || start)
        .run();
    if super::starts_again() {
        runtime::app::start_again()?;
    }
    if let Some(tree) = tree {
        crate::add_report(&tree)?;
    }
    Ok(())
}

pub(super) fn poll(start: &mut Start, _: f64, tx: &mut Tx) -> bool {
    if !start.focused {
        start.focused = true;
        tx.focus_when_shown(start.first(), false);
    }
    if start.shell.headless {
        let report = json!({
            "message": start.message,
            "opened": start.shell.opened,
            "controls": start.shell.controls().iter().map(|control| json!({
                "key": control.key, "label": control.label, "role": control.role,
            })).collect::<Vec<_>>(),
        });
        *crate::app::REPORT
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(report);
    }
    false
}

/// cmd-O opens a project, cmd-N makes one, cmd-Q quits.
pub(crate) fn on_key(start: &mut Start, key: &KeyEvent, tx: &mut Tx) -> bool {
    match &key.key {
        Key::Command('o') => Projects::choose_project::<Start>(tx),
        Key::Command('n') => start.shell.new_project(),
        Key::Command('q') => tx.quit(),
        _ => return false,
    }
    true
}

pub(crate) fn view(start: &Start, cx: &mut Cx<Start>) -> El {
    let theme = &start.theme;
    cx.focus_ring(2.0, 2.0, theme.color("focus"));
    let recent = start.shell.recent();
    let rows = recent
        .iter()
        .enumerate()
        .map(|(index, folder)| recent_row(theme, index, folder));
    let recent = (!recent.is_empty()).then(|| {
        col()
            .key("recent")
            .role("list")
            .label("Recent")
            .focus_group()
            .w_full()
            .gap(theme.space(1))
            .child(
                text("Recent")
                    .font(theme.text("label"))
                    .color(theme.color("text-secondary"))
                    .pb(theme.space(1)),
            )
            .children(rows)
    });
    let message = start.message.as_deref().unwrap_or(WHY);
    let actions = row()
        .gap(theme.space(2))
        .child(button(
            theme,
            "choose-folder",
            "Choose folder…",
            ButtonVariant::Contrast,
            true,
            |_: &mut Start, tx| Projects::choose_project::<Start>(tx),
        ))
        .child(button(
            theme,
            "new-project",
            "New project…",
            ButtonVariant::Neutral,
            true,
            |start: &mut Start, _| start.shell.new_project(),
        ));
    let content = col()
        .w_full()
        .max_w(WIDTH)
        .gap(theme.space(4))
        .child(
            col()
                .gap(theme.space(1))
                .child(
                    text("Open a project")
                        .font(theme.text("title"))
                        .color(theme.color("text")),
                )
                .child(
                    text(message.to_string())
                        .key("start-message")
                        .font(theme.text("body"))
                        .color(theme.color(match start.message {
                            Some(_) => "danger",
                            None => "text-secondary",
                        }))
                        .wrap(),
                ),
        )
        .children(recent)
        .child(actions);
    let base = col()
        .w_full()
        .h_full()
        .fill(theme.color("background"))
        .child(el_drag_row())
        .child(
            col()
                .grow(1.0)
                .w_full()
                .items_center()
                .justify_center()
                .padding(theme.space(6))
                .child(content),
        );
    rune_kit::root(base, super::overlay(start, theme))
}

/// The row under the window's buttons, which moves the window.
fn el_drag_row() -> El {
    rune::el().w_full().h(40.0).window_drag()
}

/// A recent project by its name and the folder it is in.
fn recent_row(theme: &Theme, index: usize, folder: &Path) -> El {
    let name = folder.file_name().unwrap_or(folder.as_os_str());
    let name = name.to_string_lossy().into_owned();
    let place = folder.parent().map(|parent| parent.display().to_string());
    let folder = folder.to_path_buf();
    let e = col()
        .key(format!("recent-{index}"))
        .role("button")
        .label(format!("Open {name}"))
        .focusable(true)
        .w_full()
        // Its words line up with the heading; its hover fill reaches out into the margin.
        .ml(-theme.space(3))
        .px(theme.space(3))
        .py(theme.space(2))
        .gap(2.0)
        .radius(theme.kit.radius.md)
        .on_press_tx(move |start: &mut Start, tx: &mut Tx| start.open(&folder, tx))
        .child(
            text(name)
                .font(theme.text("heading"))
                .color(theme.color("text")),
        )
        .children(place.map(|place| {
            text(place)
                .font(theme.text("label"))
                .color(theme.color("text-secondary"))
        }));
    theme.kit.interactive(e)
}

/// What the start state says of a project that did not open.
pub(crate) fn did_not_open(folder: &Path, error: &anyhow::Error) -> String {
    let name = folder.file_name().unwrap_or(folder.as_os_str());
    format!("“{}” did not open: {error:#}", name.to_string_lossy())
}
