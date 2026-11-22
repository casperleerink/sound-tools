//! The project menu: the project's name top left, which opens export, undo and redo, the
//! projects and templates, the project folder, Settings and the app's version. Its keys come
//! from the keymap as it is now, so a key the composer gave an action shows here at once.

use std::io;
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::sync::mpsc::{Receiver, Sender, channel};

use rune::{El, Key, KeyEvent, Tx, icon, row, text};
use rune_kit::{Menu as KitMenu, MenuEntry, MenuItem};

use super::State;
use super::chrome::Act;
use super::keys::app_key_of;
use super::projects::AppAction;
use crate::controls::{Control, Target};

const KEY: &str = "project-menu";
const WIDTH: f32 = 280.0;

/// What the menu remembers while the window is open.
pub(crate) struct Menu {
    pub(crate) open: bool,
    /// The recent projects show beside Open recent.
    recent_open: bool,
    /// Where the command line tool went, until the composer read it.
    pub(crate) installed: Option<(String, String)>,
    /// Why a program the menu started did not run, from the thread that waited for it.
    failed: (Sender<String>, Receiver<String>),
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            open: false,
            recent_open: false,
            installed: None,
            failed: channel(),
        }
    }
}

/// An entry of the menu, by what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    Export,
    ExportSelection,
    Undo,
    Redo,
    NewProject,
    SaveAsTemplate,
    OpenProject,
    /// Shows the recent projects beside it.
    OpenRecent,
    /// The recent project at this place in the list.
    Recent(usize),
    Reveal,
    Terminal,
    InstallTool,
    Log,
    Settings,
}

struct Entry {
    pick: Pick,
    label: String,
    key: Option<String>,
    off: bool,
    /// The folder a recent project is in, to tell two of one name apart.
    description: Option<String>,
}

impl Entry {
    fn new(pick: Pick, label: impl Into<String>) -> Self {
        Self {
            pick,
            label: label.into(),
            key: None,
            off: false,
            description: None,
        }
    }

    fn key(self, key: Option<String>) -> Self {
        Self { key, ..self }
    }

    fn off(self, off: bool) -> Self {
        Self { off, ..self }
    }
}

/// The groups of the menu, in their order; a line goes between two.
fn groups(state: &State) -> Vec<Vec<Entry>> {
    let session = &state.session;
    let rendering = session.rendering().is_some();
    let history = |pick, verb: &str, label: Option<String>, name| {
        let words = label
            .as_ref()
            .map_or(verb.to_string(), |label| format!("{verb} {label}"));
        Entry::new(pick, words)
            .key(app_key_of(state, name))
            .off(label.is_none())
    };
    let project = state.project();
    let mut folder = vec![
        Entry::new(Pick::NewProject, "New project…").key(app_key_of(state, "new_project")),
        Entry::new(Pick::SaveAsTemplate, "Save as template…")
            .key(app_key_of(state, "save_as_template")),
        Entry::new(Pick::OpenProject, "Open project…").key(app_key_of(state, "open_project")),
        Entry::new(Pick::OpenRecent, "Open recent").off(state.shell.recent().is_empty()),
        Entry::new(Pick::Reveal, REVEAL),
        Entry::new(Pick::Terminal, "Open terminal in project folder"),
    ];
    if cfg!(unix) {
        folder.push(Entry::new(Pick::InstallTool, "Install command line tool"));
    }
    if runtime::log_file::path().is_some() {
        folder.push(Entry::new(Pick::Log, "Reveal log file"));
    }
    folder.push(Entry::new(Pick::Settings, "Settings…").key(app_key_of(state, "settings")));
    vec![
        vec![
            Entry::new(Pick::Export, "Export audio…")
                .key(app_key_of(state, "export"))
                .off(rendering),
            Entry::new(Pick::ExportSelection, "Export selection…")
                .off(rendering || session.selected_span().is_none()),
        ],
        vec![
            history(Pick::Undo, "Undo", project.undo_label(), "undo"),
            history(Pick::Redo, "Redo", project.redo_label(), "redo"),
        ],
        folder,
    ]
}

const REVEAL: &str = if cfg!(target_os = "macos") {
    "Reveal project folder"
} else {
    "Show project folder"
};

/// The recent projects: the folder's name, and the folder it is in.
fn recent(state: &State) -> Vec<Entry> {
    let recent = state.shell.recent().into_iter().enumerate();
    recent
        .map(|(index, folder)| {
            let name = folder.file_name().unwrap_or(folder.as_os_str());
            Entry {
                description: folder.parent().map(crate::shell::short_path),
                ..Entry::new(Pick::Recent(index), name.to_string_lossy())
            }
        })
        .collect()
}

