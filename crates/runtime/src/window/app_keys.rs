//! The actions of the app itself, such as undo and opening a project. One table is their group
//! in the keymap, their key bindings in GPUI and the key a menu draws for each, so the
//! composer's own key for one moves all three at once.

use std::rc::Rc;

use anyhow::Result;
use gpui::{Action, App, DummyKeyboardMapper, KeyBinding, KeyBindingContextPredicate};
use sound_ui::components::text_input;
use sound_ui::{Shortcuts, shortcuts};

use super::{
    KEY_CONTEXT, NewProject, OpenCloseLeftPanel, OpenProject, Quit, Redo, ToggleComputerKeys,
    ToggleLeftPanel, Undo, UndoKeepingVersion,
};

/// The group of the app's actions in the keymap. No tool of a project has this name.
pub(super) const GROUP: &str = "app";

pub(super) const UNDO: &str = "undo";
pub(super) const REDO: &str = "redo";
pub(super) const UNDO_KEEPING_VERSION: &str = "undo-keeping-version";
pub(super) const NEW_PROJECT: &str = "new-project";
pub(super) const OPEN_PROJECT: &str = "open-project";
pub(super) const SETTINGS: &str = "settings";

struct AppAction {
    name: &'static str,
    /// What the composer reads for it in Settings.
    title: &'static str,
    keys: &'static [&'static str],
    action: fn() -> Box<dyn Action>,
    /// The key context its keys work in. `None` is every window of the app.
    context: Option<&'static str>,
    /// Whether its keys work while a text field has them. Undo does not: there cmd-z is the
    /// field's own.
    in_text: bool,
}

const ACTIONS: &[AppAction] = &[
    AppAction {
        name: UNDO,
        title: "Undo",
        keys: &["cmd-z"],
        action: || Box::new(Undo),
        context: Some(KEY_CONTEXT),
        in_text: false,
    },
    AppAction {
        name: REDO,
        title: "Redo",
        keys: &["cmd-shift-z"],
        action: || Box::new(Redo),
        context: Some(KEY_CONTEXT),
        in_text: false,
    },
    AppAction {
        name: UNDO_KEEPING_VERSION,
        title: "Undo and keep the work as a version",
        keys: &["cmd-alt-z"],
        action: || Box::new(UndoKeepingVersion),
        context: Some(KEY_CONTEXT),
        in_text: false,
    },
    AppAction {
        name: "computer-keys",
        title: "Play notes with the computer keys",
        keys: &["cmd-k"],
        action: || Box::new(ToggleComputerKeys),
        context: Some(KEY_CONTEXT),
        in_text: true,
    },
    AppAction {
        name: "agent",
        title: "Go to the agent",
        keys: &["cmd-l"],
        action: || Box::new(ToggleLeftPanel),
        context: Some(KEY_CONTEXT),
        in_text: true,
    },
    AppAction {
        name: "stop-agent",
        title: "Stop the agent",
        keys: &["cmd-."],
        action: || Box::new(sound_agent::Stop),
        context: Some(sound_agent::SIDEBAR_KEY_CONTEXT),
        in_text: true,
    },
    AppAction {
        name: "left-panel",
        title: "Open or close the left panel",
        keys: &["cmd-b"],
        action: || Box::new(OpenCloseLeftPanel),
        context: Some(KEY_CONTEXT),
        in_text: true,
    },
    AppAction {
        name: NEW_PROJECT,
        title: "New project…",
        keys: &["cmd-n"],
        action: || Box::new(NewProject),
        context: Some(KEY_CONTEXT),
        in_text: true,
    },
    AppAction {
        name: OPEN_PROJECT,
        title: "Open project…",
        keys: &["cmd-o"],
        action: || Box::new(OpenProject),
        context: Some(KEY_CONTEXT),
        in_text: true,
    },
    AppAction {
        name: SETTINGS,
        title: "Settings…",
        keys: &["cmd-,"],
        action: || Box::new(sound_ui::OpenSettings),
        context: Some(KEY_CONTEXT),
        in_text: true,
    },
    AppAction {
        name: "quit",
        title: "Quit Sound Tools",
        keys: &["cmd-q"],
        action: || Box::new(Quit),
        context: None,
        in_text: true,
    },
];

impl AppAction {
    fn id(&self) -> String {
        shortcuts::id(GROUP, self.name)
    }

