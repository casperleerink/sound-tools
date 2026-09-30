//! The card of the Wavetable in a rack. Collapsed it is the wavetable of the first oscillator,
//! whose position a drag on it moves, with the position, the cutoff and the resonance of the
//! first filter, and the gain. Behind expand, in sections to the right, each after a hairline:
//! the rest of the first oscillator, the second oscillator with its own wavetable, the sub and
//! the unison, both filters on one display, the envelopes and the LFOs, each on a display with
//! a switch, the voicing, and the matrix. Every value has one control: the ones the collapsed
//! card shows are not shown again.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob, a slider or on a display is
//! one gesture and one undo step, a key step, a reset, a pick or a click is one commit. The
//! ranges and the defaults come from the [`Parameter`](sound_core::Parameter)s of the crate.
//! What is only about the interface is here: labels, units, the travel of a knob, the names of
//! the undo steps, whether the card is expanded and which envelope and LFO it shows.

mod choices;
mod drawing;
mod matrix;

use gpui::{Context, ElementId, Entity, Point, SharedString, Window, div, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::curves::{
    Adsr as Times, EnvelopeHandle, RESPONSE_CAPTION, envelope_display, lfo_line, resonance_travel,
    response_curve, response_decades, response_height, svf_gain,
};
use sound_ui::components::device_card::{CardFrame, Column, Section};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, pan_readout, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::select::Select;
use sound_ui::components::toggle::Toggle;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use choices::Choice;
pub use choices::{EnvelopeShown, LfoShown};
use drawing::POSITION_TRAVEL;

use crate::state::{
    Adsr, ENV_ATTACK, ENV_ATTACK_CURVE, ENV_DECAY, ENV_DECAY_CURVE, ENV_RELEASE, ENV_RELEASE_CURVE,
    ENV_SUSTAIN, FILTER_CUTOFF, FILTER_DRIVE, FILTER_RESONANCE, Filter, GAIN, GLIDE, LFO_RATE,
    LfoSettings, OSC_DETUNE, OSC_EFFECT_AMOUNT, OSC_GAIN, OSC_OCTAVE, OSC_PAN, OSC_POSITION,
    OSC_SEMITONE, Oscillator, POLYPHONY, SUB_GAIN, Sub, UNISON_AMOUNT, UNISON_VOICES, Unison,
    VoiceMode, Voicing,
};
use crate::{WavetableState, wavetable};

type Parameter<T> = sound_core::Parameter<T>;

/// The name the rack puts on the card.
pub const NAME: &str = "Wavetable";

/// The width of every display of the card.
const DISPLAY_WIDTH: f32 = 200.;

/// Registers the card of the `wavetable` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(WavetableView::new);
    devices.describe::<WavetableState>(|_| DeviceLabel {
        key: WavetableState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    /// A part of one, shown as a percentage.
    Part,
    Hertz,
    Seconds,
    Octaves,
    Semitones,
    Cents,
    Decibels,
    Pan,
    /// A whole number of something, such as voices.
    Count,
}

/// A value with its unit, as a knob shows it: `632 Hz`, `5 ms`, `70%`, `+1 oct`, `-7 ct`.
fn readout(unit: Unit, value: f32) -> String {
    let signed = |value: f32| match value > 0. {
        true => format!("+{}", short(value)),
        false => short(value),
    };
    match unit {
        Unit::Part => format!("{}%", short(value * 100.)),
        Unit::Hertz if value < 1_000. => format!("{} Hz", short(value)),
        Unit::Hertz => format!("{} kHz", short(value / 1_000.)),
        Unit::Seconds if value < 1. => format!("{} ms", short(value * 1_000.)),
        Unit::Seconds => format!("{} s", short(value)),
        Unit::Octaves => format!("{} oct", signed(value)),
        Unit::Semitones => format!("{} st", signed(value)),
        Unit::Cents => format!("{} ct", signed(value)),
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Pan => pan_readout(value),
        Unit::Count => format!("{value}"),
    }
}

/// A knob on one number of an object of the record, such as the position of an oscillator.
/// The same control serves every object of its type: `osc_1` and `osc_2` alike.
struct Control<T: 'static> {
    parameter: &'static Parameter<T>,
    label: &'static str,
    /// What the undo step calls it after the name of its object: `Change Osc 2 position`.
    noun: &'static str,
    unit: Unit,
    /// A value that means the part is off, which the knob says: a sub at gain 0.
    off: Option<f32>,
}

impl<T> Control<T> {
    const fn new(
        parameter: &'static Parameter<T>,
        label: &'static str,
        noun: &'static str,
        unit: Unit,
    ) -> Self {
        Self {
            parameter,
            label,
            noun,
            unit,
            off: None,
        }
    }

    const fn off_at(self, value: f32) -> Self {
        Self {
            off: Some(value),
            ..self
        }
    }

    /// Frequencies and times are heard in ratios, so their knobs travel in ratios, when their
    /// range allows it.
    fn range(&self) -> KnobRange {
        let Parameter { min, max, .. } = *self.parameter;
        match self.unit {
            Unit::Hertz | Unit::Seconds if min > 0. => KnobRange::logarithmic(min, max),
            _ => KnobRange::linear(min, max),
        }
    }

    /// Whole octaves, semitones and voices step by one.
    fn step(&self) -> Option<f32> {
        matches!(self.unit, Unit::Octaves | Unit::Semitones | Unit::Count).then_some(1.)
    }

    fn readout(&self, value: f32) -> String {
        match self.off {
            Some(off) if value == off => "Off".into(),
            _ => readout(self.unit, value),
        }
    }

    /// A value as the parameter takes it: a handle may ask for one past its ends.
    fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.parameter.min, self.parameter.max)
    }
}

