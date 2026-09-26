//! The one gesture of every control that drags a value: the knob, the volume and the handles of
//! a display. Each of them is controlled: the owner gives the value on every render and hears a
//! [`ValueChange`]. The gesture:
//!
//! - A drag works from the value at the press and the distance the pointer went, so a press
//!   never jumps and a drag there and back ends where it began, at exactly the value of the
//!   press, also when that has more digits than the control gives.
//! - With shift the drag is ten times finer. Pressing or letting go of shift during a drag goes
//!   on from where the value is, so it does not jump either.
//! - A drag reports a value only when it is not the one it reported last. It does not look at the
//!   value of the last render: several mouse moves may arrive between two frames.
//! - A press without a move reports nothing, so a click is no undo step.
//! - Any new press ends a drag that is still open, because its mouse up was lost.
//! - Escape during a drag reports [`ValueChange::DragCancel`].
//! - A double click, or backspace on the focused control, reports its default value.
//! - The arrow keys step. What a step is belongs to the control.
//!
//! [`Travel`] and [`ValueKey`] are the maths and the keys, with no GPUI in them. The rest wires
//! them to the mouse and the keys the same way for every control.

use std::rc::Rc;

use gpui::{
    App, DispatchPhase, Entity, FocusHandle, IntoElement, KeyDownEvent, Keystroke, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window, canvas, prelude::*,
};

use crate::focus::KeyboardFocus;

/// How much finer a drag or a key step is with shift.
pub const FINE: f32 = 0.1;

/// What a control that drags a value asks of its owner. `V` is what it moves: one number, or two
/// for a handle that moves sideways and up and down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueChange<V = f32> {
    /// A mouse move of a drag gave another value. The first one of a drag begins it.
    Drag(V),
    /// Mouse up, after at least one `Drag`.
    DragEnd,
    /// Escape, after at least one `Drag`: the value of the press is wanted back.
    DragCancel,
    /// A key step, or the default by a double click or backspace: one finished change.
    Set(V),
}

/// One axis of a drag, as a place on the travel of the control from 0 to 1.
///
/// `pointer` is in points and grows in the direction that raises the value: the negative `y` for
/// a drag up, the `x` for a drag to the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Travel {
    /// Where the pointer was when the speed was last set: at the press, or at the move before
    /// shift changed.
    anchor: f32,
    /// The place on the travel there. Not clamped: past an end a drag back first undoes the
    /// overshoot, so a drag there and back ends where it began.
    anchor_position: f32,
    fine: bool,
    /// Points of pointer travel for the whole travel, at the normal speed.
    span: f32,
    press_position: f32,
    /// Where the pointer was at the last move. A change of speed takes effect from there.
    last: f32,
}

impl Travel {
    pub fn new(pointer: f32, position: f32, span: f32) -> Self {
        Self {
            anchor: pointer,
            anchor_position: position,
            fine: false,
            span,
            press_position: position,
            last: pointer,
        }
    }

    /// The place on the travel for the pointer now, or `None` when it is exactly the place of
    /// the press: the owner then keeps the value of the press as it was.
    pub fn position(&mut self, pointer: f32, fine: bool) -> Option<f32> {
        if fine != self.fine {
            self.anchor_position = self.unclamped(self.last);
            self.anchor = self.last;
            self.fine = fine;
        }
        self.last = pointer;
        let position = self.unclamped(pointer);
        (position != self.press_position).then(|| position.clamp(0., 1.))
    }

    fn unclamped(&self, pointer: f32) -> f32 {
        let speed = if self.fine { FINE } else { 1. };
        self.anchor_position + (pointer - self.anchor) / self.span * speed
    }
}

/// What a key does to a focused control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKey {
    /// Up or right raise the value, down or left lower it. With shift the step is fine.
    Step { up: bool, fine: bool },
    /// Backspace or delete: the default value.
    Reset,
    /// Escape: a drag that is open goes back.
    Cancel,
}

impl ValueKey {
    /// `None` for any other key, and for a key with control, alt or command, which belongs to
    /// something else.
    pub fn of(keystroke: &Keystroke) -> Option<Self> {
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return None;
        }
        let fine = modifiers.shift;
        match keystroke.key.as_str() {
            "up" | "right" => Some(Self::Step { up: true, fine }),
            "down" | "left" => Some(Self::Step { up: false, fine }),
            "backspace" | "delete" => Some(Self::Reset),
            "escape" => Some(Self::Cancel),
            _ => None,
        }
    }
}

pub(crate) type ChangeHandler<V> = Rc<dyn Fn(ValueChange<V>, &mut Window, &mut App)>;

