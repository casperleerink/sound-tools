//! Knob: a 36 pt dial in a cell, with its label and value under it. A 270 degree track with the
//! value arc on it and a pointer on the face. A bipolar knob, such as pan, draws its arc from
//! the top. Drag up and down, 200 pt for the whole travel; the rest of the gesture is in
//! [`gesture`](super::gesture), which the volume and the handles of a display share.
//!
//! Controlled: the caller owns the value, gives it on every render and hears a [`ValueChange`].
//! So a knob on saved state keeps no copy of it, and a value that changes from outside shows at
//! once, also during a drag. The knob keeps only its focus handle and the open drag, in element
//! state under its id.
//!
//! - The arrow keys step by a fiftieth of the travel, with shift by a five-hundredth.
//! - The ring shows only when the focus came from the keyboard.
//!
//! [`KnobRange`] maps the value to the travel of the knob, linear or logarithmic, and gives
//! values of three significant digits, so a readout and a saved file stay short.

use std::rc::Rc;

use gpui::{
    App, Bounds, CursorStyle, Div, ElementId, MouseButton, MouseDownEvent, Pixels, SharedString,
    StyleRefinement, Window, canvas, div, prelude::*, px,
};

use crate::components::cell::{self, CONTROL_HEIGHT};
use crate::components::gesture::{self, ChangeHandler, GestureState, Travel, ValueChange};
use crate::components::paint;
use crate::theme::ActiveTheme;

/// The arc runs from -135 to +135 degrees, like a hardware pot.
const SWEEP: f32 = 270.;
/// Points of pointer travel for the whole travel.
pub const TRAVEL: f32 = 200.;
/// What one arrow key moves, as a part of the travel. With shift it is a tenth of this.
const KEY_STEP: f32 = 0.02;
const FINE_KEY_STEP: f32 = KEY_STEP * gesture::FINE;

const DIAL: f32 = CONTROL_HEIGHT;
const TRACK_WIDTH: f32 = 2.5;
const FACE: f32 = 21.;
const POINTER_WIDTH: f32 = 2.;
const RING_WIDTH: f32 = 2.;

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

#[derive(IntoElement)]
pub struct Knob {
    base: Div,
    id: ElementId,
    value: f32,
    range: KnobRange,
    default_value: Option<f32>,
    bipolar: bool,
    label: Option<SharedString>,
    readout: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler<f32>>,
}

impl Knob {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div(),
            id: id.into(),
            value: 0.,
            range: KnobRange::linear(0., 1.),
            default_value: None,
            bipolar: false,
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

    /// What a double click and backspace set.
    pub fn default_value(mut self, value: f32) -> Self {
        self.default_value = Some(value);
        self
    }

