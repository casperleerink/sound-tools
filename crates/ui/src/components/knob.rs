//! Knob: a small round control with a dotted value arc and a pointer. Drag up and down to
//! change. GPUI has no arc primitive, so the arc is a ring of dots.
//!
//! Controlled: the caller owns the value, gives it on every render and hears a [`KnobChange`].
//! So a knob on saved state keeps no copy of it, and a value that changes from outside shows
//! at once, also during a drag. The knob keeps only its focus handle and the open drag, in
//! element state under its id.
//!
//! - A drag works from the value at mouse down and the distance the pointer went, so a drag
//!   there and back ends where it began, at exactly that value, also when it has more digits
//!   than the knob gives. A press without a move up or down reports nothing.
//! - A drag reports a value only when it is not the one it reported last. It does not look at
//!   the value of the last render: several mouse moves may arrive between two frames.
//! - Any new mouse press ends a drag that is still open, because its mouse up was lost.
//! - Escape during a drag reports [`KnobChange::DragCancel`].
//! - A double click reports the default value, when one is set.
//! - The arrow keys step by a fiftieth of the travel, with shift by a five-hundredth.
//! - The ring shows only when the focus came from the keyboard.
//!
//! [`KnobRange`] maps the value to the travel of the knob, linear or logarithmic, and gives
//! values of three significant digits, so a readout and a saved file stay short.

use std::rc::Rc;

use gpui::{
    App, CursorStyle, DispatchPhase, Div, ElementId, Entity, FocusHandle, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, StyleRefinement,
    Window, canvas, div, prelude::*, px,
};

use crate::focus::KeyboardFocus;
use crate::theme::ActiveTheme;
use crate::typography;

/// The arc runs from -135 to +135 degrees, like a hardware pot.
const SWEEP: f32 = 270.;
const DOTS: usize = 25;
const DOT: f32 = 2.5;
/// Pixels of vertical drag for the full range.
const DRAG_RANGE: f32 = 160.;
/// What one arrow key moves, as a part of the travel. With shift it is a tenth of this.
const KEY_STEP: f32 = 0.02;
const FINE_KEY_STEP: f32 = 0.002;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnobScale {
    Linear,
    /// Equal travel for equal ratios: for frequencies and times. The range must be above zero.
    Logarithmic,
}

/// The values of a knob and how they spread over its travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnobRange {
    pub min: f32,
    pub max: f32,
    pub scale: KnobScale,
}

impl KnobRange {
    pub const fn linear(min: f32, max: f32) -> Self {
        let scale = KnobScale::Linear;
        Self { min, max, scale }
    }

    pub const fn logarithmic(min: f32, max: f32) -> Self {
        let scale = KnobScale::Logarithmic;
        Self { min, max, scale }
    }

    /// Where a value is on the travel, from 0 to 1. A value outside the range is at an end.
    pub fn position(&self, value: f32) -> f32 {
        let value = value.clamp(self.min, self.max);
        let position = match self.scale {
            KnobScale::Linear => (value - self.min) / (self.max - self.min),
            KnobScale::Logarithmic => (value / self.min).ln() / (self.max / self.min).ln(),
        };
        position.clamp(0., 1.)
    }

    /// The value at a place on the travel, with three significant digits. The ends are exact.
    pub fn value(&self, position: f32) -> f32 {
        let position = position.clamp(0., 1.);
        let value = match self.scale {
            KnobScale::Linear => self.min + (self.max - self.min) * position,
            KnobScale::Logarithmic => self.min * (self.max / self.min).powf(position),
        };
        three_digits(value).clamp(self.min, self.max)
    }
}

/// A number with three significant digits and no zeros at its end: `2`, `15.5`, `632`. What a
/// readout next to a knob shows, so a value at rest is short and a moving one does not jump
/// between widths.
pub fn short(value: f32) -> String {
    if value == 0. {
        return "0".into();
    }
    let decimals = (2 - value.abs().log10().floor() as i32).max(0) as usize;
    let text = format!("{value:.decimals$}");
    match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.').into(),
        false => text,
    }
}

fn three_digits(value: f32) -> f32 {
    if value == 0. || !value.is_finite() {
        return value;
    }
    // In f64, so that the result is the f32 nearest to the short decimal number.
    let value = f64::from(value);
    let unit = 10_f64.powf(2. - value.abs().log10().floor());
    ((value * unit).round() / unit) as f32
}