const POSITION: Control<Oscillator> =
    Control::new(&OSC_POSITION, "Position", "position", Unit::Part);
const EFFECT_AMOUNT: Control<Oscillator> =
    Control::new(&OSC_EFFECT_AMOUNT, "Amount", "effect amount", Unit::Part);
const OCTAVE: Control<Oscillator> = Control::new(&OSC_OCTAVE, "Octave", "octave", Unit::Octaves);
const SEMITONE: Control<Oscillator> =
    Control::new(&OSC_SEMITONE, "Semitone", "semitone", Unit::Semitones);
const DETUNE: Control<Oscillator> = Control::new(&OSC_DETUNE, "Detune", "detune", Unit::Cents);
const LEVEL: Control<Oscillator> = Control::new(&OSC_GAIN, "Level", "level", Unit::Part);
const PAN: Control<Oscillator> = Control::new(&OSC_PAN, "Pan", "pan", Unit::Pan);
const SUB_LEVEL: Control<Sub> = Control::new(&SUB_GAIN, "Sub", "level", Unit::Part).off_at(0.);
const VOICES: Control<Unison> =
    Control::new(&UNISON_VOICES, "Unison", "voices", Unit::Count).off_at(1.);
const SPREAD: Control<Unison> = Control::new(&UNISON_AMOUNT, "Spread", "spread", Unit::Part);
const CUTOFF: Control<Filter> = Control::new(&FILTER_CUTOFF, "Cutoff", "cutoff", Unit::Hertz);
const RESONANCE: Control<Filter> =
    Control::new(&FILTER_RESONANCE, "Resonance", "resonance", Unit::Part);
const DRIVE: Control<Filter> = Control::new(&FILTER_DRIVE, "Drive", "drive", Unit::Decibels);
const ATTACK: Control<Adsr> = Control::new(&ENV_ATTACK, "Attack", "attack", Unit::Seconds);
const DECAY: Control<Adsr> = Control::new(&ENV_DECAY, "Decay", "decay", Unit::Seconds);
const SUSTAIN: Control<Adsr> = Control::new(&ENV_SUSTAIN, "Sustain", "sustain", Unit::Part);
const RELEASE: Control<Adsr> = Control::new(&ENV_RELEASE, "Release", "release", Unit::Seconds);
const ATTACK_CURVE: Control<Adsr> =
    Control::new(&ENV_ATTACK_CURVE, "Curve", "attack curve", Unit::Part);
const DECAY_CURVE: Control<Adsr> =
    Control::new(&ENV_DECAY_CURVE, "Curve", "decay curve", Unit::Part);
const RELEASE_CURVE: Control<Adsr> =
    Control::new(&ENV_RELEASE_CURVE, "Curve", "release curve", Unit::Part);
const RATE: Control<LfoSettings> = Control::new(&LFO_RATE, "Rate", "rate", Unit::Hertz);
const POLYPHONY_KNOB: Control<Voicing> = Control::new(&POLYPHONY, "Voices", "voices", Unit::Count);
const GLIDE_KNOB: Control<Voicing> =
    Control::new(&GLIDE, "Glide", "glide", Unit::Seconds).off_at(0.);
