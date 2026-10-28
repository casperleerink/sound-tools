//! Whether a focus came from the keyboard, for focus rings.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, BoxShadow, ClickEvent, Context, Div, ElementId, FocusHandle, MouseButton, Stateful,
    Window, point, prelude::*, px,
};

use crate::theme::ActiveTheme;

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

/// A div that does what a button does, such as a track's head: a tab stop whose ring shows for
/// keyboard focus, inside its edges so that what it is cut by keeps it, and that enter clicks.
#[derive(IntoElement)]
pub struct TabStop {
    id: ElementId,
    element: Stateful<Div>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

struct TabStopState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
}

impl TabStop {
    /// `id` keeps its focus between frames.
    pub fn new(id: impl Into<ElementId>, element: Stateful<Div>) -> Self {
        Self {
            id: id.into(),
            element,
            on_click: None,
        }
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for TabStop {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id, cx, |_, cx| TabStopState {
            focus_handle: cx.focus_handle().tab_stop(true),
            keyboard_focus: KeyboardFocus::default(),
        });
        let focus_handle = state.read(cx).focus_handle.clone();
        let ring_shows = (state.read(cx).keyboard_focus).shows_ring(&focus_handle, window);
        let ring = BoxShadow {
            color: cx.theme().lavender,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(1.5),
            inset: true,
        };
        (self.element.track_focus(&focus_handle))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                state.update(cx, |state, cx| state.keyboard_focus.pressed(cx));
            })
            .when(ring_shows, |element| element.shadow(vec![ring]))
            .when_some(self.on_click, |element, on_click| {
                element.on_click(move |event, window, cx| on_click(event, window, cx))
            })
    }
}
