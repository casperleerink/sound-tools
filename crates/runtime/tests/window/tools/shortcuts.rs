//! The Shortcuts section of Settings, and the app's own actions: the composer's key for an
//! action is the key that was pressed for it, kept in a file of the machine, and the keys of the
//! app, its menus and the tools go by it at once.

use std::path::{Path, PathBuf};

use gpui::{KeyUpEvent, Keystroke, TestAppContext};
use reverb::ReverbState;
use serde_json::{Value, json};
use sound_core::Changes;
use sound_ui::{Shortcuts, shortcuts};
use tempfile::TempDir;

use super::parts::page;
use super::{Opened, id};
use crate::support;

/// A page that writes down what its actions did, and the last note it heard. `chosen` and `rest`
/// share `d`.
const PROBE: &str = r#"import { adsr, h, note, number, sine, text, tool } from "./sdk";

const did = (what: string) => ({ update }: { update: (label: string, change: (part: { state: { did: string } }) => void) => void }) =>
  update(what, ({ state }) => { state.did = `${state.did} ${what}`.trim(); });

tool({
  name: "probe",
  title: "Probe",
  when: "A test",
  doc: "Writes down what its actions did.",
  voices: 8,
  state: { did: text(), last: number({ min: 0, max: 127, default: 0 }) },
  sound: () => sine(note.freq).times(adsr(note.gate, 1, 1, 1, 1)).times(0.2),
  actions: {
    go: { title: "Go", keys: ["g"], run: did("go") },
    chosen: { title: "Do the chosen", keys: ["d"], run: () => false },
    rest: { title: "Do the rest", keys: ["d"], run: did("rest") },
  },
  onNotes: ({ update }, event) => {
    if (event.type !== "noteOn") return;
    update("Keep the note", ({ state }) => { state.last = event.pitch; });
  },
  view: () => h("div", { key: "page", style: { width: 200, height: 100 } }, "Probe"),
});
"#;

/// The keymap of the app as it starts: the composer's own keys in a file of the machine.
fn with_file(cx: &mut TestAppContext, contents: Option<&str>) -> (TempDir, PathBuf) {
    let machine = tempfile::tempdir().unwrap();
    let file = machine.path().join("shortcuts.json");
    if let Some(contents) = contents {
        std::fs::write(&file, contents).unwrap();
    }
    cx.update(|cx| cx.set_global(Shortcuts::open(file.clone()).0));
    (machine, file)
}

fn kept(file: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

/// A key as the keymap names it: cmd is ctrl on Windows and Linux.
fn chord(keys: &str) -> String {
    shortcuts::chord(&Keystroke::parse(keys).unwrap())
}

impl Opened<'_> {
    fn open_shortcuts(&mut self) {
        self.keys("secondary-,");
        let section = self.find("settings-shortcuts").unwrap();
        self.click(section);
    }

    fn settings_open(&mut self) -> bool {
        let shell = self.shell.clone();
        self.cx.read(|cx| shell.read(cx).settings().is_some())
    }

    /// A click on the row of the action `id`, which then waits for its key.
    fn wait_for_key(&mut self, id: &str) {
        let row = self.find(&format!("shortcut-{id}")).unwrap();
        self.click(row);
        assert!(self.waits_for_key(id));
    }

    fn waits_for_key(&mut self, id: &str) -> bool {
        self.find(&format!("shortcut-waiting-{id}")).is_some()
    }

    fn keys_of(&mut self, id: &str) -> Vec<String> {
        self.cx
            .read(|cx| cx.global::<Shortcuts>().keys(id).to_vec())
    }

    fn add_reverb(&mut self) {
        self.edit(|project| {
            let mut changes = Changes::new();
            changes.create(id("room"), ReverbState::default());
            project.commit("Add reverb", changes)
        });
    }
}