    /// Its binding on `chord`, as the keymap names a key. A key that types, one with no cmd
    /// and no ctrl, never works in a text field.
    fn binding(&self, chord: &str) -> Result<KeyBinding> {
        let (keystroke, types) = match chord.strip_prefix("cmd-") {
            // cmd on macOS, ctrl on Windows and Linux.
            Some(rest) => (format!("secondary-{rest}"), false),
            None => (chord.to_string(), !chord.starts_with("ctrl-")),
        };
        let outside_text = format!("!{}", text_input::KEY_CONTEXT);
        let context = match (self.context, self.in_text && !types) {
            (Some(context), true) => Some(context.to_string()),
            (Some(context), false) => Some(format!("{context} && {outside_text}")),
            (None, true) => None,
            (None, false) => Some(outside_text),
        };
        let predicate = match context {
            Some(context) => Some(Rc::new(KeyBindingContextPredicate::parse(&context)?)),
            None => None,
        };
        let action = (self.action)();
        Ok(KeyBinding::load(
            &keystroke,
            action,
            predicate,
            false,
            None,
            &DummyKeyboardMapper,
        )?)
    }
}

/// The key the app's action `name` has now, as a menu draws it. `None` when it has none.
pub(super) fn key_of(name: &str, cx: &App) -> Option<String> {
    cx.try_global::<Shortcuts>()?
        .shown(&shortcuts::id(GROUP, name))
}

/// Lists the app's actions in the keymap and binds each to the keys it has there, now and
/// after every change of them.
pub(super) fn install(cx: &mut App) {
    let actions = ACTIONS.iter().map(|action| shortcuts::Action {
        name: action.name.to_string(),
        title: action.title.to_string(),
        keys: action.keys.iter().map(ToString::to_string).collect(),
    });
    let group = shortcuts::Group {
        name: GROUP.to_string(),
        title: "Sound Tools".to_string(),
        actions: actions.collect(),
    };
    cx.default_global::<Shortcuts>().set_group(group);
    let mut bound = Vec::new();
    bind(&mut bound, cx);
    cx.observe_global::<Shortcuts>(move |cx| bind(&mut bound, cx))
        .detach();
}

/// Binds the app's actions to the keys they have now, when those are other than `bound`. GPUI's
/// keymap only grows or empties, so it is emptied and every other binding goes back in as it
/// was, after the app's: tab then stays the window's whatever key an action is given.
fn bind(bound: &mut Vec<Vec<String>>, cx: &mut App) {
    let Some(shortcuts) = cx.try_global::<Shortcuts>() else {
        return;
    };
    let keys: Vec<Vec<String>> = (ACTIONS.iter())
        .map(|action| shortcuts.keys(&action.id()).to_vec())
        .collect();
    if *bound == keys {
        return;
    }
    let mut own = Vec::new();
    for (action, key) in (ACTIONS.iter().zip(&keys))
        .flat_map(|(action, keys)| keys.iter().map(move |key| (action, key)))
    {
        match action.binding(key) {
            Ok(binding) => own.push(binding),
            Err(error) => eprintln!("error: {key} is no key for {}: {error:#}", action.title),
        }
    }
    let is_own = |binding: &KeyBinding| {
        (ACTIONS.iter()).any(|action| binding.action().partial_eq((action.action)().as_ref()))
    };
    let keymap = cx.key_bindings();
    let others: Vec<KeyBinding> = (keymap.borrow().bindings())
        .filter(|binding| !is_own(binding))
        .cloned()
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(own.into_iter().chain(others));
    *bound = keys;
}

#[cfg(test)]
mod tests {
    use gpui::KeyContext;

    use super::*;

    #[test]
    fn a_key_that_types_never_works_in_a_text_field() {
        let field =
            [KEY_CONTEXT, text_input::KEY_CONTEXT].map(|name| KeyContext::parse(name).unwrap());
        let works_in_a_field = |name: &str, chord: &str| {
            let action = ACTIONS.iter().find(|action| action.name == name).unwrap();
            let predicate = action.binding(chord).unwrap().predicate();
            predicate.is_none_or(|predicate| predicate.depth_of(&field).is_some())
        };
        assert!(works_in_a_field(NEW_PROJECT, "cmd-n"));
        assert!(works_in_a_field(NEW_PROJECT, "ctrl-n"));
        assert!(!works_in_a_field(NEW_PROJECT, "n"));
        assert!(!works_in_a_field(NEW_PROJECT, "alt-shift-n"));
        assert!(works_in_a_field("quit", "cmd-q"));
        assert!(!works_in_a_field("quit", "q"));
        // Undo is the field's own there, on any key.
        assert!(!works_in_a_field(UNDO, "cmd-z"));
    }

    #[test]
    fn a_name_that_is_no_key_binds_nothing() {
        assert!(ACTIONS[0].binding("cmd-nothing-z").is_err());
    }
}