/// What a knob asks of its owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KnobChange {
    /// A mouse move of a drag gave another value. The first one of a drag begins it.
    Drag(f32),
    /// Mouse up, after at least one `Drag`.
    DragEnd,
    /// Escape, after at least one `Drag`: the value of mouse down is wanted back.
    DragCancel,
    /// A key step, or the default by a double click: one finished change.
    Set(f32),
}

type ChangeHandler = Rc<dyn Fn(KnobChange, &mut Window, &mut App)>;

#[derive(Clone, Copy)]
struct KnobDrag {
    start_y: f32,
    start_value: f32,
    start_position: f32,
    /// The value that went out last. The value of the press before the first `Drag`.
    sent: f32,
    /// Whether a `Drag` went out, so that the end of the drag has something to end.
    changed: bool,
}

impl KnobDrag {
    /// The value for a pointer at `y`. Back at the height of the press it is the value of the
    /// press itself, not that value in three digits: a press with a sideways move, or a drag
    /// there and back, must not rewrite a value that was written by hand.
    fn value_at(&self, y: f32, range: &KnobRange) -> f32 {
        let travelled = self.start_y - y;
        if travelled == 0. {
            return self.start_value;
        }
        range.value(self.start_position + travelled / DRAG_RANGE)
    }
}

struct KnobState {
    focus_handle: FocusHandle,
    keyboard_focus: KeyboardFocus,
    drag: Option<KnobDrag>,
}

