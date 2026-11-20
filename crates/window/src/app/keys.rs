//! The keys of the window, matched in one place: a focused text field first, which no action
//! hears; then the left panel's; then undo and redo; then the tool of the focused card, the
//! pages around it up to the main view, and the app. Outside the views the main view's tool hears only its actions that run `everywhere` and
//! its `globalKeys`. A key does an action, or a tool's `onKey` holds it until it comes up there.

use std::collections::{BTreeMap, HashMap};

use rune::{Key, KeyEvent, Mods, Tx};
use sound_core::InstanceId;
use sound_typescript::shortcuts::{self, Group, Held, Shortcuts};

use crate::controls::{self, Target};

use super::State;
use super::projects::AppAction;

/// The app's own group in the keymap.
const APP: &str = "app";
/// The app's actions a tool's key never takes the place of.
const BEFORE_TOOLS: [&str; 3] = ["undo", "redo", "undo-keeping-version"];

/// The keymap, and the keys a tool's `onKey` holds.
pub(crate) struct Keys {
    pub(crate) keymap: Shortcuts,
    /// Each key that went down to an `onKey`, by its name without what was held: its up goes
    /// to the same instance whatever is held by then.
    held: HashMap<String, InstanceId>,
}

impl Keys {
    /// The keymap with the composer's own keys of `file`; without one they last as long as the
    /// window. Why the file did not read, if it did not.
    pub(crate) fn open(file: Option<std::path::PathBuf>) -> (Self, Option<String>) {
        let (mut keymap, unread) = match file {
            Some(file) => Shortcuts::open(file),
            None => (Shortcuts::default(), None),
        };
        keymap.set_group(app_group());
        let keys = Self {
            keymap,
            held: HashMap::new(),
        };
        (keys, unread.map(|error| error.to_string()))
    }
}

fn app_group() -> Group {
    let action = |name: &str, title: &str, key: &str| shortcuts::Action {
        name: name.to_string(),
        title: title.to_string(),
        keys: vec![key.to_string()],
    };
    Group {
        name: APP.to_string(),
        title: "Sound Tools".to_string(),
        actions: vec![
            action("undo", "Undo", "cmd-z"),
            action("redo", "Redo", "cmd-shift-z"),
            action(
                "undo-keeping-version",
                "Undo, keeping what it takes back as a version",
                "cmd-alt-z",
            ),
            action("play", "Play or pause", "space"),
            action(
                "computer-keys",
                "Play notes with the computer keys",
                "cmd-k",
            ),
            action("new-project", "New project", "cmd-n"),
            action("open-project", "Open project", "cmd-o"),
        ],
    }
}

/// A key as the keymap and the SDK name it: `a`, `space`, `left`, `?`. `None` for one they do
/// not name, such as a modifier alone.
fn name_of(key: &Key) -> Option<String> {
    Some(match key {
        Key::Text(text) if text == " " => "space".to_string(),
        Key::Text(text) if text.chars().count() == 1 => text.to_lowercase(),
        Key::Command(' ') => "space".to_string(),
        Key::Command(letter) => letter.to_ascii_lowercase().to_string(),
        Key::Enter => "enter".to_string(),
        Key::Escape => "escape".to_string(),
        Key::Backspace => "backspace".to_string(),
        Key::Delete => "delete".to_string(),
        Key::Tab => "tab".to_string(),
        Key::Up => "up".to_string(),
        Key::Down => "down".to_string(),
        Key::Left => "left".to_string(),
        Key::Right => "right".to_string(),
        Key::Home => "home".to_string(),
        Key::End => "end".to_string(),
        Key::PageUp => "pageup".to_string(),
        Key::PageDown => "pagedown".to_string(),
        Key::Function(number) => format!("f{number}"),
        _ => return None,
    })
}

/// What is held with a key, as the keymap reads it: a key that types another character with
/// shift is named by that character, with no shift.
fn held_of(name: &str, mods: Mods) -> Held {
    let symbol = name.chars().count() == 1 && !name.chars().all(char::is_alphanumeric);
    Held {
        platform: mods.logo || (cfg!(target_os = "macos") && mods.cmd),
        control: mods.ctrl,
        alt: mods.alt,
        shift: mods.shift && !symbol,
    }
}