const OUTPUT_GAIN: Control<WavetableState> = Control::new(&GAIN, "Gain", "gain", Unit::Part);

/// An object of the record: an oscillator, a filter, an envelope. Its controls are told from
/// those of another object of its type by its key, and its undo steps name it.
struct Object<T: 'static> {
    /// What the composer calls it: `Osc 2`. Empty for the voicing and the record itself.
    title: &'static str,
    /// The start of the ids of its controls, `osc-2`, for tests too.
    key: &'static str,
    get: fn(&WavetableState) -> &T,
    get_mut: fn(&mut WavetableState) -> &mut T,
}

impl<T> Clone for Object<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Object<T> {}

impl<T> Object<T> {
    /// The id of a control on this object: `osc-2-position`.
    fn id(self, name: &str) -> SharedString {
        match self.key {
            "" => SharedString::from(name.to_string()),
            key => format!("{key}-{name}").into(),
        }
    }

    /// The name of an undo step on this object: `Change Osc 2 position`.
    fn undo(self, verb: &str, noun: &str) -> String {
        match self.title {
            "" => format!("{verb} {noun}"),
            title => format!("{verb} {title} {noun}"),
        }
    }
}

const OSC_1: Object<Oscillator> = Object {
    title: "Osc 1",
    key: "osc-1",
    get: |state| &state.osc_1,
    get_mut: |state| &mut state.osc_1,
};
const OSC_2: Object<Oscillator> = Object {
    title: "Osc 2",
    key: "osc-2",
    get: |state| &state.osc_2,
    get_mut: |state| &mut state.osc_2,
};
const SUB: Object<Sub> = Object {
    title: "Sub",
    key: "sub",
    get: |state| &state.sub,
    get_mut: |state| &mut state.sub,
};
const UNISON: Object<Unison> = Object {
    title: "Unison",
    key: "unison",
    get: |state| &state.unison,
    get_mut: |state| &mut state.unison,
};
const FILTER_1: Object<Filter> = Object {
    title: "Filter 1",
    key: "filter-1",
    get: |state| &state.filter_1,
    get_mut: |state| &mut state.filter_1,
};
const FILTER_2: Object<Filter> = Object {
    title: "Filter 2",
    key: "filter-2",
    get: |state| &state.filter_2,
    get_mut: |state| &mut state.filter_2,
};
const VOICING: Object<Voicing> = Object {
    title: "",
    key: "voicing",
    get: |state| &state.voicing,
    get_mut: |state| &mut state.voicing,
};
const OUTPUT: Object<WavetableState> = Object {
    title: "",
    key: "",
    get: |state| state,
    get_mut: |state| state,
};

impl EnvelopeShown {
    fn object(self) -> Object<Adsr> {
        match self {
            Self::Amp => Object {
                title: "Amp envelope",
                key: "amp-env",
                get: |state| &state.amp_env,
                get_mut: |state| &mut state.amp_env,
            },
            Self::Env2 => Object {
                title: "Env 2",
                key: "env-2",
                get: |state| &state.env_2,
                get_mut: |state| &mut state.env_2,
            },
            Self::Env3 => Object {
                title: "Env 3",
                key: "env-3",
                get: |state| &state.env_3,
                get_mut: |state| &mut state.env_3,
            },
        }
    }
}

impl LfoShown {
    fn object(self) -> Object<LfoSettings> {
        match self {
            Self::Lfo1 => Object {
                title: "LFO 1",
                key: "lfo-1",
                get: |state| &state.lfo_1,
                get_mut: |state| &mut state.lfo_1,
            },
            Self::Lfo2 => Object {
                title: "LFO 2",
                key: "lfo-2",
                get: |state| &state.lfo_2,
                get_mut: |state| &mut state.lfo_2,
            },
        }
    }
}

pub struct WavetableView {
    session: Entity<Session>,
    wavetable: Instance<WavetableState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob, a slider or on a display.
    edit: ControlEdit,
    /// Interface state, not saved: whether the card shows its sections, and which envelope and
    /// which LFO they show.
    expanded: bool,
    envelope: EnvelopeShown,
    lfo: LfoShown,
    /// The remove buttons of the routes and the add button, which the keys reach.
    remove_focus: Vec<gpui::FocusHandle>,
    add_focus: gpui::FocusHandle,
}