    /// The arc starts at the top, for a value with a middle such as pan or a gain of an EQ band.
    pub fn bipolar(mut self, bipolar: bool) -> Self {
        self.bipolar = bipolar;
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

    pub fn on_change(
        mut self,
        f: impl Fn(ValueChange, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl Styled for Knob {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

/// The angle of a place on the travel, in degrees from the top.
fn angle(position: f32) -> f32 {
    -SWEEP / 2. + SWEEP * position
}

/// The colours of a dial.
struct DialColors {
    track: gpui::Hsla,
    value: gpui::Hsla,
    face: gpui::Hsla,
    ring: Option<gpui::Hsla>,
}

fn paint_dial(
    bounds: Bounds<Pixels>,
    position: f32,
    bipolar: bool,
    colors: &DialColors,
    window: &mut Window,
) {
    let centre = bounds.center();
    let radius = DIAL / 2. - TRACK_WIDTH / 2.;
    let full = (angle(0.), angle(1.));
    paint::arc(window, centre, radius, TRACK_WIDTH, full, colors.track, false);
    let start = if bipolar { 0. } else { angle(0.) };
    let value = (start, angle(position));
    paint::arc(window, centre, radius, TRACK_WIDTH, value, colors.value, true);
    paint::circle(window, centre, FACE / 2., colors.face);
    if let Some(ring) = colors.ring {
        paint::ring(window, centre, FACE / 2. + RING_WIDTH, RING_WIDTH, ring);
    }
    let (inner, outer) = (
        paint::on_circle(centre, 3.5, angle(position)),
        paint::on_circle(centre, FACE / 2. - 2.5, angle(position)),
    );
    paint::line(window, inner, outer, POINTER_WIDTH, colors.value);
}

impl RenderOnce for Knob {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| GestureState::new(cx));
        let disabled = self.disabled;
        let focus_handle = state.read(cx).focus_handle.clone().tab_stop(!disabled);
        let ring_shows = state
            .read(cx)
            .keyboard_focus
            .shows_ring(&focus_handle, window);

        let theme = cx.theme();
        let colors = DialColors {
            track: theme.alpha_at(0.10),
            value: theme.gray_950,
            face: theme.gray_300,
            ring: ring_shows.then_some(theme.lavender),
        };
        let (value, range, bipolar) = (self.value, self.range, self.bipolar);
        let position = range.position(value);
        let dial = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_dial(bounds, position, bipolar, &colors, window),
        )
        .size_full();

        let on_change = self.on_change.filter(|_| !disabled);
        let default_value = self.default_value;
        // For tests, which find the knob by its id: `knob-<id>`. Nothing in a normal build.
        let selector = self.id.clone();
        let knob = div()
            .id(self.id)
            .debug_selector(move || format!("knob-{selector}"))
            .relative()
            .size(px(DIAL))
            .when(disabled, |d| d.cursor_not_allowed())
            .when_some(on_change, |d, on_change| {
                let on_mouse_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                        let y = -f32::from(event.position.y);
                        let mut travel = Travel::new(y, position, TRAVEL);
                        let value_at = move |pointer: gpui::Point<Pixels>, fine| {
                            match travel.position(-f32::from(pointer.y), fine) {
                                Some(position) => range.value(position),
                                None => value,
                            }
                        };
                        gesture::press(
                            &state,
                            event,
                            value,
                            default_value,
                            value_at,
                            &on_change,
                            window,
                            cx,
                        );
                    }
                };
                let on_key_down = {
                    let (state, on_change) = (state.clone(), on_change.clone());
                    move |event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut App| {
                        let step = |up: bool, fine: bool| {
                            let step = if fine { FINE_KEY_STEP } else { KEY_STEP };
                            let step = if up { step } else { -step };
                            let next = range.value(position + step);
                            (next != value).then_some(next)
                        };
                        gesture::key_down(
                            &state,
                            event,
                            step,
                            default_value,
                            &on_change,
                            window,
                            cx,
                        );
                    }
                };
                d.cursor(CursorStyle::ResizeUpDown)
                    .track_focus(&focus_handle)
                    .on_key_down(on_key_down)
                    .on_mouse_down(MouseButton::Left, on_mouse_down)
                    .child(gesture::drag_listeners(state, on_change))
            })
            .child(dial);

        cell::frame(
            self.base,
            Some(knob.into_any_element()),
            self.label,
            self.readout,
            cx,
        )
        .when(disabled, |d| d.opacity(0.4))
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
    fn a_drag_of_one_point_is_a_two_hundredth_of_the_travel() {
        let range = KnobRange::logarithmic(20., 20_000.);
        // 1234.5 is at 0.5967 of the travel; one point up and down is 0.005 of it.
        let mut travel = Travel::new(-300., range.position(1234.5), TRAVEL);
        assert_eq!(travel.position(-300., false), None);
        assert_eq!(travel.position(-299., false).map(|p| range.value(p)), Some(1280.));
        assert_eq!(travel.position(-301., false).map(|p| range.value(p)), Some(1190.));
        // With shift a tenth of that.
        assert_eq!(travel.position(-301., true).map(|p| range.value(p)), Some(1190.));
        assert_eq!(travel.position(-311., true).map(|p| range.value(p)), Some(1150.));
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