/// Works out the value for the pointer and shift during one drag.
type ValueAt<V> = Box<dyn FnMut(Point<Pixels>, bool) -> V>;

struct OpenDrag<V> {
    value_at: ValueAt<V>,
    /// The value that went out last. The value of the press before the first `Drag`.
    sent: V,
    /// Whether a `Drag` went out, so that the end of the drag has something to end.
    changed: bool,
}

/// What a dragging control keeps in element state under its id: its focus and the open drag.
pub(crate) struct GestureState<V> {
    pub focus_handle: FocusHandle,
    pub keyboard_focus: KeyboardFocus,
    drag: Option<OpenDrag<V>>,
}

impl<V: Copy + PartialEq + 'static> GestureState<V> {
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            keyboard_focus: KeyboardFocus::default(),
            drag: None,
        }
    }
}

/// A mouse press on the control. A double click reports `reset` when it is another value, any
/// other press opens a drag from `value`. The press also gives the control the focus: GPUI does
/// that for a tracked handle.
pub(crate) fn press<V: Copy + PartialEq + 'static>(
    state: &Entity<GestureState<V>>,
    event: &MouseDownEvent,
    value: V,
    reset: Option<V>,
    value_at: impl FnMut(Point<Pixels>, bool) -> V + 'static,
    on_change: &ChangeHandler<V>,
    window: &mut Window,
    cx: &mut App,
) {
    let double = event.click_count == 2;
    state.update(cx, |state, cx| {
        state.keyboard_focus.pressed(cx);
        state.drag = (!double).then(|| OpenDrag {
            value_at: Box::new(value_at),
            sent: value,
            changed: false,
        });
    });
    if let Some(reset) = reset.filter(|reset| double && *reset != value) {
        on_change(ValueChange::Set(reset), window, cx);
    }
}

/// A key on the focused control. `step(up, fine)` gives the value one step on, or `None` at an
/// end; `reset` is the default. Escape cancels a drag. Every key of a drag is taken and does
/// nothing else: the mouse has the control, and a key would fight the next mouse move.
pub(crate) fn key_down<V: Copy + PartialEq + 'static>(
    state: &Entity<GestureState<V>>,
    event: &KeyDownEvent,
    step: impl Fn(bool, bool) -> Option<V>,
    reset: Option<V>,
    on_change: &ChangeHandler<V>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(key) = ValueKey::of(&event.keystroke) else {
        return;
    };
    let dragging = state.read(cx).drag.is_some();
    let change = match key {
        ValueKey::Cancel if dragging => {
            let drag = state.update(cx, |state, _| state.drag.take());
            drag.filter(|drag| drag.changed)
                .map(|_| ValueChange::DragCancel)
        }
        // Escape without a drag belongs to what holds the control, such as a panel it closes.
        ValueKey::Cancel => return,
        _ if dragging => None,
        ValueKey::Step { up, fine } => step(up, fine).map(ValueChange::Set),
        ValueKey::Reset => reset.map(ValueChange::Set),
    };
    cx.stop_propagation();
    if let Some(change) = change {
        on_change(change, window, cx);
    }
}

/// The end of a drag: mouse up, the button came up somewhere else, or a new press. Nothing that
/// is drawn depends on the drag, so nobody is notified: these listeners hear every mouse up and
/// every press of the window.
fn end_drag<V: Copy + PartialEq + 'static>(
    state: &Entity<GestureState<V>>,
    on_change: &ChangeHandler<V>,
    window: &mut Window,
    cx: &mut App,
) {
    if state.read(cx).drag.is_none() {
        return;
    }
    let drag = state.update(cx, |state, _| state.drag.take());
    if drag.is_some_and(|drag| drag.changed) {
        on_change(ValueChange::DragEnd, window, cx);
    }
}

