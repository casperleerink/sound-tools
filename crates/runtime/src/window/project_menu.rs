//! The project menu: the project name top-left as a quiet menu. Fit the tempo to a take,
//! export the project or the selected clips as a WAV, undo and redo with what they would do,
//! the output device by name, another project, the project folder in the Finder or in a
//! terminal, and the command line tool. The
//! terminal is where the composer starts a coding agent on the project, and the tool is what
//! that agent runs to read the whole piece.

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{
    App, Context, Entity, IntoElement, PromptLevel, Render, SharedString, Window, prelude::*,
};
use sound_core::{Changes, InstanceId};
use sound_notes::Clip;
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::{Session, extension_is_enabled};

use crate::app::{self, Installed};
use crate::main_arrangement;

const FIT_TEMPO: &str = "fit-tempo";
const EXPORT: &str = "export";
const EXPORT_SELECTION: &str = "export-selection";
const UNDO: &str = "undo";
const REDO: &str = "redo";
const DEVICE: &str = "device";
const OPEN_PROJECT: &str = "open-project";
const REVEAL: &str = "reveal";
const TERMINAL: &str = "terminal";
const INSTALL_TOOL: &str = "install-tool";

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
    has_arrangement: bool,
    /// The selected clip, when it was recorded and its take can be fitted to.
    fit_clip: Option<InstanceId>,
    /// Why the fit is out of reach, when the project does not enable the extension it needs. A
    /// project made before the fit existed is such a project. The file edit that enables it is
    /// in the agent docs.
    fit_needs: Option<&'static str>,
    has_selection: bool,
    undo: Option<String>,
    redo: Option<String>,
}

