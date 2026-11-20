//! The keys of the window, matched in one place.

use rune::{Key, KeyEvent, Tx};

use super::State;

pub(crate) fn on_key(state: &mut State, key: &KeyEvent, tx: &mut Tx) -> bool {
    if panel_key(state, key, false, tx) {
        state.handled();
        return true;
    }
    let done = if key.is_command('z') {
        match key.mods.shift {
            true => state.redo(),
            false => state.undo(),
        }
    } else if matches!(&key.key, rune::Key::Text(space) if space == " ") && !key.mods.cmd {
        state.play_or_stop();
        Ok(())
    } else {
        return false;
    };
    if let Err(error) = done {
        state.notice = Some(error.to_string());
    }
    state.handled();
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