fn item(entry: Entry) -> MenuItem {
    let pick = entry.pick;
    let mut item = MenuItem::new(entry.label)
        .disabled(entry.off)
        .on_select_tx(move |s: &mut State, tx: &mut Tx| s.pick(pick, tx));
    if let Some(key) = entry.key {
        item = item.shortcut(key);
    }
    if let Some(description) = entry.description {
        item = item.description(description);
    }
    item
}

/// The key of an entry's row, as the kit's menu makes it from its place.
fn row_key(place: usize) -> String {
    format!("{KEY}-item{place}")
}

/// Where Open recent is among the entries and lines of the menu.
fn recent_place(state: &State) -> usize {
    let mut place = 0;
    for group in groups(state) {
        for entry in group {
            if entry.pick == Pick::OpenRecent {
                return place;
            }
            place += 1;
        }
        place += 1;
    }
    place
}

/// The key of the first recent project in the menu beside Open recent.
fn first_recent(state: &State) -> String {
    format!("{}-sub-item0", row_key(recent_place(state)))
}

/// The project's name, which opens the menu, and the menu while it is open, to go over the
/// window.
pub(super) fn view(state: &State) -> (El, Vec<El>) {
    let theme = &state.theme;
    let kit = &theme.kit;
    let open = state.project_menu.open;
    let trigger = row()
        .key(KEY)
        .role("button")
        .label(state.name.clone())
        .description("Project menu")
        .h(32.0)
        .pl(theme.space(2))
        .pr(theme.space(1))
        .gap(theme.space(1))
        .items_center()
        .radius(kit.radius.md)
        .fill(match open {
            true => kit.c.hover,
            false => rune::Color::TRANSPARENT,
        })
        .focusable(true)
        .on_press(|s: &mut State| {
            s.project_menu.open = !s.project_menu.open;
            s.project_menu.recent_open = false;
        })
        .child(
            text(state.name.clone())
                .font(theme.text("title"))
                .color(theme.color("text"))
                .a11y_hidden(),
        )
        .child(
            icon(kit.glyph.chevron_down, 14.0)
                .color(theme.color("text-muted"))
                .a11y_hidden(),
        );
    let mut menu = KitMenu::new(KEY)
        .trigger(kit.interactive(trigger))
        .min_width(WIDTH)
        .open(open)
        .on_close(|s: &mut State| s.project_menu.close());
    for (index, group) in groups(state).into_iter().enumerate() {
        if index > 0 {
            menu = menu.separator();
        }
        for entry in group {
            let recents = (entry.pick == Pick::OpenRecent).then(|| {
                let open = state.project_menu.recent_open;
                let items = recent(state)
                    .into_iter()
                    .map(|entry| MenuEntry::item(item(entry)));
                (items.collect(), open)
            });
            let mut built = item(entry);
            if let Some((entries, open)) = recents {
                built = built.submenu(entries, open);
            }
            menu = menu.item(built);
        }
    }
    let menu = (menu.separator()).group_label(format!("Sound Tools {}", runtime::VERSION));
    let mut layer = Vec::new();
    let trigger = menu.build(kit, &mut layer);
    let layer = layer.into_iter().map(|layer| layer.on_key(submenu_key));
    (trigger, layer.collect())
}

/// Right on Open recent shows the recent projects; Left or Escape in them goes back.
fn submenu_key(s: &mut State, key: &KeyEvent, tx: &mut Tx) -> bool {
    let place = recent_place(s);
    let on_recent = tx.focused() == Some(row_key(place).as_str());
    let in_recent = s.project_menu.recent_open && !on_recent;
    match (&key.key, on_recent, in_recent) {
        (Key::Right, true, _) if !s.shell.recent().is_empty() => {
            s.project_menu.recent_open = true;
            tx.focus_when_shown(first_recent(s), true);
        }
        (Key::Left | Key::Escape, _, true) => {
            s.project_menu.recent_open = false;
            tx.focus(row_key(place), true);
        }
        _ => return false,
    }
    true
}

/// What gets the keys back for the element `key`: the name for any part of the menu, which is
/// gone once it closes.
pub(super) fn outside(key: &str) -> String {
    match key.starts_with(KEY) {
        true => KEY.to_string(),
        false => key.to_string(),
    }
}

