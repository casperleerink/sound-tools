//! The project menu: the project name top-left as a quiet menu. Add track, undo and redo with
//! what they would do, the output device by name, and the project folder in the Finder or in
//! a terminal. The terminal is where the composer starts a coding agent on the project.

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{Context, Entity, IntoElement, Render, SharedString, Window, prelude::*};
use sound_core::{Changes, InstanceId};
use sound_notes::Clip;
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::{Session, extension_is_enabled};

use crate::{add_track, main_arrangement};

const ADD_TRACK: &str = "add-track";
const FIT_TEMPO: &str = "fit-tempo";
const UNDO: &str = "undo";
const REDO: &str = "redo";
const DEVICE: &str = "device";
const REVEAL: &str = "reveal";
const TERMINAL: &str = "terminal";

pub struct ProjectMenu {
    session: Entity<Session>,
    device_name: SharedString,
    menu: Entity<DropdownMenu>,
    /// What the items were made from. They are made again only when this changes.
    shown: Shown,
}

/// All that the items depend on in the project.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    can_add_track: bool,
    /// The selected clip, when it was recorded and its take can be fitted to.
    fit_clip: Option<InstanceId>,
    /// Why the fit is out of reach, when the project does not enable the extension it needs. A
    /// project made before the fit existed is such a project. The file edit that enables it is
    /// in the agent docs.
    fit_needs: Option<&'static str>,
    undo: Option<String>,
    redo: Option<String>,
}

impl Shown {
    fn of(session: &Session) -> Self {
        let project = session.project();
        Self {
            can_add_track: main_arrangement(project).is_some(),
            fit_clip: recorded_clip(session).map(|(id, _)| id),
            fit_needs: (!extension_is_enabled(project, fit_tempo::EXTENSION))
                .then_some("This project does not include the tempo fit."),
            undo: project.undo_label().map(str::to_string),
            redo: project.redo_label().map(str::to_string),
        }
    }
}

/// The selected clip and its state, when it names a raw take. Fitting the tempo needs a
/// performance to follow, so a clip that was drawn by hand offers nothing.
fn recorded_clip(session: &Session) -> Option<(InstanceId, Clip)> {
    let project = session.project();
    let clip = project.resolve::<Clip>(session.selected_clip()?)?;
    let state = project.state(&clip)?;
    state.take.as_ref()?;
    Some((clip.id().clone(), state.clone()))
}

impl ProjectMenu {
    pub fn new(
        session: Entity<Session>,
        device_name: SharedString,
        cx: &mut Context<Self>,
    ) -> Self {
        let project = session.read(cx).project();
        let name = project.root().file_name().unwrap_or_default();
        let name = name.to_string_lossy().into_owned();
        let shown = Shown::of(session.read(cx));
        let items = entries(&shown, &device_name);
        let menu = cx.new(|cx| {
            DropdownMenu::new(name, items, cx)
                .selected(DEVICE)
                .trigger(Trigger::Ghost)
                .width(280.)
        });
        // Undo and redo say what they would do. A finished edit changes the label and sends no
        // project event, so this follows every notify, and it is cheap: three values to compare,
        // and new items only when one differs. A drag changes none of them until it ends.
        cx.observe(&session, |this, session, cx| {
            let shown = Shown::of(session.read(cx));
            if shown != this.shown {
                let items = entries(&shown, &this.device_name);
                this.menu.update(cx, |menu, cx| menu.set_entries(items, cx));
                this.shown = shown;
            }
        })
        .detach();
        cx.subscribe(&menu, Self::on_picked).detach();
        Self {
            session,
            device_name,
            menu,
            shown,
        }
    }

    pub fn menu(&self) -> &Entity<DropdownMenu> {
        &self.menu
    }

    fn on_picked(&mut self, _: Entity<DropdownMenu>, picked: &MenuPicked, cx: &mut Context<Self>) {
        // An error from any of these shows as the notice of the session.
        self.session
            .update(cx, |session, cx| match picked.0.as_ref() {
                ADD_TRACK => {
                    if let Some(arrangement) = main_arrangement(session.project()) {
                        session.edit(cx, |project| add_track(project, &arrangement));
                    }
                }
                FIT_TEMPO => fit_tempo_to_take(session, cx),
                UNDO => session.undo(cx),
                REDO => session.redo(cx),
                REVEAL => cx.reveal_path(session.project().root()),
                TERMINAL => open_terminal(session.project().root().to_path_buf(), cx),
                // Switching the device is not built yet. The menu only names it.
                _ => {}
            });
    }
}

