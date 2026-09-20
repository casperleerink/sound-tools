//! Whether a focus came from the keyboard, for focus rings.

use std::cell::Cell;

use gpui::{Context, FocusHandle, Window};

/// Whether the focus of a view or a control came from the keyboard. Only then it shows its
/// ring: after a click the pointer already says where the composer is. GPUI's own
/// focus-visible would also show the ring when a key follows a click, and space follows a
/// click all the time here.
///
/// It is worked out while rendering or painting: a focus change draws the whole window again,
/// so the first frame with the focus sees the input that brought it.
#[derive(Default)]
pub struct KeyboardFocus {
    had_focus: Cell<bool>,
    from_keyboard: Cell<bool>,
}

impl KeyboardFocus {
    /// A mouse press on what holds this: the ring goes.
    pub fn pressed<V: 'static>(&self, cx: &mut Context<V>) {
        if self.from_keyboard.replace(false) {
            cx.notify();
        }
    }

    /// Whether the handle has the focus from the keyboard now, so that its ring shows.
    pub fn shows_ring(&self, handle: &FocusHandle, window: &Window) -> bool {
        let has_focus = handle.is_focused(window);
        if has_focus && !self.had_focus.get() {
            self.from_keyboard.set(window.last_input_was_keyboard());
        }
        self.had_focus.set(has_focus);
        has_focus && self.from_keyboard.get()
    }
}