impl Shown {
    fn of(session: &Session) -> Self {
        let project = session.project();
        Self {
            has_arrangement: main_arrangement(project).is_some(),
            fit_clip: recorded_clip(session).map(|(id, _)| id),
            fit_needs: (!extension_is_enabled(project, fit_tempo::EXTENSION))
                .then_some("This project does not include the tempo fit."),
            has_selection: !session.selected_clips().is_empty(),
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let project = session.read(cx).project();
        let name = project.root().file_name().unwrap_or_default();
        let name = name.to_string_lossy().into_owned();
        let shown = Shown::of(session.read(cx));
        let items = entries(&shown, &device_name);
        let menu = cx.new(|cx| {
            DropdownMenu::new(name, items, cx)
                .debug_name("project-menu")
                .selected(DEVICE)
                .trigger(Trigger::Subtle)
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
        cx.subscribe_in(&menu, window, Self::on_picked).detach();
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

    fn on_picked(
        &mut self,
        _: &Entity<DropdownMenu>,
        picked: &MenuPicked,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match picked.0.as_ref() {
            INSTALL_TOOL => return install_command_line_tool(self.session.clone(), window, cx),
            EXPORT => return export_audio(self.session.clone(), false, window, cx),
            EXPORT_SELECTION => return export_audio(self.session.clone(), true, window, cx),
            _ => {}
        }
        // An error from any of these shows as the notice of the session.
        self.session
            .update(cx, |session, cx| match picked.0.as_ref() {
                FIT_TEMPO => fit_tempo_to_take(session, cx),
                UNDO => session.undo(cx),
                REDO => session.redo(cx),
                OPEN_PROJECT => open_another_project(cx),
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

/// What the export runs after `<folder> --render <wav>`: nothing for the whole project, which
/// the render ends by itself, or the time the selected clips cover. `None` when there is
/// nothing to export.
fn export_span(session: &Session, selection: bool) -> Option<Vec<String>> {
    let project = session.project();
    if !selection {
        return crate::project_end(project).map(|_| Vec::new());
    }
    let (from, to) = crate::clips_span(project, session.selected_clips())?;
    Some(vec![
        "--from".into(),
        from.0.to_string(),
        "--to".into(),
        to.0.to_string(),
    ])
}

/// Asks where the WAV goes and renders it in a process of its own: this program with
/// `--render`, which reads the project from its folder as an agent's render does. The window
/// keeps playing meanwhile, and a plugin that crashes in the render costs the render only.
fn export_audio(
    session: Entity<Session>,
    selection: bool,
    window: &mut Window,
    cx: &mut Context<ProjectMenu>,
) {
    let (folder, span) = session.read_with(cx, |session, _| {
        let folder = session.project().root().to_path_buf();
        (folder, export_span(session, selection))
    });
    let Some(span) = span else {
        let message = "The project has no clips, so there is nothing to export.";
        session.update(cx, |session, cx| session.report(message, cx));
        return;
    };
    // Next to the project folder and not in it: an export is not part of the project, and in
    // the folder it would end up in git and in front of the agent.
    let directory = folder.parent().unwrap_or(&folder).to_path_buf();
    let name = folder.file_name().unwrap_or_default().to_string_lossy();
    let picked = cx.prompt_for_new_path(&directory, Some(&format!("{name}.wav")));
    cx.spawn_in(window, async move |_, cx| {
        let wav = match picked.await {
            Ok(Ok(Some(wav))) => wav,
            Ok(Ok(None)) | Err(_) => return,
            Ok(Err(error)) => {
                let message = format!("{error:#}");
                session.update(cx, |session, cx| session.report(message, cx));
                return;
            }
        };
        let rendered = cx
            .background_spawn({
                let wav = wav.clone();
                async move { render_in_child(&folder, &wav, &span) }
            })
            .await;
        let errors = match rendered {
            Ok(errors) => errors,
            Err(error) => {
                let message = format!("The export failed: {error:#}");
                session.update(cx, |session, cx| session.report(message, cx));
                return;
            }
        };
        let file = wav.file_name().unwrap_or_default().to_string_lossy();
        let answer = cx.update(|window, cx| {
            let message = format!("Exported {file}");
            // A plugin that did not load or answer in the render is missing from the file.
            let (level, detail) = if errors.is_empty() {
                (PromptLevel::Info, None)
            } else {
                let detail = format!("It may be incomplete:\n{}", errors.join("\n"));
                (PromptLevel::Warning, Some(detail))
            };
            let reveal = if cfg!(target_os = "macos") {
                "Show in Finder"
            } else {
                "Show in folder"
            };
            let buttons = [reveal, "OK"];
            window.prompt(level, &message, detail.as_deref(), &buttons, cx)
        });
        if let Ok(answer) = answer
            && let Ok(0) = answer.await
        {
            cx.update(|_, cx| cx.reveal_path(&wav)).ok();
        }
    })
    .detach();
}

/// Runs `--render` and waits for it. Gives the errors it printed on the way, such as a plugin
/// that did not load. When it failed, what it printed last on stderr is the error.
fn render_in_child(folder: &Path, wav: &Path, span: &[String]) -> anyhow::Result<Vec<String>> {
    let program = std::env::current_exe()?;
    let output = Command::new(program)
        .arg(folder)
        .arg("--render")
        .arg(wav)
        .args(span)
        .output()?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let errors = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("error: "));
        return Ok(errors.map(str::to_string).collect());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr.lines().rfind(|line| !line.trim().is_empty());
    anyhow::bail!(
        "{}",
        last.unwrap_or("the render stopped")
            .trim_start_matches("Error: ")
    )
}

/// The command that opens a terminal in `folder`. macOS only: the system Terminal, which is
/// there on every Mac. No picker and no setting until someone asks for one.
///
/// A function of its own so a test can read the program and the arguments without a Terminal
/// opening, which CI has no way to close.
#[cfg(target_os = "macos")]
fn terminal_command(folder: &Path) -> Command {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-a").arg("Terminal").arg(folder);
    command
}

/// Linux: the terminal `$TERMINAL` names, else `x-terminal-emulator`, which Debian and Ubuntu
/// point at the one the system has, started in `folder`. The shell starts it without waiting,
/// as `open` does, and fails only when there is no such program.
#[cfg(not(target_os = "macos"))]
fn terminal_command(folder: &Path) -> Command {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(r#"t="${TERMINAL:-x-terminal-emulator}"; command -v "$t" >/dev/null && { "$t" >/dev/null 2>&1 & }"#)
        .current_dir(folder);
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

/// Picks another project folder in the macOS panel and opens it. The app starts again on it:
/// this process quits the way cmd-q quits, which saves the state of every plugin and frees the
/// project, and the new one opens the folder as the last project.
fn open_another_project(cx: &mut Context<Session>) {
    let picked = cx.prompt_for_paths(super::start::folder_prompt());
    cx.spawn(async move |session, cx| {
        let folder = match super::start::picked_folder(picked.await.ok()) {
            Ok(Some(folder)) => folder,
            Ok(None) => return,
            Err(error) => {
                report(&session, error, cx);
                return;
            }
        };
        let remembered = cx
            .background_spawn(async move {
                app::check_project_folder(&folder)?;
                app::remember_project(&folder)
            })
            .await;
        match remembered {
            Ok(()) => cx.update(|cx| start_again(cx)),
            Err(error) => report(&session, format!("{error:#}"), cx),
        }
    })
    .detach();
}

/// Quits, and starts this program again once the project is closed. The project goes with the
/// window, before the last step of a quit, which is where the new process starts.
fn start_again(cx: &mut App) {
    cx.on_app_quit(|_| async {
        if let Err(error) = app::start_again() {
            eprintln!("error: {error:#}");
        }
    })
    .detach();
    cx.quit();
}

fn report(session: &gpui::WeakEntity<Session>, message: String, cx: &mut gpui::AsyncApp) {
    // The window is gone when this fails, and there is nobody left to tell.
    if let Some(session) = session.upgrade() {
        session.update(cx, |session, cx| session.report(message, cx));
    }
}

/// Links `sound-tools` to this program, off the UI thread, and says where in a dialog: when
/// it went to `~/.local/bin`, a terminal may not find it yet.
fn install_command_line_tool(
    session: Entity<Session>,
    window: &mut Window,
    cx: &mut Context<ProjectMenu>,
) {
    cx.spawn_in(window, async move |_, cx| {
        let installed = cx
            .background_spawn(async move { app::install_command_line_tool() })
            .await;
        match installed {
            Ok(installed) => {
                let (message, detail) = installed_message(&installed);
                let answer = cx.update(|window, cx| {
                    window.prompt(PromptLevel::Info, &message, Some(&detail), &["OK"], cx)
                });
                if let Ok(answer) = answer {
                    // Only one answer, so which one does not matter.
                    match answer.await {
                        Ok(_) | Err(_) => {}
                    }
                }
            }
            Err(error) => {
                let message = format!("{error:#}");
                session.update(cx, |session, cx| session.report(message, cx));
            }
        }
    })
    .detach();
}

fn installed_message(installed: &Installed) -> (String, String) {
    let link = installed.link.display();
    let message = format!("Installed {}", app::TOOL_NAME);
    let mut detail = format!(
        "{link} runs this app. An agent in a project folder runs “{} . --inspect” to read the whole piece.",
        app::TOOL_NAME
    );
    if !installed.on_default_path {
        // The shell a new account starts with: zsh on a Mac, bash on most Linux systems.
        let startup = if cfg!(target_os = "macos") {
            "~/.zshrc"
        } else {
            "~/.bashrc"
        };
        detail.push_str(&format!(
            "\n\nIf a terminal says “command not found”, add ~/.local/bin to your PATH: add the line export PATH=\"$HOME/.local/bin:$PATH\" to {startup} and open a new terminal."
        ));
    }
    (message, detail)
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
        MenuEntry::Group(
            MenuGroup::new().items([
                command(EXPORT, "Export audio…".to_string()).disabled(!shown.has_arrangement),
                command(EXPORT_SELECTION, "Export selection…".to_string())
                    .disabled(!shown.has_selection),
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

/// The last group: another project, the project folder in the Finder, a terminal in it for a
/// coding agent, and the command that agent runs.
fn folder_items() -> Vec<MenuItem> {
    [
        (OPEN_PROJECT, "Open project…"),
        (REVEAL, "Reveal project folder"),
        (TERMINAL, "Open terminal in project folder"),
        (INSTALL_TOOL, "Install command line tool"),
    ]
    .map(|(value, label)| MenuItem::new(value, label).selectable(false))
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI has no Terminal to open, so the check is on the command itself.
    #[cfg(target_os = "macos")]
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

    /// `true` stands for a terminal: it starts, in the folder, and the command succeeds. A
    /// name that is no program fails, which is what the notice reports.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_terminal_command_starts_the_terminal_of_the_system_in_the_project_folder() {
        let folder = tempfile::tempdir().expect("a folder");
        let mut command = terminal_command(folder.path());
        assert_eq!(command.get_current_dir(), Some(folder.path()));
        let started = command.env("TERMINAL", "true").status().expect("sh runs");
        assert!(started.success());
        let missing = terminal_command(folder.path())
            .env("TERMINAL", "no-such-terminal-program")
            .status()
            .expect("sh runs");
        assert!(!missing.success());
    }

    #[test]
    fn the_install_message_says_how_to_reach_a_folder_off_the_path() {
        let (message, detail) = installed_message(&Installed {
            link: "/usr/local/bin/sound-tools".into(),
            on_default_path: true,
        });
        assert_eq!(message, "Installed sound-tools");
        assert!(detail.starts_with("/usr/local/bin/sound-tools runs this app."));
        assert!(!detail.contains("PATH"));

        let (_, detail) = installed_message(&Installed {
            link: "/Users/someone/.local/bin/sound-tools".into(),
            on_default_path: false,
        });
        assert!(detail.contains("export PATH=\"$HOME/.local/bin:$PATH\""));
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
                (OPEN_PROJECT.into(), "Open project…".into()),
                (REVEAL.into(), "Reveal project folder".into()),
                (TERMINAL.into(), "Open terminal in project folder".into()),
                (INSTALL_TOOL.into(), "Install command line tool".into()),
            ]
        );
    }
}

impl Render for ProjectMenu {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.menu.clone()
    }
}