/// Who hears a key, nearest first: an instance, and whether only what runs everywhere.
type Chain = Vec<(InstanceId, bool)>;

impl State {
    /// The instances that hear a key while `focused` has the focus: none in a text field; the
    /// view of the focused element and those around it up to the main view; outside the views
    /// the main view, with only what runs everywhere.
    fn chain(&self, focused: Option<&str>) -> Chain {
        let Some(main) = self.main().cloned() else {
            return Vec::new();
        };
        let typing = self.typing_now();
        let Some(focused) = focused.filter(|_| !typing) else {
            return match typing {
                true => Vec::new(),
                false => vec![(main, false)],
            };
        };
        let found = controls::collect(self)
            .controls
            .into_iter()
            .find_map(|control| match (control.key == focused, control.target) {
                (true, Target::View(press)) => Some(press.instance),
                _ => None,
            });
        let Some(instance) = found else {
            return vec![(main, true)];
        };
        let around = std::iter::once(instance.clone()).chain(instance.ancestors());
        let mut chain: Chain = around
            .take_while(|id| id.is_inside(&main) || *id == main)
            .map(|id| (id, false))
            .collect();
        if chain.is_empty() {
            chain.push((main, true));
        }
        chain
    }

    /// Runs the actions of the tool of `instance` that `chord` does, if any: as one step of
    /// undo, in their order until one does it. Whether it had any.
    fn act_on_key(
        &mut self,
        instance: &InstanceId,
        chord: &str,
        everywhere: bool,
        repeat: bool,
    ) -> bool {
        let Some(tool) = self.project().tool_of(instance).map(str::to_string) else {
            return false;
        };
        let Some(tools) = &self.tools else {
            return false;
        };
        let mut actions: Vec<String> = (self.keys.keymap.bound(&tool, chord).into_iter())
            .map(str::to_string)
            .collect();
        actions.retain(|action| !everywhere || tools.acts_everywhere(&tool, action));
        if actions.is_empty() {
            return false;
        }
        // A key held down does it once.
        if !repeat {
            self.run_actions(instance, &actions);
        }
        true
    }

    /// Runs `actions` of the tool of `instance`, in order until one does it, as one step of undo.
    pub(crate) fn run_actions(&mut self, instance: &InstanceId, actions: &[String]) -> bool {
        let clock = self.session.clock();
        let Some(tools) = &mut self.tools else {
            return false;
        };
        let sent = tools.action(self.session.project(), instance, actions, clock);
        self.handled();
        sent
    }

    /// The key `name` goes down or up to the `onKey` of the tool of `instance`.
    fn key_to(&mut self, instance: &InstanceId, (name, down): (&str, bool), held: Held) {
        let clock = self.session.clock();
        if let Some(tools) = &mut self.tools {
            tools.key(self.session.project(), instance, (name, down), held, clock);
        }
        self.handled();
    }

    /// Every key an `onKey` holds comes up: cmd went down, under which macOS sends no key up, or
    /// the window lost the keys.
    pub(crate) fn let_go_of_keys(&mut self) {
        for (name, instance) in std::mem::take(&mut self.keys.held) {
            self.key_to(&instance, (&name, false), Held::default());
        }
    }

    /// The app's action `action` of the keymap.
    fn app_key(&mut self, action: &str, tx: &mut Tx) {
        let done = match action {
            "undo" => self.undo(),
            "redo" => self.redo(),
            "undo-keeping-version" => self.undo_keeping_version(),
            "play" => {
                self.play_or_pause();
                Ok(())
            }
            "computer-keys" => super::transport::Act::Keys.run(self, None),
            "new-project" => {
                self.app_action(AppAction::NewProject, tx);
                Ok(())
            }
            "open-project" => {
                self.app_action(AppAction::OpenProject, tx);
                Ok(())
            }
            _ => Ok(()),
        };
        // What refused is the session's notice already.
        done.ok();
        self.handled();
    }

    /// The actions of the tools shown and of the app, with the keys they have now, for `--tree`.
    pub(crate) fn shown_actions(&self) -> Vec<serde_json::Value> {
        let tools: std::collections::BTreeSet<&str> = (self.shown.iter())
            .filter_map(|id| self.project().tool_of(id))
            .chain([APP])
            .collect();
        let groups =
            (self.keys.keymap.groups()).filter(|group| tools.contains(group.name.as_str()));
        groups
            .flat_map(|group| {
                group.actions.iter().map(|action| {
                    let id = shortcuts::id(&group.name, &action.name);
                    serde_json::json!({ "action": id, "title": action.title, "keys": self.keys.keymap.keys(&id) })
                })
            })
            .collect()
    }

