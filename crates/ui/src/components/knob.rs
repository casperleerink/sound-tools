//! Knob: a dial in a cell, with its label and value under it. A bipolar knob, such as pan, draws
//! its arc from the top. It drags up and down; the rest of the gesture is in
//! [`gesture`](super::gesture), which the volume and the handles of a display share.
//!
//! Controlled: the caller owns the value, gives it on every render and hears a [`ValueChange`].
//! So a knob on saved state keeps no copy of it, and a value that changes from outside shows at
//! once, also during a drag. The knob keeps only its focus handle and the open drag, in element
//! state under its id.
//!
//! A knob with a [`Unit`] also takes a typed value: a click on its value, or enter while it has
//! the focus, puts a field where the dial is. Enter, or the focus going elsewhere, reads what
//! was typed in the unit the knob shows, as one [`ValueChange::Set`]; escape puts it back.
//!
//! An automated knob shows the value its lane plays, with the mark of
//! [`automated`](super::automated), and does not drag, step, reset or take a typed value.
//!
//! [`KnobRange`] maps the value to the travel of the knob, linear or logarithmic, and gives
//! values of three significant digits, so a readout and a saved file stay short.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, Context, CursorStyle, Div, ElementId, Entity, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, Pixels, SharedString, StyleRefinement, Subscription, Window, canvas, div,
    prelude::*, px,
};
use sound_core::Parameter;

use crate::components::automated;
use crate::components::cell::{self, CONTROL_HEIGHT};
use crate::components::gesture::{self, ChangeHandler, GestureState, Travel, ValueChange};
use crate::components::paint;
use crate::components::text_input::TextInput;
pub use crate::components::typed::Typed;
use crate::scale::ControlSize;
use crate::theme::ActiveTheme;

/// The arc runs from -135 to +135 degrees, like a hardware pot.
const SWEEP: f32 = 270.;
/// Points of pointer travel for the whole travel.
pub(crate) const TRAVEL: f32 = 200.;
/// What one arrow key moves, as a part of the travel. With shift it is a tenth of this.
pub const KEY_STEP: f32 = 0.02;
pub const FINE_KEY_STEP: f32 = KEY_STEP * gesture::FINE;

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

/// A number with three significant digits and no zeros at its end: `2`, `15.5`, `632`, and a
/// minus sign, not a hyphen, before a negative one: `−4.5`, as wide as the plus. What a
/// readout next to a knob shows, so a value at rest is short and a moving one does not jump
/// between widths.
pub fn short(value: f32) -> String {
    if value == 0. {
        return "0".into();
    }
    let decimals = (2 - value.abs().log10().floor() as i32).max(0) as usize;
    let text = format!("{value:.decimals$}");
    let text = match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.'),
        false => &text,
    };
    text.replace('-', "\u{2212}")
}

