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
//! - An automated knob shows the value its lane plays, with the mark of
//!   [`automated`](super::automated), and does not drag, step or reset.
//!
//! [`KnobRange`] maps the value to the travel of the knob, linear or logarithmic, and gives
//! values of three significant digits, so a readout and a saved file stay short.

use std::rc::Rc;

use gpui::{
    App, Bounds, CursorStyle, Div, ElementId, MouseButton, MouseDownEvent, Pixels, SharedString,
    StyleRefinement, Window, canvas, div, prelude::*, px,
};
use sound_core::Parameter;

use crate::components::automated;
use crate::components::cell::{self, CONTROL_HEIGHT};
use crate::components::gesture::{self, ChangeHandler, GestureState, Travel, ValueChange};
use crate::components::paint;
use crate::theme::ActiveTheme;

/// The arc runs from -135 to +135 degrees, like a hardware pot.
const SWEEP: f32 = 270.;
/// Points of pointer travel for the whole travel.
pub(crate) const TRAVEL: f32 = 200.;
/// What one arrow key moves, as a part of the travel. With shift it is a tenth of this.
const KEY_STEP: f32 = 0.02;
const FINE_KEY_STEP: f32 = KEY_STEP * gesture::FINE;

const DIAL: f32 = CONTROL_HEIGHT;
const TRACK_WIDTH: f32 = 2.5;
const FACE: f32 = 21.;
const POINTER_WIDTH: f32 = 2.;
const RING_WIDTH: f32 = 2.;
/// Where the mark of an automated knob sits: in the top right corner of the dial, clear of
/// the track, this far in from both edges.
const MARK_INSET: f32 = 1.;

/// The values of a knob and how they spread over its travel: the range of the core, which a
/// [`Parameter`](sound_core::Parameter) gives with [`KnobRange::of`], so the knob and an
/// automation lane agree. Its values have three significant digits.
pub type KnobRange = sound_core::ValueRange;

