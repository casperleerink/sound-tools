//! The keys of the window, matched in one place.

use rune::{KeyEvent, Tx};

use super::State;

pub(crate) fn on_key(state: &mut State, key: &KeyEvent, _: &mut Tx) -> bool {
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
