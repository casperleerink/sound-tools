//! The window with no project: when the app starts with no folder and there is no last
//! project, or it did not open. It says why, lists the recent projects, and opens a project
//! folder or makes a new project. Closing it quits.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result};
use rune::{Cx, El, Key, KeyEvent, Tx, col, row, text};
use rune_kit::ButtonVariant;
use serde_json::json;
use sound_core::Project;

use super::dialog::button;
use super::{Host, Projects};
use crate::controls::{Step, read_steps};
use crate::theme::Theme;

const SIZE: (f32, f32) = (760.0, 560.0);
const MIN_SIZE: (f32, f32) = (560.0, 480.0);
const WIDTH: f32 = 440.0;
/// What the start state says when nothing went wrong.
const WHY: &str = "Pick the folder of a project, or start a new one.";
const CHOOSE_FOLDER: &str = "Choose folder…";
const NEW_PROJECT: &str = "New project…";
/// The step of `--act` that failed, for the run to end with.
static FAILED: Mutex<Option<String>> = Mutex::new(None);

pub(crate) struct Start {
    theme: Theme,
    shell: Projects,
    /// Why no project is open, such as the last one that did not.
    message: Option<String>,
    /// The keys go to the first choice once it shows.
    focused: bool,
    /// The steps of `--act`, which run once it shows, and the first that failed.
    steps: Vec<Step>,
    failed: Option<String>,
}

/// What a control of the start state does, by key or by `--act`.
#[derive(Clone, Copy)]
enum Act {
    Recent(usize),
    ChooseFolder,
    NewProject,
    Dialog(super::Act),
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
            steps: Vec::new(),
            failed: None,
        }
    }

    /// Its controls for `--tree` and `--act`: the open dialog's, else its own.
    fn controls(&self) -> Vec<(String, String, &'static str, Act)> {
        if self.shell.dialog.is_some() {
            let controls = self.shell.controls().into_iter();
            return controls
                .map(|control| {
                    (
                        control.key,
                        control.label,
                        control.role,
                        Act::Dialog(control.act),
                    )
                })
                .collect();
        }
        let recent = self.shell.recent().into_iter().enumerate();
        let recent = recent.map(|(index, folder)| {
            let name = folder.file_name().unwrap_or(folder.as_os_str());
            let label = format!("Open {}", name.to_string_lossy());
            (
                format!("recent-{index}"),
                label,
                "button",
                Act::Recent(index),
            )
        });
        let own = [
            ("choose-folder", CHOOSE_FOLDER, Act::ChooseFolder),
            ("new-project", NEW_PROJECT, Act::NewProject),
        ];
        let own = own.map(|(key, label, act)| (key.to_string(), label.to_string(), "button", act));
        recent.chain(own).collect()
    }

    fn press(&mut self, act: Act, value: Option<String>, tx: &mut Tx) {
        match act {
            Act::Recent(index) => {
                if let Some(folder) = self.shell.recent().get(index).cloned() {
                    self.open(&folder, tx);
                }
            }
            Act::ChooseFolder => Projects::choose_project::<Start>(tx),
            Act::NewProject => self.shell.new_project(),
            Act::Dialog(act) => super::run(self, act, value, tx),
        }
    }

    /// The steps of `--act`, in order, until one names no control.
    fn act(&mut self, tx: &mut Tx) {
        for step in std::mem::take(&mut self.steps) {
            let found =
                (self.controls().into_iter()).find(|(key, label, _, _)| step.acts_on(key, label));
            let Some((_, _, _, act)) = found else {
                self.failed = Some(format!("there is no {} here", step.named()));
                return;
            };
            self.press(act, step.text(), tx);
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
    let mut arguments = arguments;
    let mut start = Start::new(Projects::machine(), headless, message);
    if let Some(at) = arguments.iter().position(|argument| argument == "--act") {
        let file = arguments.drain(at..at + 2).nth(1);
        start.steps = read_steps(Path::new(&file.context("--act takes a file of steps")?))?;
    }
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
    // The steps of `--act` ran on a state the app has given back by now.
    if let Some(failed) = FAILED.lock().unwrap_or_else(PoisonError::into_inner).take() {
        anyhow::bail!(failed);
    }
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
    start.act(tx);
    if let Some(failed) = start.failed.take() {
        *FAILED.lock().unwrap_or_else(PoisonError::into_inner) = Some(failed);
        tx.quit();
    }
    if start.shell.headless {
        let report = json!({
            "message": start.message,
            "opened": start.shell.opened,
            "controls": start.controls().into_iter().map(|(key, label, role, _)| json!({
                "key": key, "label": label, "role": role,
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
            CHOOSE_FOLDER,
            ButtonVariant::Contrast,
            true,
            |_: &mut Start, tx| Projects::choose_project::<Start>(tx),
        ))
        .child(button(
            theme,
            "new-project",
            NEW_PROJECT,
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