/// A pan as a knob shows it, from -1 (left) to 1 (right): `C`, `25L`, `100R`.
pub fn pan_readout(pan: f32) -> String {
    let percent = short(pan.abs() * 100.);
    if pan < 0. {
        format!("{percent}L")
    } else if pan > 0. {
        format!("{percent}R")
    } else {
        "C".to_string()
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

/// A frequency: `632 Hz`, `1.2 kHz`.
pub fn hertz_readout(hz: f32) -> String {
    match hz < 1_000. {
        true => format!("{} Hz", short(hz)),
        false => format!("{} kHz", short(hz / 1_000.)),
    }
}

/// A gain: `-4.5 dB`.
pub fn decibels_readout(db: f32) -> String {
    format!("{} dB", short(db))
}

/// A part of one as a percentage: `30%`.
pub fn percent_readout(part: f32) -> String {
    format!("{}%", short(part * 100.))
}

/// A time in milliseconds: `250 ms`, `1.5 s`.
pub fn milliseconds_readout(ms: f32) -> String {
    match ms < 1_000. {
        true => format!("{} ms", short(ms)),
        false => format!("{} s", short(ms / 1_000.)),
    }
}

/// A time in seconds: `5 ms`, `1.5 s`.
pub fn seconds_readout(seconds: f32) -> String {
    match seconds < 1. {
        true => format!("{} ms", short(seconds * 1_000.)),
        false => format!("{} s", short(seconds)),
    }
}

/// A knob of a device card on one number of a record. The name of the knob, its range and its
/// default come from the [`Parameter`]; the card says the label, the name of the undo step and
/// how the value reads. A const of the card, and `Copy`, so a callback can keep it.
pub struct ParameterKnob<S: 'static> {
    pub parameter: &'static Parameter<S>,
    pub label: &'static str,
    pub undo_label: &'static str,
    /// The value with its unit, such as [`hertz_readout`].
    readout: fn(f32) -> String,
    bipolar: bool,
    id: &'static str,
}

impl<S> Clone for ParameterKnob<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for ParameterKnob<S> {}

impl<S> ParameterKnob<S> {
    pub const fn new(
        parameter: &'static Parameter<S>,
        label: &'static str,
        undo_label: &'static str,
        readout: fn(f32) -> String,
    ) -> Self {
        Self {
            parameter,
            label,
            undo_label,
            readout,
            bipolar: false,
            id: parameter.field,
        }
    }

    /// The arc starts at the top, for a value with a middle such as a gain or a pan.
    pub const fn bipolar(self) -> Self {
        Self {
            bipolar: true,
            ..self
        }
    }

    /// Another name than the field, for a knob whose field another control in the same panel
    /// has too.
    pub const fn id(self, id: &'static str) -> Self {
        Self { id, ..self }
    }

    pub const fn range(&self) -> KnobRange {
        KnobRange::of(self.parameter)
    }

    /// A value as the parameter takes it: a handle may ask for one past its ends.
    pub fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.parameter.min, self.parameter.max)
    }

    pub fn readout(&self, value: f32) -> String {
        (self.readout)(value)
    }

    /// The knob at the value of `of`. The card adds whether a lane moves it and what a change
    /// does.
    pub fn knob(&self, of: &S) -> Knob {
        let value = (self.parameter.get)(of);
        Knob::new(self.id)
            .range(self.range())
            .value(value)
            .default_value(self.parameter.default)
            .bipolar(self.bipolar)
            .label(self.label)
            .readout(self.readout(value))
    }
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
    automated: bool,
    /// Values in whole steps of this, and an arrow key moves one step.
    step: Option<f32>,
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
            automated: false,
            step: None,
            on_change: None,
        }
    }

    /// Values in whole steps of `step` from 0, such as a note number: a drag gives the step
    /// nearest the pointer, and an arrow key moves one step, with shift too.
    pub fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
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

    /// An automation lane of the track moves the value: give the value it plays. The knob shows
    /// it with a mark and a tooltip, and the mouse and the keys change nothing.
    pub fn automated(mut self, automated: bool) -> Self {
        self.automated = automated;
        self
    }

    pub fn on_change(mut self, f: impl Fn(ValueChange, &mut Window, &mut App) + 'static) -> Self {
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
    paint::arc(
        window,
        centre,
        radius,
        TRACK_WIDTH,
        full,
        colors.track,
        false,
    );
    let start = if bipolar { 0. } else { angle(0.) };
    let value = (start, angle(position));
    paint::arc(
        window,
        centre,
        radius,
        TRACK_WIDTH,
        value,
        colors.value,
        true,
    );
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

/// How a control that drags one value on a [`KnobRange`] moves it: the knob and the slider.
#[derive(Clone, Copy)]
pub(crate) struct Dragged {
    pub range: KnobRange,
    pub value: f32,
    /// What a double click and backspace set.
    pub default: Option<f32>,
    /// Values in whole steps of this, and an arrow key moves one step.
    pub step: Option<f32>,
    /// Points of pointer travel for the whole travel.
    pub span: f32,
    /// A drag to the right raises the value, and not a drag up.
    pub sideways: bool,
}

/// The mouse and the keys of a control that drags one value: the gesture of
/// [`gesture`](super::gesture), with the arrows stepping a fiftieth of the travel, or one step.
pub(crate) fn drags(
    control: gpui::Stateful<Div>,
    dragged: Dragged,
    state: &gpui::Entity<GestureState<f32>>,
    focus_handle: &gpui::FocusHandle,
    on_change: ChangeHandler<f32>,
) -> gpui::Stateful<Div> {
    let Dragged {
        range,
        value,
        default,
        step: whole_step,
        span,
        sideways,
    } = dragged;
    let position = range.position(value);
    // A value on a whole step, inside the range.
    let stepped = move |value: f32| match whole_step {
        Some(step) => ((value / step).round() * step).clamp(range.min, range.max),
        None => value,
    };
    // The place on the travel as a value. A stepped one is snapped from the exact value: three
    // digits first would skip steps on a range of more than a thousand of them.
    let value_of = move |position: f32| match whole_step {
        Some(_) => stepped(range.exact(position)),
        None => range.value(position),
    };
    // Grows in the direction that raises the value.
    let along = move |pointer: gpui::Point<Pixels>| match sideways {
        true => f32::from(pointer.x),
        false => -f32::from(pointer.y),
    };
    let on_mouse_down = {
        let (state, on_change) = (state.clone(), on_change.clone());
        move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            let mut travel = Travel::new(along(event.position), position, span);
            let value_at = move |pointer: gpui::Point<Pixels>, fine| match travel
                .position(along(pointer), fine)
            {
                Some(position) => value_of(position),
                None => value,
            };
            gesture::press(
                &state, event, value, default, value_at, &on_change, window, cx,
            );
        }
    };
    let on_key_down = {
        let (state, on_change) = (state.clone(), on_change.clone());
        move |event: &gpui::KeyDownEvent, window: &mut Window, cx: &mut App| {
            let step = |up: bool, fine: bool| {
                let next = match whole_step {
                    Some(whole) => {
                        let whole = if up { whole } else { -whole };
                        stepped(value + whole)
                    }
                    None => {
                        let step = if fine { FINE_KEY_STEP } else { KEY_STEP };
                        let step = if up { step } else { -step };
                        range.value(position + step)
                    }
                };
                (next != value).then_some(next)
            };
            gesture::key_down(&state, event, Some(&step), default, &on_change, window, cx);
        }
    };
    let cursor = match sideways {
        true => CursorStyle::ResizeLeftRight,
        false => CursorStyle::ResizeUpDown,
    };
    control
        .cursor(cursor)
        .track_focus(focus_handle)
        .on_key_down(on_key_down)
        .on_mouse_down(MouseButton::Left, on_mouse_down)
        .child(gesture::drag_listeners(state.clone(), on_change))
}