/// A number that goes either way, with its sign: `+3.5`, `−7`, `0`.
pub fn signed(value: f32) -> String {
    match value > 0. {
        true => format!("+{}", short(value)),
        false => short(value),
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

/// A pitch in semitones: `-7 st`, `0.25 st`.
pub fn semitones_readout(semitones: f32) -> String {
    format!("{} st", short(semitones))
}

/// A time in seconds: `5 ms`, `1.5 s`.
pub fn seconds_readout(seconds: f32) -> String {
    match seconds < 1. {
        true => format!("{} ms", short(seconds * 1_000.)),
        false => format!("{} s", short(seconds)),
    }
}

/// How a knob says a value and how it reads one that is typed on it. The two are one thing, so
/// a typed value is always read in the unit the knob shows.
#[derive(Clone, Copy)]
pub struct Unit {
    readout: fn(f32) -> String,
    typed: Typed,
}

impl Unit {
    pub const fn new(readout: fn(f32) -> String, typed: Typed) -> Self {
        Self { readout, typed }
    }

    /// The value with its unit, as the knob shows it.
    pub fn readout(&self, value: f32) -> String {
        (self.readout)(value)
    }

    pub const fn typed(&self) -> Typed {
        self.typed
    }

    pub const HERTZ: Self = Self::new(hertz_readout, Typed::Hertz);
    pub const DECIBELS: Self = Self::new(decibels_readout, Typed::Number);
    /// A part of one.
    pub const PERCENT: Self = Self::new(percent_readout, Typed::Per(100.));
    pub const MILLISECONDS: Self = Self::new(milliseconds_readout, Typed::Milliseconds);
    pub const SECONDS: Self = Self::new(seconds_readout, Typed::Seconds);
    pub const SEMITONES: Self = Self::new(semitones_readout, Typed::Number);
    /// From -1, left, to 1, right.
    pub const PAN: Self = Self::new(pan_readout, Typed::Pan);
    /// A number with no unit, such as the Q of a band.
    pub const NUMBER: Self = Self::new(short, Typed::Number);
}

/// A knob of a device card on one number of a record. The name of the knob, its range and its
/// default come from the [`Parameter`]; the card says the label, the name of the undo step and
/// the unit of the value. A const of the card, and `Copy`, so a callback can keep it.
pub struct ParameterKnob<S: 'static> {
    pub parameter: &'static Parameter<S>,
    pub label: &'static str,
    pub undo_label: &'static str,
    unit: Unit,
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
        unit: Unit,
    ) -> Self {
        Self {
            parameter,
            label,
            undo_label,
            unit,
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
        self.unit.readout(value)
    }

    /// The name and the value in `of`, as the handle of a display says what it moves:
    /// `Cutoff 1.2 kHz`.
    pub fn caption(&self, of: &S) -> String {
        format!("{} {}", self.label, self.readout((self.parameter.get)(of)))
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
            .typed(self.unit.typed)
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
    typed: Option<Typed>,
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
            typed: None,
            on_change: None,
        }
    }

    /// The knob takes a typed value, read as `typed` says: give how its readout reads.
    pub fn typed(mut self, typed: Typed) -> Self {
        self.typed = Some(typed);
        self
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

/// What a typed text does to the knob the field opened on.
type Taken = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// What a knob that takes a typed value keeps in element state under its id.
struct Typing {
    open: Option<OpenField>,
    /// Whether the knob held still at its last render: what was typed is then dropped.
    holds: Cell<bool>,
}

struct OpenField {
    input: Entity<TextInput>,
    /// What the field opened on: left as it is, nothing was typed.
    opened: String,
    /// Kept from the knob as it was then: a card may show another value under the same knob
    /// before the field closes, such as another band of an EQ.
    taken: Taken,
    /// The focus of the knob, which gets the keys back.
    knob: FocusHandle,
    _blur: Subscription,
}

impl Typing {
    /// Puts the field on the knob with `text` in it, all selected, and gives it the keys.
    fn open(
        &mut self,
        text: String,
        taken: Taken,
        knob: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| TextInput::new(cx).size(ControlSize::Sm));
        // The field calls these inside its own update, so it closes once that is over.
        let closes = |commit: bool| {
            let typing = cx.weak_entity();
            move |_: &str, window: &mut Window, cx: &mut App| {
                let typing = typing.clone();
                window.defer(cx, move |window, cx| {
                    typing
                        .update(cx, |typing, cx| typing.close(commit, window, cx))
                        .ok();
                });
            }
        };
        let (enter, escape) = (closes(true), closes(false));
        input.update(cx, |input, cx| {
            input.set_text(text.clone(), cx);
            input.select_all_text(cx);
            input.set_on_submit(enter);
            input.set_on_cancel(escape);
        });
        let focus = input.focus_handle(cx);
        // What was typed counts when the keys go elsewhere too, as on a press beside it.
        let blur = cx.on_blur(&focus, window, |typing, window, cx| {
            typing.close(true, window, cx);
        });
        window.focus(&focus, cx);
        self.open = Some(OpenField {
            input,
            opened: text,
            taken,
            knob,
            _blur: blur,
        });
        cx.notify();
    }

    fn close(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.open.take() else {
            return;
        };
        cx.notify();
        if field.input.focus_handle(cx).is_focused(window) {
            window.focus(&field.knob, cx);
        }
        let text = field.input.read(cx).text().to_string();
        if commit && !self.holds.get() && text != field.opened {
            (field.taken)(&text, window, cx);
        }
    }
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
            track: theme.fill_strong(),
            value: theme.text(),
            face: theme.fill(),
            ring: ring_shows.then_some(theme.lavender),
        };
        let (value, range, bipolar) = (self.value, self.range, self.bipolar);
        let position = range.position(value);
        let held = disabled || automated;
        let on_change = self.on_change;

        let typing = self.typed.zip(on_change.clone()).map(|(typed, on_change)| {
            let id = ElementId::NamedChild(Arc::new(self.id.clone()), "typed".into());
            let typing = window.use_keyed_state(id, cx, |_, _| Typing {
                open: None,
                holds: Cell::new(held),
            });
            typing.read(cx).holds.set(held);
            (typing, typed, on_change)
        });
        // The field stands where the dial is, over the label, and the value line is gone. A
        // knob that holds still shows none: the field then loses the keys, which closes it.
        let field = (typing.as_ref())
            .and_then(|(typing, ..)| typing.read(cx).open.as_ref())
            .filter(|_| !held)
            .map(|field| field.input.clone());
        if let Some(field) = field {
            let selector = self.id.clone();
            let field = div()
                .debug_selector(move || format!("knob-{selector}-typed"))
                .w(px(cell::control_width(1)))
                .child(field);
            let field = Some(field.into_any_element());
            return cell::frame(self.base, field, self.label, None, cell::CELL_WIDTH, cx);
        }
        // Opens the field on the value as it is now.
        let step = self.step;
        let types = typing.filter(|_| !held).map(|(typing, typed, on_change)| {
            let knob = focus_handle.clone();
            move |window: &mut Window, cx: &mut App| {
                let on_change = on_change.clone();
                let taken: Taken = Rc::new(move |text, window, cx| {
                    let Some(typed) = typed.value(text) else {
                        return;
                    };
                    let typed = match step {
                        Some(step) => (typed / step).round() * step,
                        None => typed,
                    };
                    let typed = typed.max(range.min).min(range.max);
                    if typed != value {
                        on_change(ValueChange::Set(typed), window, cx);
                    }
                });
                let (text, knob) = (typed.text(value), knob.clone());
                typing.update(cx, |typing, cx| {
                    typing.open(text, taken, knob, window, cx);
                });
            }
        });

        let dial = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| paint_dial(bounds, position, bipolar, &colors, window),
        )
        .size_full();

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
            .id(self.id.clone())
            .debug_selector(move || format!("knob-{selector}"))
            .relative()
            .size(px(DIAL))
            .when(disabled, |d| d.cursor_not_allowed())
            .when_some(on_change, |d, on_change| match held {
                true => d.child(gesture::held_listeners(state.clone(), on_change)),
                false => drags(d, dragged, &state, &focus_handle, on_change),
            })
            .when_some(types.clone(), |d, types| {
                d.on_key_down(move |event, window, cx| {
                    let enter = event.keystroke.key == "enter";
                    let plain = !event.keystroke.modifiers.modified();
                    // The mouse has a knob that is dragged.
                    if enter && plain && !state.read(cx).is_dragging() {
                        cx.stop_propagation();
                        types(window, cx);
                    }
                })
            })
            .when(automated, |d| {
                let d = d.track_focus(&focus_handle);
                automated::tooltip(d.on_key_down(|event, _, cx| gesture::held_key_down(event, cx)))
            })
            .child(dial)
            .children(mark);

        // A click on the value types it. The target reaches the foot of the cell.
        let value_target = types.filter(|_| self.readout.is_some()).map(|types| {
            let selector = self.id.clone();
            div()
                .id(ElementId::NamedChild(Arc::new(self.id), "value".into()))
                .debug_selector(move || format!("knob-{selector}-value"))
                .absolute()
                .top(px(cell::VALUE_LINE))
                .left_0()
                .w_full()
                .h(px(cell::ROW_HEIGHT - cell::VALUE_LINE))
                .cursor(CursorStyle::IBeam)
                .on_click(move |_, window, cx| types(window, cx))
        });

        cell::frame(
            self.base,
            Some(knob.into_any_element()),
            self.label,
            self.readout,
            cell::CELL_WIDTH,
            cx,
        )
        .children(value_target)
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
        assert_eq!(decibels_readout(-4.5), "−4.5 dB");
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
        assert_eq!(semitones_readout(-7.), "−7 st");
        assert_eq!(semitones_readout(0.01), "0.01 st");
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