impl Menu {
    fn close(&mut self) {
        self.open = false;
        self.recent_open = false;
    }
}

impl State {
    /// Runs the entry `pick`: the menu closes first, but for Open recent, which shows the
    /// recent projects beside it.
    pub(crate) fn pick(&mut self, pick: Pick, tx: &mut Tx) {
        if pick == Pick::OpenRecent {
            self.project_menu.recent_open = !self.project_menu.recent_open;
            if self.project_menu.recent_open {
                tx.focus_when_shown(first_recent(self), true);
            }
            return;
        }
        self.project_menu.close();
        let folder = self.project().root().to_path_buf();
        match pick {
            Pick::Export => self.app_action(AppAction::Export, tx),
            Pick::ExportSelection => self.app_action(AppAction::ExportSelection, tx),
            Pick::Undo => drop(self.undo()),
            Pick::Redo => drop(self.redo()),
            Pick::NewProject => self.app_action(AppAction::NewProject, tx),
            Pick::SaveAsTemplate => self.app_action(AppAction::SaveAsTemplate, tx),
            Pick::OpenProject => self.app_action(AppAction::OpenProject, tx),
            Pick::OpenRecent => {}
            Pick::Recent(index) => {
                let opened = match self.shell.recent().get(index) {
                    Some(folder) => self.shell.open(folder, tx),
                    None => Ok(()),
                };
                if let Err(error) = opened {
                    self.session.report(format!("{error:#}"));
                }
            }
            Pick::Reveal => {
                self.run_off_thread("The folder did not show", reveal_commands(&folder))
            }
            Pick::Terminal => {
                self.run_off_thread("The terminal did not open", terminal_commands(&folder))
            }
            Pick::InstallTool => self.install_tool(),
            Pick::Log => {
                if let Some(log) = runtime::log_file::path() {
                    self.run_off_thread("The log file did not show", reveal_commands(log));
                }
            }
            Pick::Settings => self.app_action(AppAction::Settings, tx),
        }
        self.handled();
    }

    /// The first of `commands` whose program is there, on a thread: each returns as soon as
    /// its program has the folder. A failure is a notice, which starts with `failed`. A run
    /// without a window starts no program.
    fn run_off_thread(&mut self, failed: &'static str, commands: Vec<Command>) {
        if self.headless() {
            return;
        }
        let (tell, waker) = (self.project_menu.failed.0.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let why = match run_first(commands) {
                Ok(status) if status.success() => return,
                Ok(status) => format!("{failed}: {status}"),
                Err(error) => format!("{failed}: {error}"),
            };
            if tell.send(why).is_ok()
                && let Some(wake) = waker.lock().ok().as_deref().and_then(Option::as_ref)
            {
                wake();
            }
        });
    }

    /// The notices of programs the menu started that failed. Whether there were any.
    pub(crate) fn menu_failures(&mut self) -> bool {
        let failed: Vec<String> = self.project_menu.failed.1.try_iter().collect();
        let any = !failed.is_empty();
        for why in failed {
            self.session.report(why);
        }
        any
    }

    #[cfg(unix)]
    fn install_tool(&mut self) {
        match runtime::app::install_command_line_tool() {
            Ok(installed) => self.project_menu.installed = Some(installed_message(&installed)),
            Err(error) => self.session.report(format!("{error:#}")),
        }
    }

    #[cfg(not(unix))]
    fn install_tool(&mut self) {}
}

/// The dialog that says where the command line tool went, until OK.
pub(super) fn installed_dialog(state: &State) -> Vec<El> {
    let Some((title, words)) = &state.project_menu.installed else {
        return Vec::new();
    };
    let theme = &state.theme;
    let ok = crate::shell::dialog::button(
        theme,
        "installed-ok",
        "OK",
        rune_kit::ButtonVariant::Contrast,
        true,
        |s: &mut State, _: &mut Tx| s.project_menu.installed = None,
    );
    crate::shell::dialog::Dialog {
        key: "installed",
        title: title.clone(),
        description: None,
        body: vec![
            text(words.clone())
                .font(theme.text("body"))
                .color(theme.color("text-secondary"))
                .wrap(),
        ],
        note: None,
        buttons: vec![ok],
        close: |s: &mut State| s.project_menu.installed = None,
    }
    .build(theme)
}