/// The listeners of a drag, as an element of no size to put into the control. A drag goes on
/// wherever the pointer is, so they are not hit tested. They are there on every frame and look
/// at the drag when an event arrives.
pub(crate) fn drag_listeners<V: Copy + PartialEq + 'static>(
    state: Entity<GestureState<V>>,
    on_change: ChangeHandler<V>,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            window.on_mouse_event({
                let (state, on_change) = (state.clone(), on_change.clone());
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || state.read(cx).drag.is_none() {
                        return;
                    }
                    if !event.dragging() {
                        // The button came up somewhere that did not tell us.
                        return end_drag(&state, &on_change, window, cx);
                    }
                    let next = state.update(cx, |state, _| {
                        let drag = state.drag.as_mut()?;
                        let next = (drag.value_at)(event.position, event.modifiers.shift);
                        if next == drag.sent {
                            return None;
                        }
                        (drag.sent, drag.changed) = (next, true);
                        Some(next)
                    });
                    if let Some(next) = next {
                        on_change(ValueChange::Drag(next), window, cx);
                    }
                }
            });
            window.on_mouse_event({
                let (state, on_change) = (state.clone(), on_change.clone());
                move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                        end_drag(&state, &on_change, window, cx);
                    }
                }
            });
            // A press while a drag is open: its mouse up went somewhere that did not tell this
            // window. Without this, the held button of the new press would look like the old
            // drag going on. Before the press of the control itself, which opens a new drag.
            window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    end_drag(&state, &on_change, window, cx);
                }
            });
        },
    )
    .absolute()
    .size_0()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The travel of a knob.
    const SPAN: f32 = 200.;

    #[test]
    fn the_whole_travel_is_the_span_of_pointer_movement() {
        let mut travel = Travel::new(0., 0., SPAN);
        assert_eq!(travel.position(100., false), Some(0.5));
        assert_eq!(travel.position(200., false), Some(1.));
        let mut travel = Travel::new(500., 1., SPAN);
        assert_eq!(travel.position(300., false), Some(0.));
    }

    #[test]
    fn shift_is_ten_times_finer() {
        let mut travel = Travel::new(0., 0.5, SPAN);
        let coarse = travel.position(20., false).unwrap_or(0.5) - 0.5;
        let mut travel = Travel::new(0., 0.5, SPAN);
        let fine = travel.position(20., true).unwrap_or(0.5) - 0.5;
        assert!((coarse - 0.1).abs() < 1e-6, "{coarse}");
        assert!((fine - 0.01).abs() < 1e-6, "{fine}");
    }

    #[test]
    fn a_press_never_jumps() {
        // At the press the value is the value of the press itself, whatever it is.
        let mut travel = Travel::new(300., 0.123_456_7, SPAN);
        assert_eq!(travel.position(300., false), None);
        assert_eq!(travel.position(300., true), None);
        // One point away it is one point of travel away, not somewhere the pointer points to.
        let moved = travel.position(301., false);
        assert!(moved.is_some_and(|position| (position - 0.128_456_7).abs() < 1e-5));
    }

    #[test]
    fn pressing_or_releasing_shift_during_a_drag_goes_on_from_where_the_value_is() {
        let mut travel = Travel::new(0., 0.5, SPAN);
        let before = travel.position(40., false);
        assert_eq!(before, Some(0.7));
        // Shift down at the same place: no jump.
        assert_eq!(travel.position(40., true), before);
        // Then fine from there.
        let fine = travel.position(60., true).unwrap_or(0.);
        assert!((fine - 0.71).abs() < 1e-6, "{fine}");
        // Shift up: no jump, then coarse again from there.
        assert_eq!(travel.position(60., false), Some(fine));
        let coarse = travel.position(80., false).unwrap_or(0.);
        assert!((coarse - 0.81).abs() < 1e-6, "{coarse}");
    }

    #[test]
    fn a_drag_stops_at_the_ends_and_there_and_back_ends_where_it_began() {
        let mut travel = Travel::new(0., 0.5, SPAN);
        assert_eq!(travel.position(400., false), Some(1.));
        assert_eq!(travel.position(-400., false), Some(0.));
        // Back past the far end is still the end: the overshoot is undone first.
        assert_eq!(travel.position(150., false), Some(1.));
        assert_eq!(travel.position(0., false), None);
    }

    fn key(text: &str) -> Option<ValueKey> {
        let keystroke = Keystroke::parse(text).ok()?;
        ValueKey::of(&keystroke)
    }

    #[test]
    fn the_keys_step_reset_and_cancel() {
        let up = |fine| Some(ValueKey::Step { up: true, fine });
        let down = |fine| Some(ValueKey::Step { up: false, fine });
        assert_eq!(key("up"), up(false));
        assert_eq!(key("right"), up(false));
        assert_eq!(key("shift-up"), up(true));
        assert_eq!(key("down"), down(false));
        assert_eq!(key("shift-left"), down(true));
        assert_eq!(key("backspace"), Some(ValueKey::Reset));
        assert_eq!(key("delete"), Some(ValueKey::Reset));
        assert_eq!(key("escape"), Some(ValueKey::Cancel));
        // Keys of something else.
        assert_eq!(key("cmd-up"), None);
        assert_eq!(key("alt-left"), None);
        assert_eq!(key("space"), None);
        assert_eq!(key("a"), None);
    }
}