/// Fits the project tempo to the take of the selected clip, as one undo step. The tempo map
/// and the clip follow in the same group, through the derive of the fit record.
fn fit_tempo_to_take(session: &mut Session, cx: &mut Context<Session>) {
    let Some((_, clip)) = recorded_clip(session) else {
        return;
    };
    // The item is at 40 % in that case and says what to add, so a click cannot get here.
    if !extension_is_enabled(session.project(), fit_tempo::EXTENSION) {
        return;
    }
    session.edit(cx, |project| {
        let mut changes = Changes::new();
        fit_tempo::fit_take(project, &mut changes, &clip)?;
        project.commit(fit_tempo::FIT_LABEL, changes)
    });
}

/// The command that opens a terminal in `folder`. macOS only: the system Terminal, which is
/// there on every Mac. No picker and no setting until someone asks for one.
///
/// A function of its own so a test can read the program and the arguments without a Terminal
/// opening, which CI has no way to close.
fn terminal_command(folder: &Path) -> Command {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-a").arg("Terminal").arg(folder);
    command
}

/// Runs it off the UI thread and puts a failure in the notice of the session. `open` returns
/// as soon as the Terminal has the folder, so waiting for it here costs nothing and leaves no
/// child process behind.
fn open_terminal(folder: PathBuf, cx: &mut Context<Session>) {
    cx.spawn(async move |session, cx| {
        let opened = cx
            .background_spawn(async move { terminal_command(&folder).status() })
            .await;
        let failure = match opened {
            Ok(status) if status.success() => return,
            Ok(status) => format!("The terminal did not open: {status}"),
            Err(error) => format!("The terminal did not open: {error}"),
        };
        // The window is gone when this fails, and there is nobody left to tell.
        if let Some(session) = session.upgrade() {
            session.update(cx, |session, cx| session.report(failure, cx));
        }
    })
    .detach();
}

fn entries(shown: &Shown, device_name: &SharedString) -> Vec<MenuEntry> {
    let command =
        |value: &'static str, label: String| MenuItem::new(value, label).selectable(false);
    let history = |value, verb: &str, label: Option<&str>, shortcut: &'static str| {
        let text = label.map_or(verb.to_string(), |label| format!("{verb} {label}"));
        command(value, text)
            .shortcut(shortcut)
            .disabled(label.is_none())
    };
    vec![
        MenuEntry::Group(
            MenuGroup::new().items([
                command(ADD_TRACK, "Add track".to_string()).disabled(!shown.can_add_track),
                // The fit belongs to the whole project: it rewrites the tempo map every other
                // part follows. So it sits here and not on the clip, and it is offered only for a
                // clip that came from a recording.
                match &shown.fit_needs {
                    // Why, in words, as an instrument picker says it of an offer a project
                    // cannot take. Enabling an extension is a file edit and a reopen, which the
                    // agent docs say.
                    Some(reason) => command(FIT_TEMPO, "Fit tempo to take".to_string())
                        .disabled(true)
                        .description(*reason),
                    None => command(FIT_TEMPO, "Fit tempo to take".to_string())
                        .disabled(shown.fit_clip.is_none()),
                },
            ]),
        ),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().items([
            history(UNDO, "Undo", shown.undo.as_deref(), "mod+z"),
            history(REDO, "Redo", shown.redo.as_deref(), "shift+mod+z"),
        ])),
        MenuEntry::Separator,
        MenuEntry::Group(
            MenuGroup::new()
                .label("Output device")
                .item(MenuItem::new(DEVICE, device_name.clone())),
        ),
        MenuEntry::Separator,
        MenuEntry::Group(MenuGroup::new().items(folder_items())),
    ]
}

/// The last group: the project folder in the Finder, and a terminal in it for a coding agent.
fn folder_items() -> Vec<MenuItem> {
    [
        (REVEAL, "Reveal project folder"),
        (TERMINAL, "Open terminal in project folder"),
    ]
    .map(|(value, label)| MenuItem::new(value, label).selectable(false))
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI has no Terminal to open, so the check is on the command itself.
    #[test]
    fn the_terminal_command_opens_the_system_terminal_at_the_project_folder() {
        let command = terminal_command(Path::new("/Users/someone/Music/my piece"));
        assert_eq!(command.get_program(), "/usr/bin/open");
        let arguments: Vec<&std::ffi::OsStr> = command.get_args().collect();
        assert_eq!(
            arguments,
            ["-a", "Terminal", "/Users/someone/Music/my piece"]
        );
        // One argument, not a shell line, so a space in the folder name needs no quoting.
        assert_eq!(arguments.len(), 3);
    }

    #[test]
    fn the_menu_offers_the_terminal_next_to_the_finder() {
        let items: Vec<(SharedString, SharedString)> = folder_items()
            .iter()
            .map(|item| (item.value.clone(), item.label()))
            .collect();
        assert_eq!(
            items,
            [
                (REVEAL.into(), "Reveal project folder".into()),
                (TERMINAL.into(), "Open terminal in project folder".into()),
            ]
        );
    }
}

impl Render for ProjectMenu {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.menu.clone()
    }
}