#[derive(IntoElement)]
pub struct Knob {
    base: Div,
    id: ElementId,
    value: f32,
    range: KnobRange,
    default_value: Option<f32>,
    size: f32,
    label: Option<SharedString>,
    readout: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Knob {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            value: 0.,
            range: KnobRange::linear(0., 1.),
            default_value: None,
            size: 44.,
            label: None,
            readout: None,
            disabled: false,
            on_change: None,
        }
    }

    pub fn range(mut self, range: KnobRange) -> Self {
        self.range = range;
        self
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    /// What a double click sets.
    pub fn default_value(mut self, value: f32) -> Self {
        self.default_value = Some(value);
        self
    }

    /// Diameter in pixels.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The value as text, with its unit. The caller formats it: only it knows the unit.
    pub fn readout(mut self, readout: impl Into<SharedString>) -> Self {
        self.readout = Some(readout.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(KnobChange, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Knob {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// Centre offset of a point on the arc at `fraction` of the sweep, at radius `r`.
fn arc_point(fraction: f32, r: f32) -> (f32, f32) {
    let angle = (-SWEEP / 2. + SWEEP * fraction).to_radians();
    (r * angle.sin(), -r * angle.cos())
}

/// The end of a drag: mouse up, the button came up somewhere else, or a new press. Nothing
/// that is drawn depends on the drag, so nobody is notified: these listeners hear every mouse
/// up and every press of the window.
fn end_drag(
    state: &Entity<KnobState>,
    on_change: &ChangeHandler,
    window: &mut Window,
    cx: &mut App,
) {
    if state.read(cx).drag.is_none() {
        return;
    }
    let drag = state.update(cx, |state, _| state.drag.take());
    if drag.is_some_and(|drag| drag.changed) {
        on_change(KnobChange::DragEnd, window, cx);
    }
}

impl RenderOnce for Knob {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| KnobState {
            focus_handle: cx.focus_handle(),
            keyboard_focus: KeyboardFocus::default(),
            drag: None,
        });
        let disabled = self.disabled;
        let focus_handle = state.read(cx).focus_handle.clone().tab_stop(!disabled);
        let ring_shows = state
            .read(cx)
            .keyboard_focus
            .shows_ring(&focus_handle, window);

        let theme = cx.theme();
        let (dim, lit, face, border, muted, text, ring) = (
            theme.alpha_at(0.10),
            theme.gray_950,
            theme.alpha_at(0.05),
            theme.alpha_at(0.10),
            theme.gray_700,
            theme.gray_950,
            theme.lavender,
        );
        let (size, value, range) = (self.size, self.value, self.range);
        let position = range.position(value);
        let centre = size / 2.;
        let arc_r = centre - DOT / 2.;
        let face_size = size - DOT * 2. - 4.;
        let (pointer_x, pointer_y) = arc_point(position, face_size / 2. - 5.);

        let dots = (0..DOTS).map(|ix| {
            let f = ix as f32 / (DOTS - 1) as f32;
            let (dx, dy) = arc_point(f, arc_r);
            div()
                .absolute()
                .left(px(centre + dx - DOT / 2.))
                .top(px(centre + dy - DOT / 2.))
                .size(px(DOT))
                .rounded_full()
                .bg(if f <= position + 0.001 { lit } else { dim })
        });

        let on_change = self.on_change.filter(|_| !disabled);
        let default_value = self.default_value;
        // For tests, which find the knob by its id: `knob-<id>`. Nothing in a normal build.
        let selector = self.id.clone();
        let knob = div()
            .id(self.id)
            .debug_selector(move || format!("knob-{selector}"))
            .relative()
            .size(px(size))
            .when(disabled, |d| d.cursor_not_allowed())
            .when_some(on_change, |d, on_change| {
                // The press also gives the knob the focus: GPUI does that for a tracked handle.
                let on_mouse_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        let reset = event.click_count == 2;
                        state.update(cx, |state, cx| {
                            state.keyboard_focus.pressed(cx);
                            state.drag = (!reset).then(|| KnobDrag {
                                start_y: f32::from(event.position.y),
                                start_value: value,
                                start_position: position,
                                sent: value,
                                changed: false,
                            });
                        });
                        let default_value = default_value.filter(|default| *default != value);
                        if let Some(default_value) = default_value.filter(|_| reset) {
                            on_change(KnobChange::Set(default_value), window, cx);
                        }
                    }
                };
                let on_key_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                        let modifiers = event.keystroke.modifiers;
                        if modifiers.control || modifiers.alt || modifiers.platform {
                            return;
                        }
                        let dragging = state.read(cx).drag.is_some();
                        let step = if modifiers.shift {
                            FINE_KEY_STEP
                        } else {
                            KEY_STEP
                        };
                        let step = match event.keystroke.key.as_str() {
                            "escape" if dragging => {
                                cx.stop_propagation();
                                let drag = state.update(cx, |state, _| state.drag.take());
                                if drag.is_some_and(|drag| drag.changed) {
                                    on_change(KnobChange::DragCancel, window, cx);
                                }
                                return;
                            }
                            "up" | "right" => step,
                            "down" | "left" => -step,
                            _ => return,
                        };
                        cx.stop_propagation();
                        // The mouse has the knob: a key would fight the next mouse move.
                        if dragging {
                            return;
                        }
                        let next = range.value(position + step);
                        if next != value {
                            on_change(KnobChange::Set(next), window, cx);
                        }
                    }
                };
                // A drag goes on wherever the pointer is, so these are not hit tested. They are
                // there on every frame and look at the drag when an event arrives.
                let listeners = canvas(
                    |_, _, _| {},
                    move |_, (), window, _| {
                        window.on_mouse_event({
                            let (state, on_change) = (state.clone(), on_change.clone());
                            move |event: &MouseMoveEvent, phase, window, cx| {
                                let Some(drag) = state.read(cx).drag else {
                                    return;
                                };
                                if phase != DispatchPhase::Bubble {
                                    return;
                                }
                                if !event.dragging() {
                                    // The button came up somewhere that did not tell us.
                                    return end_drag(&state, &on_change, window, cx);
                                }
                                let next = drag.value_at(f32::from(event.position.y), &range);
                                if next == drag.sent {
                                    return;
                                }
                                state.update(cx, |state, _| {
                                    if let Some(drag) = &mut state.drag {
                                        (drag.sent, drag.changed) = (next, true);
                                    }
                                });
                                on_change(KnobChange::Drag(next), window, cx);
                            }
                        });
                        window.on_mouse_event({
                            let (state, on_change) = (state.clone(), on_change.clone());
                            move |event: &MouseUpEvent, phase, window, cx| {
                                let left = event.button == MouseButton::Left;
                                if phase == DispatchPhase::Bubble && left {
                                    end_drag(&state, &on_change, window, cx);
                                }
                            }
                        });
                        // A press while a drag is open: its mouse up went somewhere that did
                        // not tell this window. Without this, the held button of the new press
                        // would look like the old drag going on. Before the press of the knob
                        // itself, which opens a new drag.
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, window, cx| {
                            if phase == DispatchPhase::Capture {
                                end_drag(&state, &on_change, window, cx);
                            }
                        });
                    },
                );
                d.cursor(CursorStyle::ResizeUpDown)
                    .track_focus(&focus_handle)
                    .on_key_down(on_key_down)
                    .on_mouse_down(MouseButton::Left, on_mouse_down)
                    .child(listeners.absolute().size_0())
            })
            .children(dots)
            // Face.
            .child(
                div()
                    .absolute()
                    .left(px(centre - face_size / 2.))
                    .top(px(centre - face_size / 2.))
                    .size(px(face_size))
                    .rounded_full()
                    .bg(face)
                    .map(|face| match ring_shows {
                        true => face.border_2().border_color(ring),
                        false => face.border_1().border_color(border),
                    }),
            )
            // Pointer.
            .child(
                div()
                    .absolute()
                    .left(px(centre + pointer_x - 1.5))
                    .top(px(centre + pointer_y - 1.5))
                    .size(px(3.))
                    .rounded_full()
                    .bg(lit),
            );

        self.base
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .when(disabled, |d| d.opacity(0.4))
            .child(knob)
            .when_some(self.label, |d, label| {
                d.child(
                    div()
                        .mt(px(8.))
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(muted)
                        .child(label),
                )
            })
            .when_some(self.readout, |d, readout| {
                d.child(
                    div()
                        .font(typography::tabular())
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(text)
                        .whitespace_nowrap()
                        .child(readout),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGES: [KnobRange; 4] = [
        KnobRange::linear(0., 1.),
        KnobRange::linear(-50., 50.),
        KnobRange::logarithmic(20., 20_000.),
        KnobRange::logarithmic(0.001, 10.),
    ];

    #[test]
    fn the_ends_of_the_travel_are_the_ends_of_the_range() {
        for range in RANGES {
            assert_eq!(range.value(0.), range.min);
            assert_eq!(range.value(1.), range.max);
            assert_eq!(range.position(range.min), 0.);
            assert_eq!(range.position(range.max), 1.);
            // Outside is an end too.
            assert_eq!(range.value(-0.5), range.min);
            assert_eq!(range.value(1.5), range.max);
            assert_eq!(range.position(range.min - 1.), 0.);
            assert_eq!(range.position(range.max * 2. + 1.), 1.);
        }
    }

    #[test]
    fn a_value_goes_to_its_position_and_back() {
        for range in RANGES {
            for step in 0..=1000 {
                let value = range.value(step as f32 / 1000.);
                let back = range.value(range.position(value));
                assert_eq!(back, value, "{range:?} at step {step}");
            }
        }
    }

    #[test]
    fn a_position_goes_to_its_value_and_back_within_the_rounding() {
        for range in RANGES {
            for step in 0..=1000 {
                let position = step as f32 / 1000.;
                let back = range.position(range.value(position));
                // Three digits are at most half a percent of a value, which is this much travel
                // on the narrowest range here, the three decades of the frequencies.
                assert!((back - position).abs() < 0.006, "{range:?} at {position}");
            }
        }
    }

    #[test]
    fn the_travel_is_even_in_ratios_on_a_logarithmic_range() {
        let range = KnobRange::logarithmic(20., 20_000.);
        assert_eq!(range.value(1. / 3.), 200.);
        assert_eq!(range.value(2. / 3.), 2_000.);
        assert_eq!(range.value(0.5), 632.);
        assert!((range.position(2_000.) - 2. / 3.).abs() < 1e-6);
        let linear = KnobRange::linear(0., 1.);
        assert_eq!(linear.value(0.5), 0.5);
        assert_eq!(linear.position(0.25), 0.25);
    }

    #[test]
    fn a_drag_back_at_the_height_of_the_press_gives_the_value_of_the_press_exactly() {
        let range = KnobRange::logarithmic(20., 20_000.);
        let drag = KnobDrag {
            start_y: 300.,
            start_value: 1234.5,
            start_position: range.position(1234.5),
            sent: 1234.5,
            changed: false,
        };
        assert_eq!(drag.value_at(300., &range), 1234.5);
        assert_eq!(drag.value_at(299., &range), 1290.);
        assert_eq!(drag.value_at(301., &range), 1180.);
    }

    #[test]
    fn values_have_three_significant_digits() {
        assert_eq!(three_digits(2143.55), 2140.);
        assert_eq!(three_digits(0.0123456), 0.0123);
        assert_eq!(three_digits(0.15549), 0.155);
        assert_eq!(three_digits(-12.34), -12.3);
        assert_eq!(three_digits(0.), 0.);
    }

    #[test]
    fn every_key_step_changes_the_value() {
        for range in RANGES {
            for step in 0..500 {
                let position = step as f32 * FINE_KEY_STEP;
                let value = range.value(position);
                let next = range.value(range.position(value) + FINE_KEY_STEP);
                assert!(next > value, "{range:?} is stuck at {value}");
            }
        }
    }
}