/// What the dialog says once the tool is installed: when it went to `~/.local/bin`, a terminal
/// may not find it yet.
#[cfg(unix)]
fn installed_message(installed: &runtime::app::Installed) -> (String, String) {
    let tool = runtime::app::TOOL_NAME;
    let mut words = format!(
        "{} runs this app. An agent in a project folder runs “{tool} . --inspect” to read the whole piece.",
        installed.link.display()
    );
    if !installed.on_default_path {
        // The shell a new account starts with: zsh on a Mac, bash on most Linux systems.
        let startup = if cfg!(target_os = "macos") {
            "~/.zshrc"
        } else {
            "~/.bashrc"
        };
        words.push_str(&format!(
            "\n\nIf a terminal says “command not found”, add ~/.local/bin to your PATH: add the line export PATH=\"$HOME/.local/bin:$PATH\" to {startup} and open a new terminal."
        ));
    }
    (format!("Installed {tool}"), words)
}

/// The commands that show `path` selected in the system's file manager. Linux has no common
/// way to select a file, so it opens the folder it is in.
#[cfg(target_os = "macos")]
fn reveal_commands(path: &Path) -> Vec<Command> {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-R").arg(path);
    vec![command]
}

#[cfg(windows)]
fn reveal_commands(path: &Path) -> Vec<Command> {
    let mut select = std::ffi::OsString::from("/select,");
    select.push(path);
    let mut command = Command::new("explorer.exe");
    command.arg(select);
    vec![command]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn reveal_commands(path: &Path) -> Vec<Command> {
    let mut command = Command::new("xdg-open");
    command.arg(path.parent().unwrap_or(path));
    vec![command]
}

/// macOS: the system Terminal, which every Mac has.
#[cfg(target_os = "macos")]
fn terminal_commands(folder: &Path) -> Vec<Command> {
    let mut command = Command::new("/usr/bin/open");
    command.arg("-a").arg("Terminal").arg(folder);
    vec![command]
}

/// Linux: the terminal `$TERMINAL` names, else `x-terminal-emulator`, which Debian and Ubuntu
/// point at the system's. The shell starts it without waiting, as `open` does, and fails only
/// when there is no such program.
#[cfg(all(unix, not(target_os = "macos")))]
fn terminal_commands(folder: &Path) -> Vec<Command> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(r#"t="${TERMINAL:-x-terminal-emulator}"; command -v "$t" >/dev/null && { "$t" >/dev/null 2>&1 & }"#)
        .current_dir(folder);
    vec![command]
}

/// Windows: Windows Terminal, else a Command Prompt, which `start` gives a window of its own.
#[cfg(windows)]
fn terminal_commands(folder: &Path) -> Vec<Command> {
    let mut windows_terminal = Command::new("wt.exe");
    windows_terminal.arg("-d").arg(folder);
    let mut command_prompt = Command::new("cmd.exe");
    command_prompt.args(["/c", "start"]).current_dir(folder);
    vec![windows_terminal, command_prompt]
}

/// Waits for the first of `commands` whose program is there. One that is there and fails is
/// the answer: the next one would hide why.
fn run_first(commands: Vec<Command>) -> io::Result<ExitStatus> {
    let mut missing = io::Error::from(io::ErrorKind::NotFound);
    for mut command in commands {
        match command.status() {
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing = error,
            ran => return ran,
        }
    }
    Err(missing)
}

/// The trigger, and the entries while the menu is open, for `--tree` and `--act`. An entry
/// that is off has no action.
pub(super) fn controls(state: &State) -> Vec<Control> {
    let control = |key: String, label: String, role: &str, act: Act, off: bool| Control {
        key,
        label,
        role: Some(role.into()),
        value: None,
        checked: None,
        expanded: None,
        actions: match off {
            true => Vec::new(),
            false => vec!["press".into()],
        },
        place: None,
        target: Target::Chrome(act),
        menu: None,
        drag: None,
    };
    let trigger = Control {
        expanded: Some(state.project_menu.open),
        ..control(
            KEY.into(),
            state.name.clone(),
            "button",
            Act::Menu(None),
            false,
        )
    };
    let mut controls = vec![trigger];
    if !state.project_menu.open {
        return controls;
    }
    let mut place = 0;
    for group in groups(state) {
        for entry in group {
            let opens = entry.pick == Pick::OpenRecent;
            controls.push(Control {
                expanded: opens.then_some(state.project_menu.recent_open),
                ..control(
                    row_key(place),
                    entry.label,
                    "menuitem",
                    Act::Menu(Some(entry.pick)),
                    entry.off,
                )
            });
            place += 1;
        }
        place += 1;
    }
    if state.project_menu.recent_open {
        let sub = row_key(recent_place(state));
        controls.extend(recent(state).into_iter().enumerate().map(|(index, entry)| {
            let key = format!("{sub}-sub-item{index}");
            control(
                key,
                entry.label,
                "menuitem",
                Act::Menu(Some(entry.pick)),
                false,
            )
        }));
    }
    controls
}

/// Runs a control of the menu for `--act`: the trigger opens or closes it.
pub(super) fn act(state: &mut State, pick: Option<Pick>) {
    match pick {
        Some(pick) => state.pick(pick, &mut Tx::default()),
        None => {
            state.project_menu.open = !state.project_menu.open;
            state.project_menu.recent_open = false;
        }
    }
}

#[cfg(test)]
pub(super) fn picks(state: &State) -> Vec<(Pick, String, Option<String>)> {
    let entries = groups(state).into_iter().flatten();
    entries
        .map(|entry| (entry.pick, entry.label, entry.key))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use super::*;

    /// CI has no Terminal or Finder to open, so the check is on the command itself: the folder is
    /// one argument, not a shell line, so a space in it needs no quoting.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_terminal_and_the_finder_open_at_the_project_folder() {
        let folder = Path::new("/Users/someone/Music/my piece");
        let said = |commands: Vec<Command>| {
            let [command] = <[Command; 1]>::try_from(commands).unwrap();
            let arguments = command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned());
            (
                command.get_program().to_string_lossy().into_owned(),
                arguments.collect::<Vec<_>>(),
            )
        };
        let open = "/usr/bin/open".to_string();
        let piece = folder.display().to_string();
        assert_eq!(
            said(terminal_commands(folder)),
            (
                open.clone(),
                vec!["-a".into(), "Terminal".into(), piece.clone()]
            )
        );
        assert_eq!(
            said(reveal_commands(folder)),
            (open, vec!["-R".into(), piece])
        );
    }

    /// `true` stands for a terminal: it starts, in the folder, and the command succeeds. A name
    /// that is no program fails, which is what the notice says.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_terminal_command_starts_the_terminal_of_the_system_in_the_project_folder() {
        let folder = tempfile::tempdir().unwrap();
        let terminal = |name: &str| {
            let [mut command] = <[Command; 1]>::try_from(terminal_commands(folder.path())).unwrap();
            assert_eq!(command.get_current_dir(), Some(folder.path()));
            command.env("TERMINAL", name).status().unwrap()
        };
        assert!(terminal("true").success());
        assert!(!terminal("no-such-terminal-program").success());
    }

    #[cfg(windows)]
    #[test]
    fn the_terminal_command_opens_windows_terminal_or_else_a_command_prompt_at_the_project_folder()
    {
        let folder = Path::new(r"C:\Users\someone\Music\my piece");
        let programs: Vec<_> = (terminal_commands(folder).iter())
            .map(|command| command.get_program().to_owned())
            .collect();
        assert_eq!(programs, ["wt.exe", "cmd.exe"]);
    }

    /// This test program stands for a terminal: `--list` runs and succeeds, an unknown flag
    /// fails.
    #[test]
    fn a_missing_program_falls_back_to_the_next_and_a_failing_one_does_not() {
        let this = std::env::current_exe().unwrap();
        let command = |program: &Path, argument: &str| {
            let mut command = Command::new(program);
            command
                .arg(argument)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            command
        };
        let missing = Path::new("no-such-terminal-program");
        let ran = run_first(vec![command(missing, "--list"), command(&this, "--list")]);
        assert!(ran.unwrap().success());
        let ran = run_first(vec![
            command(&this, "--no-such-flag"),
            command(&this, "--list"),
        ]);
        assert!(!ran.unwrap().success());
        let ran = run_first(vec![command(missing, "--list")]);
        assert_eq!(ran.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn the_install_message_says_how_to_reach_a_folder_off_the_path() {
        let (title, words) = installed_message(&runtime::app::Installed {
            link: "/usr/local/bin/sound-tools".into(),
            on_default_path: true,
        });
        assert_eq!(title, "Installed sound-tools");
        assert!(words.starts_with("/usr/local/bin/sound-tools runs this app."));
        assert!(!words.contains("PATH"));
        let (_, words) = installed_message(&runtime::app::Installed {
            link: "/Users/someone/.local/bin/sound-tools".into(),
            on_default_path: false,
        });
        assert!(words.contains("export PATH=\"$HOME/.local/bin:$PATH\""));
    }
}
