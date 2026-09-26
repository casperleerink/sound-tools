//! Drag number: a plain number that a drag up and down changes, such as the tempo in the
//! transport. No box and no fill; the owner gives the text as children.
//!
//! The gesture is that of the knob, see [`gesture`](super::gesture): a drag from the value at
//! the press, shift ten times finer, escape puts it back, the arrows step. What is its own: a
//! drag moves by whole steps from the value it began on and does not round the result, so a
//! value written by hand keeps its fraction and a drag there and back ends on exactly that
//! value. With shift the step is a tenth too.
//!
//! Controlled: the owner gives the value on every render and hears a [`ValueChange`].

use std::rc::Rc;

use gpui::{
    AnyElement, App, CursorStyle, Div, ElementId, Hsla, KeyDownEvent, MouseButton, MouseDownEvent,
    Pixels, Point, StyleRefinement, Window, div, prelude::*, px,
};

use crate::components::gesture::{self, ChangeHandler, FINE, GestureState, Travel, ValueChange};
use crate::theme::ActiveTheme;

#[derive(IntoElement)]
pub struct DragNumber {
    base: Div,
    id: ElementId,
    value: f64,
    min: f64,
    max: f64,
    /// What one point of a plain drag moves.
    per_point: f64,
    /// A plain drag moves by whole steps of this.
    step: f64,
    /// What an arrow key moves, plain and with shift.
    key_steps: (f64, f64),
    on_change: Option<ChangeHandler<f64>>,
    children: Vec<AnyElement>,
}

impl DragNumber {
    /// `value` between `min` and `max`.
    pub fn new(id: impl Into<ElementId>, value: f64, min: f64, max: f64) -> Self {
        Self {
            base: div(),
            id: id.into(),
            value,
            min,
            max,
            per_point: 1.,
            step: 1.,
            key_steps: (1., 0.1),
            on_change: None,
            children: Vec::new(),
        }
    }

    /// What one point of a plain drag moves, and the step it moves by.
    pub fn drag(mut self, per_point: f64, step: f64) -> Self {
        (self.per_point, self.step) = (per_point, step);
        self
    }

    /// What an arrow key moves, plain and with shift.
    pub fn keys(mut self, plain: f64, fine: f64) -> Self {
        self.key_steps = (plain, fine);
        self
    }

    pub fn on_change(
        mut self,
        f: impl Fn(ValueChange<f64>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for DragNumber {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for DragNumber {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

/// How much finer shift makes a drag, as a divisor, so that a fine step of a tenth is exactly
/// the `f64` of a tenth and not the `f32` one.
fn fine_divisor() -> f64 {
    (1. / f64::from(FINE)).round()
}

/// The value for a place `raw` that the pointer asks for: whole steps from `start`, inside the
/// range.
fn stepped(start: f64, raw: f64, step: f64, (min, max): (f64, f64)) -> f64 {
    let steps = ((raw - start) / step).round();
    (start + steps * step).clamp(min, max)
}

impl RenderOnce for DragNumber {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| GestureState::new(cx));
        let focus_handle = state.read(cx).focus_handle.clone().tab_stop(true);
        let ring_shows = state
            .read(cx)
            .keyboard_focus
            .shows_ring(&focus_handle, window);
        let ring = match ring_shows {
            true => cx.theme().lavender,
            false => Hsla::transparent_black(),
        };
        let (value, range, step) = (self.value, (self.min, self.max), self.step);
        let span = (self.max - self.min).max(f64::EPSILON);
        let position = ((value - self.min) / span) as f32;
        // Points of pointer travel for the whole range.
        let travel = (span / self.per_point) as f32;
        let key_steps = self.key_steps;
        let selector = self.id.clone();

        self.base
            .id(self.id)
            .debug_selector(move || format!("number-{selector}"))
            .flex()
            .flex_none()
            .items_baseline()
            .gap(px(4.))
            .px(px(6.))
            .rounded(px(6.))
            .border_1()
            .border_color(ring)
            .cursor(CursorStyle::ResizeUpDown)
            .track_focus(&focus_handle)
            .when_some(self.on_change, |d, on_change| {
                let on_mouse_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        let y = -f32::from(event.position.y);
                        let mut along = Travel::new(y, position, travel);
                        let value_at = move |pointer: Point<Pixels>, fine: bool| {
                            match along.position(-f32::from(pointer.y), fine) {
                                Some(position) => {
                                    let raw = range.0 + f64::from(position) * span;
                                    let step = if fine { step / fine_divisor() } else { step };
                                    stepped(value, raw, step, range)
                                }
                                None => value,
                            }
                        };
                        gesture::press(&state, event, value, None, value_at, &on_change, window, cx);
                    }
                };
                let on_key_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                        let step = |up: bool, fine: bool| {
                            let step = if fine { key_steps.1 } else { key_steps.0 };
                            let step = if up { step } else { -step };
                            let next = (value + step).clamp(range.0, range.1);
                            (next != value).then_some(next)
                        };
                        gesture::key_down(&state, event, Some(&step), None, &on_change, window, cx);
                    }
                };
                d.on_key_down(on_key_down)
                    .on_mouse_down(MouseButton::Left, on_mouse_down)
                    .child(gesture::drag_listeners(state, on_change))
            })
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPO: (f64, f64) = (10., 1000.);

    #[test]
    fn a_drag_moves_by_whole_steps_from_where_it_began() {
        assert_eq!(stepped(120., 125.2, 1., TEMPO), 125.);
        assert_eq!(stepped(120., 114.9, 1., TEMPO), 115.);
        // A value written by hand keeps its fraction, and there and back is exact.
        assert_eq!(stepped(93.5, 94.4, 1., TEMPO), 94.5);
        assert_eq!(stepped(93.5, 92.6, 1., TEMPO), 92.5);
        assert_eq!(stepped(93.5, 93.9, 1., TEMPO), 93.5);
        assert_eq!(stepped(120.125, 130., 1., TEMPO), 130.125);
        // A fine step is a tenth.
        let fine = 1. / fine_divisor();
        assert_eq!(fine, 0.1);
        assert!((stepped(93.5, 93.61, fine, TEMPO) - 93.6).abs() < 1e-9);
    }

    #[test]
    fn a_drag_stops_at_the_ends() {
        assert_eq!(stepped(990.5, 2000., 1., TEMPO), 1000.);
        assert_eq!(stepped(12., -5., 1., TEMPO), 10.);
    }
}