impl WavetableView {
    pub fn new(
        session: Entity<Session>,
        wavetable: Instance<WavetableState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.wavetable.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.wavetable.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let remove_focus = (0..crate::MAX_ROUTES)
            .map(|_| cx.focus_handle().tab_stop(true))
            .collect();
        Self {
            session,
            wavetable,
            frame,
            edit: ControlEdit::default(),
            expanded: false,
            envelope: EnvelopeShown::default(),
            lfo: LfoShown::default(),
            remove_focus,
            add_focus: cx.focus_handle().tab_stop(true),
        }
    }

    /// Shows or hides the sections, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    /// Which envelope the envelope section shows, as its switch picks.
    pub fn show_envelope(&mut self, envelope: EnvelopeShown, cx: &mut Context<Self>) {
        self.envelope = envelope;
        cx.notify();
    }

    /// Which LFO the LFO section shows, as its switch picks.
    pub fn show_lfo(&mut self, lfo: LfoShown, cx: &mut Context<Self>) {
        self.lfo = lfo;
        cx.notify();
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut WavetableState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, wavetable) = (&self.session, &self.wavetable);
        self.edit.apply(session, wavetable, label, change, set, cx);
    }

    fn knob<T>(
        &self,
        object: Object<T>,
        control: &'static Control<T>,
        state: &WavetableState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let parameter = control.parameter;
        let value = (parameter.get)((object.get)(state));
        let knob = Knob::new(object.id(parameter.field))
            .range(control.range())
            .value(value)
            .default_value(parameter.default)
            .bipolar(parameter.min < 0.)
            .label(control.label)
            .readout(control.readout(value))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let label = object.undo("Change", control.noun);
                let set = |state: &mut WavetableState, value| {
                    (parameter.set)((object.get_mut)(state), value)
                };
                view.change(&label, change, set, cx);
            }));
        match control.step() {
            Some(step) => knob.step(step),
            None => knob,
        }
    }

    /// A segmented control of a choice. `pick` hears the option picked.
    fn segments<C: Choice>(
        id: impl Into<ElementId>,
        value: C,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut Self, C, &mut Context<Self>) + 'static,
    ) -> SegmentedControl {
        SegmentedControl::new(id, value.key())
            .options(C::ALL.iter().map(|option| (option.key(), option.label())))
            .on_change(weak_callback(cx, move |view, key: SharedString, cx| {
                if let Some(option) = C::of(&key) {
                    pick(view, option, cx)
                }
            }))
    }

    /// A select of a choice. `pick` hears the option picked.
    fn select<C: Choice>(
        id: impl Into<ElementId>,
        value: C,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut Self, C, &mut Context<Self>) + 'static,
    ) -> Select {
        Select::new(id, value.key())
            .entries(C::rows())
            .on_change(weak_callback(cx, move |view, key: SharedString, cx| {
                if let Some(option) = C::of(&key) {
                    pick(view, option, cx)
                }
            }))
    }

    /// What a pick of a choice on the record does: one commit named `label`.
    fn picks<C: Choice>(
        label: String,
        set: impl Fn(&mut WavetableState, C) + 'static,
    ) -> impl Fn(&mut Self, C, &mut Context<Self>) + 'static {
        move |view, option, cx| view.change(&label, ValueChange::Set(option), &set, cx)
    }

    /// A cell with a toggle that turns something on or off, labelled with what it is.
    fn switch(
        id: SharedString,
        label: &'static str,
        what: String,
        on: bool,
        cx: &mut Context<Self>,
        set: impl Fn(&mut WavetableState, bool) + 'static,
    ) -> Cell {
        let text = if on { "On" } else { "Off" };
        let toggle = Toggle::new(id, text, on).on_change(weak_callback(cx, move |view, on, cx| {
            let label = format!("Turn {what} {}", if on { "on" } else { "off" });
            view.change(&label, ValueChange::Set(on), &set, cx);
        }));
        Cell::new(toggle).label(label)
    }

    /// The wavetable of an oscillator, with its table at the top. A drag up and down anywhere
    /// on it moves the position, as the Position knob does and under the same name.
    fn oscillator_display(
        &self,
        object: Object<Oscillator>,
        state: &WavetableState,
        cx: &mut Context<Self>,
    ) -> Display {
        let oscillator = (object.get)(state);
        let table = wavetable(oscillator.table).ok();
        let (lines, playing, caption) = match &table {
            Some(table) => {
                let (frame, count) = drawing::frame_number(table, oscillator.position);
                (
                    drawing::frames(table),
                    drawing::playing(table, oscillator.position),
                    format!("{} · frame {frame} of {count}", object.title),
                )
            }
            None => (Vec::new(), Vec::new(), object.title.to_string()),
        };
        let travel = Axis::new(
            POSITION_TRAVEL,
            oscillator.position,
            POSITION.parameter.default,
        );
        let area = Handle::new(object.id("position-area"), Axis::fixed(0.5), travel)
            .area()
            .on_change(weak_callback(
                cx,
                move |view, change: ValueChange<Point<f32>>, cx| {
                    let label = object.undo("Change", POSITION.noun);
                    let set = |state: &mut WavetableState, place: Point<f32>| {
                        (object.get_mut)(state).position = POSITION.clamp(place.y)
                    };
                    view.change(&label, change, set, cx);
                },
            ));
        let pick = Self::picks(object.undo("Change", "table"), move |state, table| {
            (object.get_mut)(state).table = table
        });
        let tables = Self::select(object.id("table"), oscillator.table, cx, pick).menu_width(200.);
        Display::new(object.id("display"), DISPLAY_WIDTH)
            .lines(lines)
            .curve(playing)
            .filled(false)
            .handle(area)
            .caption(caption)
            .child(tables)
    }

    /// The columns of an oscillator but its position: on, level, the effect and its amount,
    /// the tuning and the pan.
    fn oscillator_columns(
        &self,
        object: Object<Oscillator>,
        state: &WavetableState,
        cx: &mut Context<Self>,
    ) -> [Column; 4] {
        let oscillator = (object.get)(state);
        let on = Self::switch(
            object.id("on"),
            object.title,
            object.title.to_string(),
            oscillator.on,
            cx,
            move |state, on| (object.get_mut)(state).on = on,
        );
        let pick = Self::picks(object.undo("Change", "effect"), move |state, effect| {
            (object.get_mut)(state).effect = effect
        });
        let effect =
            Self::select(object.id("effect"), oscillator.effect, cx, pick).menu_width(120.);
        let mut knob = |control| self.knob(object, control, state, cx);
        [
            Column::new().top(on).bottom(knob(&LEVEL)),
            Column::new()
                .top(Cell::new(effect).label("Effect"))
                .bottom(knob(&EFFECT_AMOUNT)),
            Column::new().top(knob(&OCTAVE)).bottom(knob(&SEMITONE)),
            Column::new().top(knob(&DETUNE)).bottom(knob(&PAN)),
        ]
    }

    fn oscillator_1(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let columns = self.oscillator_columns(OSC_1, state, cx);
        columns.into_iter().fold(Section::new(), Section::column)
    }

    fn oscillator_2(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let position = Column::new().top(self.knob(OSC_2, &POSITION, state, cx));
        let columns = self.oscillator_columns(OSC_2, state, cx);
        let section = Section::new()
            .display(self.oscillator_display(OSC_2, state, cx))
            .column(position);
        columns.into_iter().fold(section, Section::column)
    }

    fn sub_and_unison(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let pick = Self::picks("Change sub octave".into(), |state, octave| {
            state.sub.octave = octave
        });
        let octave = Self::segments("sub-octave", state.sub.octave, cx, pick);
        Section::new()
            .column(
                Column::new()
                    .top(self.knob(SUB, &SUB_LEVEL, state, cx))
                    .bottom(Cell::new(octave).label("Octave")),
            )
            .column(
                Column::new()
                    .top(self.knob(UNISON, &VOICES, state, cx))
                    .bottom(self.knob(UNISON, &SPREAD, state, cx)),
            )
    }

    /// The handle of a filter on the response display: sideways its cutoff, up and down its
    /// resonance, in one undo step.
    fn filter_handle(object: Object<Filter>, filter: &Filter, cx: &mut Context<Self>) -> Handle {
        let x = Axis::new(CUTOFF.range(), filter.cutoff_hz, CUTOFF.parameter.default);
        let travel = resonance_travel(filter.slope);
        let y = Axis::new(travel, filter.resonance, RESONANCE.parameter.default);
        Handle::new(object.id("handle"), x, y).on_change(weak_callback(
            cx,
            move |view, change: ValueChange<Point<f32>>, cx| {
                let label = object.undo("Change", "cutoff and resonance");
                let set = |state: &mut WavetableState, place: Point<f32>| {
                    let filter = (object.get_mut)(state);
                    filter.cutoff_hz = CUTOFF.clamp(place.x);
                    // The travel reaches past both ends of the range, so that the handle can
                    // sit on the curve.
                    filter.resonance = RESONANCE.clamp(place.y);
                };
                view.change(&label, change, set, cx);
            },
        ))
    }

    /// Both filters on one response display: the first as the line, the second dashed with a
    /// hollow handle. A filter that is off lets the sound through: the first is flat, the
    /// second has no line and its handle is dim. The routing at the top.
    fn filter_display(&self, state: &WavetableState, cx: &mut Context<Self>) -> Display {
        let response = |filter: &Filter| {
            let filter = *filter;
            response_curve(move |hz| match filter.on {
                true => svf_gain(
                    filter.kind,
                    filter.slope,
                    filter.cutoff_hz,
                    filter.resonance,
                    hz,
                ),
                false => 1.,
            })
        };
        let pick = Self::picks("Change filter routing".into(), |state, routing| {
            state.routing = routing
        });
        let routing = Self::segments("routing", state.routing, cx, pick);
        let second = Self::filter_handle(FILTER_2, &state.filter_2, cx)
            .hollow(true)
            .dimmed(!state.filter_2.on);
        Display::new("filter-display", DISPLAY_WIDTH)
            .curve(response(&state.filter_1))
            .dashed(match state.filter_2.on {
                true => response(&state.filter_2),
                false => Vec::new(),
            })
            .grid(response_decades(), Vec::new())
            .zero_line(response_height(0.))
            .handle(second)
            .handle(Self::filter_handle(FILTER_1, &state.filter_1, cx))
            .caption(RESPONSE_CAPTION)
            .child(routing)
    }

    /// On, type, slope and drive of a filter.
    fn filter_columns(
        &self,
        object: Object<Filter>,
        state: &WavetableState,
        cx: &mut Context<Self>,
    ) -> [Column; 2] {
        let filter = (object.get)(state);
        let on = Self::switch(
            object.id("on"),
            object.title,
            object.title.to_string(),
            filter.on,
            cx,
            move |state, on| (object.get_mut)(state).on = on,
        );
        let pick = Self::picks(object.undo("Change", "type"), move |state, kind| {
            (object.get_mut)(state).kind = kind
        });
        let kind = Self::select(object.id("type"), filter.kind, cx, pick).menu_width(120.);
        let pick = Self::picks(object.undo("Change", "slope"), move |state, slope| {
            (object.get_mut)(state).slope = slope
        });
        let slope = Self::segments(object.id("slope"), filter.slope, cx, pick);
        [
            Column::new().top(on).bottom(Cell::new(kind).label("Type")),
            Column::new()
                .top(Cell::new(slope).label("Slope").value("dB / oct"))
                .bottom(self.knob(object, &DRIVE, state, cx)),
        ]
    }

    fn filters(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let [first_on, first_slope] = self.filter_columns(FILTER_1, state, cx);
        let [second_on, second_slope] = self.filter_columns(FILTER_2, state, cx);
        let second_tone = Column::new()
            .top(self.knob(FILTER_2, &CUTOFF, state, cx))
            .bottom(self.knob(FILTER_2, &RESONANCE, state, cx));
        Section::new()
            .display(self.filter_display(state, cx))
            .column(first_on)
            .column(first_slope)
            .column(second_on)
            .column(second_slope)
            .column(second_tone)
    }

    /// The envelope the switch shows, with the handles of the synth, its curves, and its
    /// knobs.
    fn envelopes(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let object = self.envelope.object();
        let envelope = (object.get)(state);
        let times = |envelope: &Adsr| Times {
            attack: envelope.attack_seconds,
            decay: envelope.decay_seconds,
            sustain: envelope.sustain,
            release: envelope.release_seconds,
        };
        let defaults = times(&Adsr::default());
        let curves = [
            envelope.attack_curve,
            envelope.decay_curve,
            envelope.release_curve,
        ];
        let on_change = weak_callback(
            cx,
            move |view, (handle, change): (EnvelopeHandle, ValueChange<Point<f32>>), cx| {
                let (noun, set): (_, fn(&mut Adsr, Point<f32>)) = match handle {
                    EnvelopeHandle::Attack => (ATTACK.noun, |envelope, place| {
                        envelope.attack_seconds = ATTACK.clamp(place.x)
                    }),
                    EnvelopeHandle::Decay => ("decay and sustain", |envelope, place| {
                        envelope.decay_seconds = DECAY.clamp(place.x);
                        envelope.sustain = SUSTAIN.clamp(place.y);
                    }),
                    EnvelopeHandle::Release => (RELEASE.noun, |envelope, place| {
                        envelope.release_seconds = RELEASE.clamp(place.x)
                    }),
                };
                let label = object.undo("Change", noun);
                let set = |state: &mut WavetableState, place| set((object.get_mut)(state), place);
                view.change(&label, change, set, cx);
            },
        );
        let values = times(envelope);
        let caption = format!(
            "A {} · D {} · S {} · R {}",
            readout(Unit::Seconds, values.attack),
            readout(Unit::Seconds, values.decay),
            readout(Unit::Part, values.sustain),
            readout(Unit::Seconds, values.release),
        );
        let switch = Self::segments("envelope-shown", self.envelope, cx, Self::show_envelope);
        let display = envelope_display(
            "envelope-display",
            DISPLAY_WIDTH,
            ATTACK.range(),
            (values, defaults),
            curves,
            on_change,
        )
        .caption(caption)
        .child(div().ml_auto().child(switch));
        let mut knob = |control| self.knob(object, control, state, cx);
        Section::new()
            .display(display)
            .column(Column::new().top(knob(&ATTACK)).bottom(knob(&ATTACK_CURVE)))
            .column(Column::new().top(knob(&DECAY)).bottom(knob(&DECAY_CURVE)))
            .column(Column::new().top(knob(&SUSTAIN)))
            .column(
                Column::new()
                    .top(knob(&RELEASE))
                    .bottom(knob(&RELEASE_CURVE)),
            )
    }

    /// The LFO the switch shows: two cycles of its shape, its shape at the top, and its rate,
    /// free or in time with the tempo.
    fn lfos(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let object = self.lfo.object();
        let lfo = *(object.get)(state);
        let switch = Self::segments("lfo-shown", self.lfo, cx, Self::show_lfo);
        let pick = Self::picks(object.undo("Change", "shape"), move |state, shape| {
            (object.get_mut)(state).shape = shape
        });
        let shape = Self::select(object.id("shape"), lfo.shape, cx, pick).menu_width(160.);
        let caption = match lfo.sync {
            true => format!(
                "{} · {}",
                lfo.division.label(),
                lfo.feel.label().to_lowercase()
            ),
            false => readout(Unit::Hertz, lfo.rate_hz),
        };
        let display = Display::new("lfo-display", DISPLAY_WIDTH)
            .curve(lfo_line(lfo.shape, 2., 0.4, 0.28))
            .zero_line(0.4)
            .caption(caption)
            .child(div().flex().gap_1().child(switch).child(shape));
        let sync = Self::switch(
            object.id("sync"),
            "Sync",
            format!("{} sync", object.title),
            lfo.sync,
            cx,
            move |state, sync| (object.get_mut)(state).sync = sync,
        );
        let retrigger = Self::switch(
            object.id("retrigger"),
            "Retrigger",
            format!("{} retrigger", object.title),
            lfo.retrigger,
            cx,
            move |state, on| (object.get_mut)(state).retrigger = on,
        );
        // Synced, the rate is a note of the tempo and its feel; free, a number of Hz.
        let rate = match lfo.sync {
            true => {
                let pick =
                    Self::picks(object.undo("Change", "division"), move |state, division| {
                        (object.get_mut)(state).division = division
                    });
                let division =
                    Self::select(object.id("division"), lfo.division, cx, pick).menu_width(96.);
                Cell::new(division).label("Rate").into_any_element()
            }
            false => self.knob(object, &RATE, state, cx).into_any_element(),
        };
        let feel = lfo.sync.then(|| {
            let pick = Self::picks(object.undo("Change", "feel"), move |state, feel| {
                (object.get_mut)(state).feel = feel
            });
            let feel = Self::select(object.id("feel"), lfo.feel, cx, pick).menu_width(120.);
            Cell::new(feel).label("Feel")
        });
        let feel_column = match feel {
            Some(feel) => Column::new().top(feel),
            None => Column::new(),
        };
        Section::new()
            .display(display)
            .column(Column::new().top(rate).bottom(sync))
            .column(feel_column.bottom(retrigger))
    }

    fn voicing(&self, state: &WavetableState, cx: &mut Context<Self>) -> Section {
        let mono = Self::switch(
            "voicing-mono".into(),
            "Mono",
            "mono".into(),
            state.voicing.mode == VoiceMode::Mono,
            cx,
            |state, mono| {
                state.voicing.mode = if mono {
                    VoiceMode::Mono
                } else {
                    VoiceMode::Poly
                }
            },
        );
        // Mono plays one voice, so its number does nothing then.
        let polyphony = Column::new();
        let polyphony = match state.voicing.mode {
            VoiceMode::Poly => polyphony.top(self.knob(VOICING, &POLYPHONY_KNOB, state, cx)),
            VoiceMode::Mono => polyphony,
        };
        Section::new()
            .column(
                Column::new()
                    .top(mono)
                    .bottom(self.knob(VOICING, &GLIDE_KNOB, state, cx)),
            )
            .column(polyphony)
    }
}