#[gpui::test]
fn a_key_pressed_for_an_action_of_a_tool_is_its_key_and_does_nothing_else(cx: &mut TestAppContext) {
    let (_machine, file) = with_file(cx, None);
    let mut window = page(cx, "probe", PROBE);
    let on_page = window.until_found("div-page");
    // The computer keys play, so a letter that is pressed for an action could play a note.
    window.keys("secondary-k");
    window.settle();
    window.settle();

    window.open_shortcuts();
    // Two actions of the tool share `d` on purpose, which is no news to the composer.
    assert!(window.find("shortcut-hint-probe.chosen").is_none());
    // A key the composer gives an action that another has is: on the row they changed.
    window.wait_for_key("probe.go");
    window.keys("d");
    assert!(window.find("shortcut-hint-probe.go").is_some());
    assert!(window.find("shortcut-hint-probe.chosen").is_none());

    window.wait_for_key("probe.go");
    window.keys("a");
    assert!(!window.waits_for_key("probe.go"));
    assert_eq!(kept(&file), json!({"probe.go": ["a"]}));
    // It played no note, and the next letter is the computer keys' again.
    window.settle();
    window.after_the_runs();
    assert_eq!(window.record("probe")["last"], Value::Null);
    window.keys("s");
    window.until("a note of the computer keys", |window| {
        window.record("probe")["last"] == 62
    });
    window.after_the_runs();

    // A key of the app does not do what the app has it for while it is pressed for an action,
    // and the row then names the action it shares it with.
    assert_eq!(window.undo_label().as_deref(), Some("Keep the note"));
    window.wait_for_key("probe.go");
    window.keys("secondary-z");
    assert_eq!(window.undo_label().as_deref(), Some("Keep the note"));
    assert_eq!(kept(&file), json!({"probe.go": [chord("secondary-z")]}));
    assert!(window.find("shortcut-hint-probe.go").is_some());

    // Escape leaves the key as it is, and Settings open.
    window.wait_for_key("probe.go");
    window.keys("escape");
    assert!(!window.waits_for_key("probe.go"));
    assert!(window.settings_open());
    assert_eq!(window.keys_of("probe.go"), [chord("secondary-z")]);

    // On a key they share the app's action comes first: the tool's does not run.
    window.keys("escape");
    assert!(!window.settings_open());
    window.click(on_page);
    window.keys("secondary-z");
    assert_eq!(window.undo_label(), None);
    assert_eq!(window.did("probe"), "");

    window.open_shortcuts();
    let remove = window.find("shortcut-remove-probe.go").unwrap();
    window.click(remove);
    assert_eq!(kept(&file), json!({"probe.go": []}));
    assert!(window.find("shortcut-remove-probe.go").is_none());
    let reset = window.find("shortcut-reset-probe.go").unwrap();
    window.click(reset);
    assert_eq!(kept(&file), json!({}));
    assert_eq!(window.keys_of("probe.go"), ["g"]);
    assert!(window.find("shortcut-reset-probe.go").is_none());

    // The keys reach a row: enter on it makes it wait, as a click does.
    window.wait_for_key("probe.go");
    window.keys("escape");
    window.keys("enter");
    let enter = Keystroke::parse("enter").unwrap();
    window.cx.simulate_event(KeyUpEvent { keystroke: enter });
    assert!(window.waits_for_key("probe.go"));
    // Tab moves the focus on and is never taken for a key.
    window.keys("tab");
    assert!(!window.waits_for_key("probe.go"));
    assert_eq!(window.keys_of("probe.go"), ["g"]);

    window.wait_for_key("probe.go");
    window.keys("j");
    assert_eq!(kept(&file), json!({"probe.go": ["j"]}));
    window.keys("escape");
    window.click(on_page);
    window.keys("g j");
    window.until("go on its new key", |window| window.did("probe") == "go");
}

#[gpui::test]
fn an_action_of_the_app_follows_its_key_at_once_and_the_menu_draws_it(cx: &mut TestAppContext) {
    let (_machine, file) = with_file(cx, None);
    let mut window = support::open_new(cx);
    window.add_reverb();

    window.open_shortcuts();
    window.wait_for_key("app.undo");
    window.keys("secondary-u");
    assert_eq!(kept(&file), json!({"app.undo": [chord("secondary-u")]}));
    window.keys("escape");

    // The old key no longer undoes, the new one does.
    window.keys("secondary-z");
    assert_eq!(window.undo_label().as_deref(), Some("Add reverb"));
    window.keys("secondary-u");
    assert_eq!(window.undo_label(), None);
    window.keys("shift-secondary-z");
    assert_eq!(window.undo_label().as_deref(), Some("Add reverb"));

    window.open_shortcuts();
    let reset = window.find("shortcut-reset-app.undo").unwrap();
    window.click(reset);
    window.keys("escape");
    window.keys("secondary-u");
    assert_eq!(window.undo_label().as_deref(), Some("Add reverb"));
    window.keys("secondary-z");
    assert_eq!(window.undo_label(), None);

    // The menu draws the key an action has now, and none for an action with none.
    let menu = window.find("project-menu").unwrap();
    let usual = format!("kbd-{}", shortcuts::drawn("cmd-n"));
    let own = format!("kbd-{}", shortcuts::drawn("cmd-alt-n"));
    let drawn = |window: &mut Opened<'_>| {
        window.click(menu);
        assert!(window.find("menu-new-project").is_some());
        let drawn = [&usual, &own].map(|key| window.find(key).is_some());
        window.keys("escape");
        assert!(window.find("menu-new-project").is_none());
        drawn
    };
    assert_eq!(drawn(&mut window), [true, false]);
    window.set_keys("app.new_project", &["cmd-alt-n"]);
    assert_eq!(drawn(&mut window), [false, true]);
    window.set_keys("app.new_project", &[]);
    assert_eq!(drawn(&mut window), [false, false]);
}

#[gpui::test]
fn shortcuts_that_do_not_read_leave_the_usual_keys_and_are_never_written_over(
    cx: &mut TestAppContext,
) {
    let broken = r#"{ "app.undo": ["cmd-u""#;
    let (_machine, file) = with_file(cx, Some(broken));
    let mut window = support::open_new(cx);
    window.add_reverb();
    window.keys("secondary-z");
    assert_eq!(window.undo_label(), None);

    window.open_shortcuts();
    assert!(window.find("notice-shortcuts-unread").is_some());
    window.wait_for_key("app.undo");
    window.keys("secondary-u");
    assert!(!window.waits_for_key("app.undo"));
    assert_eq!(window.keys_of("app.undo"), ["cmd-z"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
    assert!(window.find("notice-shortcuts-unread").is_some());
}