impl RenderOnce for Knob {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id.clone(), cx, |_, cx| GestureState::new(cx));
        let (disabled, automated) = (self.disabled, self.automated);
        // An automated knob is still a tab stop, so the keys reach it, and a lane that arrives
        // on a focused knob leaves the focus where it is.
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

        let held = disabled || automated;
        let on_change = self.on_change;
        let marked = self.id.clone();
        let mark = automated.then(|| {
            let mark = automated::mark(DIAL - automated::MARK - MARK_INSET, MARK_INSET, cx);
            mark.debug_selector(move || format!("automated-{marked}"))
        });
        let dragged = Dragged {
            range,
            value,
            default: self.default_value,
            step: self.step,
            span: TRAVEL,
            sideways: false,
        };
        // For tests, which find the knob by its id: `knob-<id>`. Nothing in a normal build.
        let selector = self.id.clone();
        let knob = div()
            .id(self.id)
            .debug_selector(move || format!("knob-{selector}"))
            .relative()
            .size(px(DIAL))
            .when(disabled, |d| d.cursor_not_allowed())
            .when_some(on_change, |d, on_change| match held {
                true => d.child(gesture::held_listeners(state.clone(), on_change)),
                false => drags(d, dragged, &state, &focus_handle, on_change),
            })
            .when(automated, |d| {
                let d = d.track_focus(&focus_handle);
                automated::tooltip(d.on_key_down(|event, _, cx| gesture::held_key_down(event, cx)))
            })
            .child(dial)
            .children(mark);

        cell::frame(
            self.base,
            Some(knob.into_any_element()),
            self.label,
            self.readout,
            cell::CELL_WIDTH,
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
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(hertz_readout(0.05), "0.05 Hz");
        assert_eq!(hertz_readout(632.), "632 Hz");
        assert_eq!(hertz_readout(999.), "999 Hz");
        assert_eq!(hertz_readout(1_000.), "1 kHz");
        assert_eq!(hertz_readout(2_143.553), "2.14 kHz");
        assert_eq!(decibels_readout(0.), "0 dB");
        assert_eq!(decibels_readout(-4.5), "-4.5 dB");
        assert_eq!(decibels_readout(12.26), "12.3 dB");
        assert_eq!(percent_readout(0.), "0%");
        assert_eq!(percent_readout(0.123_456), "12.3%");
        assert_eq!(percent_readout(1.5), "150%");
        assert_eq!(milliseconds_readout(0.1), "0.1 ms");
        assert_eq!(milliseconds_readout(250.), "250 ms");
        assert_eq!(milliseconds_readout(1_500.), "1.5 s");
        assert_eq!(seconds_readout(0.001), "1 ms");
        assert_eq!(seconds_readout(0.0155), "15.5 ms");
        assert_eq!(seconds_readout(1.), "1 s");
        assert_eq!(seconds_readout(60.), "60 s");
    }

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
        assert_eq!(
            travel.position(-299., false).map(|p| range.value(p)),
            Some(1280.)
        );
        assert_eq!(
            travel.position(-301., false).map(|p| range.value(p)),
            Some(1190.)
        );
        // With shift a tenth of that.
        assert_eq!(
            travel.position(-301., true).map(|p| range.value(p)),
            Some(1190.)
        );
        assert_eq!(
            travel.position(-311., true).map(|p| range.value(p)),
            Some(1150.)
        );
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