    /// The key each action of a tool has now, by `<tool>.<action>`, for `ctx.shortcut`.
    pub(crate) fn action_keys(&self) -> BTreeMap<String, Option<String>> {
        let groups = self.keys.keymap.groups().filter(|group| group.name != APP);
        let ids = groups.flat_map(|group| {
            (group.actions.iter()).map(|action| shortcuts::id(&group.name, &action.name))
        });
        ids.map(|id| {
            let key = self.keys.keymap.keys(&id).first().cloned();
            (id, key)
        })
        .collect()
    }
}

pub(crate) fn on_key(state: &mut State, key: &KeyEvent, tx: &mut Tx) -> bool {
    // What a field passes on is typing, never a shortcut.
    if state.typing_now() {
        return false;
    }
    // A dialog keeps every key until it closes: its own buttons take theirs.
    if !state.waiting.asked.is_empty() || state.shell.dialog.is_some() {
        return true;
    }
    let Some(name) = name_of(&key.key) else {
        return false;
    };
    let held = held_of(&name, key.mods);
    if held.platform {
        state.let_go_of_keys();
    }
    if panel_key(state, key, false, tx) {
        state.handled();
        return true;
    }
    let chord = shortcuts::chord(&name, held);
    let plain = shortcuts::chord(
        &name,
        Held {
            shift: false,
            ..held
        },
    );
    let app: Option<String> = (state.keys.keymap.bound(APP, &chord).first()).map(|a| a.to_string());
    if let Some(action) = app
        .as_deref()
        .filter(|action| BEFORE_TOOLS.contains(action))
    {
        state.app_key(action, tx);
        return true;
    }
    let repeat = tx.key_repeat();
    let main = state.main().cloned();
    for (instance, everywhere) in state.chain(tx.focused()) {
        if state.act_on_key(&instance, &chord, everywhere, repeat) {
            return true;
        }
        let Some(tool) = state.project().tool_of(&instance).map(str::to_string) else {
            continue;
        };
        let Some(tools) = &state.tools else {
            continue;
        };
        let takes = (!everywhere && tools.takes_key(&tool, &plain))
            || (Some(&instance) == main.as_ref() && tools.takes_global_key(&tool, &plain));
        if !takes {
            continue;
        }
        // Only a key with nothing held comes up: macOS sends no up while cmd is held.
        let modified = plain != name;
        if !repeat && (modified || !state.keys.held.contains_key(&name)) {
            if !modified {
                state.keys.held.insert(name, instance.clone());
            }
            state.key_to(&instance, (&plain, true), held);
        }
        return true;
    }
    let Some(action) = app else {
        return false;
    };
    state.app_key(&action, tx);
    true
}

/// The keys of the left panel, from inside it or from the window: cmd-L, cmd-B, cmd-., and
/// Escape inside it. Whether `key` was one.
pub(super) fn panel_key(state: &mut State, key: &KeyEvent, inside: bool, tx: &mut Tx) -> bool {
    match &key.key {
        Key::Command('l') => state.agent_keys(inside, tx),
        Key::Command('b') => state.open_or_close_panel(inside, tx),
        Key::Command('.') => match state.panel.agent.as_mut() {
            Some(agent) => agent.stop(),
            None => return false,
        },
        Key::Escape if inside => state.give_keys_back(tx),
        _ => return false,
    }
    true
}

/// A key came up: to the `onKey` that took it going down, if one did.
pub(crate) fn on_key_up(state: &mut State, key: &KeyEvent, _: &mut Tx) -> bool {
    let Some(name) = name_of(&key.key) else {
        return false;
    };
    let Some(instance) = state.keys.held.remove(&name) else {
        return false;
    };
    let held = held_of(&name, key.mods);
    state.key_to(&instance, (&name, false), held);
    true
}

/// The window lost the keys: whatever an `onKey` holds comes up.
pub(crate) fn on_focus(state: &mut State, focused: bool, _: &mut Tx) {
    if !focused {
        state.let_go_of_keys();
    }
}