impl Render for WavetableView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self
            .session
            .read(cx)
            .project()
            .state(&self.wavetable)
            .cloned()
        else {
            return div().into_any_element();
        };
        let expand = cx.listener(|view, _, _, cx| view.set_expanded(!view.expanded, cx));
        let card = self
            .frame
            .card()
            .expand(self.expanded, expand)
            .display(self.oscillator_display(OSC_1, &state, cx))
            .column(
                Column::new()
                    .top(self.knob(OSC_1, &POSITION, &state, cx))
                    .bottom(self.knob(OUTPUT, &OUTPUT_GAIN, &state, cx)),
            )
            .column(
                Column::new()
                    .top(self.knob(FILTER_1, &CUTOFF, &state, cx))
                    .bottom(self.knob(FILTER_1, &RESONANCE, &state, cx)),
            );
        if !self.expanded {
            return card.into_any_element();
        }
        card.section(self.oscillator_1(&state, cx))
            .section(self.oscillator_2(&state, cx))
            .section(self.sub_and_unison(&state, cx))
            .section(self.filters(&state, cx))
            .section(self.envelopes(&state, cx))
            .section(self.lfos(&state, cx))
            .section(self.voicing(&state, cx))
            .section(self.matrix(&state, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readout_has_its_unit_and_a_sign_where_it_goes_either_way() {
        assert_eq!(readout(Unit::Part, 0.7), "70%");
        assert_eq!(readout(Unit::Hertz, 1_200.), "1.2 kHz");
        assert_eq!(readout(Unit::Seconds, 0.005), "5 ms");
        assert_eq!(readout(Unit::Octaves, 1.), "+1 oct");
        assert_eq!(readout(Unit::Octaves, 0.), "0 oct");
        assert_eq!(readout(Unit::Semitones, -7.), "-7 st");
        assert_eq!(readout(Unit::Cents, 7.), "+7 ct");
        assert_eq!(readout(Unit::Pan, -0.25), "25L");
        assert_eq!(readout(Unit::Count, 8.), "8");
        assert_eq!(SUB_LEVEL.readout(0.), "Off");
        assert_eq!(VOICES.readout(1.), "Off");
        assert_eq!(VOICES.readout(3.), "3");
    }

    /// The defaults and both ends of every range, through the travel of its knob and back.
    #[test]
    fn every_knob_gives_the_ends_of_its_range_and_keeps_a_value_it_gave() {
        fn check<T>(control: &Control<T>) {
            let (range, parameter) = (control.range(), control.parameter);
            assert_eq!(range.value(0.0), parameter.min, "{}", parameter.field);
            assert_eq!(range.value(1.0), parameter.max, "{}", parameter.field);
            for value in [parameter.min, parameter.default, parameter.max] {
                let back = range.value(range.position(value));
                assert_eq!(back, value, "{}", parameter.field);
            }
        }
        for control in [
            &POSITION,
            &EFFECT_AMOUNT,
            &OCTAVE,
            &SEMITONE,
            &DETUNE,
            &LEVEL,
            &PAN,
        ] {
            check(control);
        }
        for control in [&CUTOFF, &RESONANCE, &DRIVE] {
            check(control);
        }
        for control in [
            &ATTACK,
            &DECAY,
            &SUSTAIN,
            &RELEASE,
            &ATTACK_CURVE,
            &DECAY_CURVE,
            &RELEASE_CURVE,
        ] {
            check(control);
        }
        check(&SUB_LEVEL);
        check(&VOICES);
        check(&SPREAD);
        check(&RATE);
        check(&POLYPHONY_KNOB);
        check(&GLIDE_KNOB);
        check(&OUTPUT_GAIN);
    }

    #[test]
    fn the_undo_steps_name_the_object() {
        assert_eq!(OSC_2.undo("Change", "position"), "Change Osc 2 position");
        assert_eq!(OUTPUT.undo("Change", "gain"), "Change gain");
        assert_eq!(OSC_2.id("position"), "osc-2-position");
        assert_eq!(OUTPUT.id("gain"), "gain");
    }
}
