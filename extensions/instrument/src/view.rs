//! The card of the synth in a rack: the envelope as a display whose handles drag, the waveform
//! at the top of it, the main knobs next to it and the envelope knobs behind expand. The rack
//! gives the frame of the card, whose title says "Synth" and is where another instrument is
//! picked.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every
//! change goes through the session, by [`ControlEdit`]: a knob or a handle drag is one gesture
//! and one undo step, a key step, a reset or a waveform switch is one commit. A handle edits the
//! same field as its knob, under the same name in the history, and the knob is the way to that
//! value from the keys. The ranges and the defaults come from the [`Parameter`]s of the crate.
//! What is only about the interface is here: the label, the unit, the travel of the knob, the
//! name of the undo step and whether the card is expanded.

use gpui::{App, Context, Entity, Point, SharedString, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, KnobScale, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Session, Views};

use crate::{
    ATTACK, CUTOFF, DECAY, GAIN, Parameter, RELEASE, RESONANCE, SUSTAIN, SynthState, Waveform,
};

/// The name the rack puts on the card of a synth.
pub const NAME: &str = "Synth";

/// Registers the card of the `instrument.synth` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(SynthView::new);
    devices.describe::<SynthState>(|_| DeviceLabel {
        key: SynthState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Hertz,
    Seconds,
    /// A part of one, shown as a percentage.
    Part,
}

/// A knob of the view.
struct Control {
    parameter: &'static Parameter,
    label: &'static str,
    undo_label: &'static str,
    /// Frequencies and times are heard in ratios, so their knobs travel in ratios.
    scale: KnobScale,
    unit: Unit,
}

impl Control {
    const fn new(
        parameter: &'static Parameter,
        label: &'static str,
        undo_label: &'static str,
        unit: Unit,
    ) -> Self {
        let scale = match unit {
            Unit::Hertz | Unit::Seconds => KnobScale::Logarithmic,
            Unit::Part => KnobScale::Linear,
        };
        Self {
            parameter,
            label,
            undo_label,
            scale,
            unit,
        }
    }

    fn range(&self) -> KnobRange {
        KnobRange {
            min: self.parameter.min,
            max: self.parameter.max,
            scale: self.scale,
        }
    }

    /// A value as the parameter takes it: a handle may ask for one past its ends.
    fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.parameter.min, self.parameter.max)
    }
}

const CUTOFF_KNOB: Control = Control::new(&CUTOFF, "Cutoff", "Change cutoff", Unit::Hertz);
const RESONANCE_KNOB: Control =
    Control::new(&RESONANCE, "Resonance", "Change resonance", Unit::Part);
const GAIN_KNOB: Control = Control::new(&GAIN, "Gain", "Change gain", Unit::Part);
const ATTACK_KNOB: Control = Control::new(&ATTACK, "Attack", "Change attack", Unit::Seconds);
const DECAY_KNOB: Control = Control::new(&DECAY, "Decay", "Change decay", Unit::Seconds);
const SUSTAIN_KNOB: Control = Control::new(&SUSTAIN, "Sustain", "Change sustain", Unit::Part);
const RELEASE_KNOB: Control = Control::new(&RELEASE, "Release", "Change release", Unit::Seconds);

/// Every knob, for the test of their ranges.
#[cfg(test)]
const KNOBS: [&Control; 7] = [
    &CUTOFF_KNOB,
    &RESONANCE_KNOB,
    &GAIN_KNOB,
    &ATTACK_KNOB,
    &DECAY_KNOB,
    &SUSTAIN_KNOB,
    &RELEASE_KNOB,
];

/// The value of the segmented control and the label of each waveform.
const WAVEFORMS: [(Waveform, &str, &str); 2] = [
    (Waveform::Saw, "saw", "Saw"),
    (Waveform::Square, "square", "Square"),
];

/// A value with its unit, as the knob shows it: `632 Hz`, `2 kHz`, `5 ms`, `1.5 s`, `70%`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Hertz if value < 1_000.0 => format!("{} Hz", short(value)),
        Unit::Hertz => format!("{} kHz", short(value / 1_000.0)),
        Unit::Seconds if value < 1.0 => format!("{} ms", short(value * 1_000.0)),
        Unit::Seconds => format!("{} s", short(value)),
        Unit::Part => format!("{}%", short(value * 100.0)),
    }
}

/// The width of the display: the synth card is 352 pt, with two columns of cells.
const DISPLAY_WIDTH: f32 = 200.;

/// Where the envelope sits in its display, as places from 0 to 1, `y` up.
///
/// Each time has a zone of its own across, on the travel of its knob: any time from 1 ms to
/// 10 s shows, and its handle moves as its knob turns. A stage starts where the one before it
/// ends, so the axis of its handle is the knob's range moved along by that place.
mod envelope {
    use sound_ui::components::knob::KnobRange;

    /// Where the attack starts.
    pub const LEFT: f32 = 0.04;
    /// The zone of one time.
    pub const ZONE: f32 = 0.28;
    /// How long a held note is drawn at the sustain level.
    pub const HOLD: f32 = 0.1;
    /// Full level and silence, clear of the edges so a handle there can be taken.
    pub const TOP: f32 = 0.88;
    pub const BOTTOM: f32 = 0.08;

    /// The range of a time whose zone starts at `start`: `time.position(value)` of the knob,
    /// squeezed into the zone and moved to its start. A logarithmic range stays one when it is
    /// stretched and moved, with other ends.
    pub fn time_axis(time: KnobRange, start: f32) -> KnobRange {
        let ratio = time.max / time.min;
        let min = time.min * ratio.powf(-start / ZONE);
        KnobRange::logarithmic(min, min * ratio.powf(1. / ZONE))
    }

    /// The range of the sustain level, from silence at `BOTTOM` to full level at `TOP`.
    pub fn level_axis() -> KnobRange {
        let min = -BOTTOM / (TOP - BOTTOM);
        KnobRange::linear(min, min + 1. / (TOP - BOTTOM))
    }

    /// The places of the stages: the peak after the attack, the end of the decay, the end of
    /// the hold and the end of the release.
    pub fn stages(time: KnobRange, attack: f32, decay: f32, release: f32) -> [f32; 4] {
        let peak = LEFT + ZONE * time.position(attack);
        let decayed = peak + ZONE * time.position(decay);
        let held = decayed + HOLD;
        [peak, decayed, held, held + ZONE * time.position(release)]
    }
}

pub struct SynthView {
    session: Entity<Session>,
    synth: Instance<SynthState>,
    /// The title and the close icon the rack gives the card.
    frame: CardFrame,
    /// The gesture of a knob or handle drag.
    edit: ControlEdit,
    /// Whether the card shows the envelope knobs. Interface state: nothing saves it.
    expanded: bool,
}

impl SynthView {
    pub fn new(
        session: Entity<Session>,
        synth: Instance<SynthState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.synth.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.synth.id() => {
                view.end_drag(cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        Self {
            session,
            synth,
            frame,
            edit: ControlEdit::default(),
            expanded: false,
        }
    }

    /// A callback of a control. It holds the view weakly, as `cx.listener` does. With
    /// `cx.processor` the listeners of the last frame would keep a closed view alive for one
    /// more frame, and an open drag with it.
    fn callback<E>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, E, &mut Context<Self>) + 'static,
    ) -> impl Fn(E, &mut Window, &mut App) + 'static {
        let view = cx.weak_entity();
        move |event, _, cx| {
            // Released: there is nothing left to tell.
            view.update(cx, |view, cx| f(view, event, cx)).ok();
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.edit.finish(&self.session, cx);
    }

    fn on_knob(&mut self, control: &Control, change: ValueChange, cx: &mut Context<Self>) {
        let (session, synth) = (&self.session, &self.synth);
        let (label, set) = (control.undo_label, control.parameter.set);
        self.edit.apply(session, synth, label, change, set, cx);
    }

    fn knob(&self, control: &'static Control, state: &SynthState, cx: &mut Context<Self>) -> Knob {
        let value = (control.parameter.get)(state);
        Knob::new(control.parameter.field)
            .range(control.range())
            .value(value)
            .default_value(control.parameter.default)
            .label(control.label)
            .readout(readout(control.unit, value))
            .on_change(Self::callback(cx, move |view, change, cx| {
                view.on_knob(control, change, cx)
            }))
    }

    /// A handle that moves one time sideways, at a fixed height. It edits what its knob edits.
    fn time_handle(
        &self,
        control: &'static Control,
        (start, height): (f32, f32),
        state: &SynthState,
        cx: &mut Context<Self>,
    ) -> Handle {
        let parameter = control.parameter;
        let x = Axis::new(
            envelope::time_axis(control.range(), start),
            (parameter.get)(state),
            parameter.default,
        );
        let id = control.label.to_lowercase();
        Handle::new(SharedString::from(id), x, Axis::fixed(height)).on_change(Self::callback(
            cx,
            move |view, change: ValueChange<Point<f32>>, cx| {
                let (session, synth) = (&view.session, &view.synth);
                let set = |state: &mut SynthState, place: Point<f32>| {
                    (parameter.set)(state, control.clamp(place.x))
                };
                view.edit
                    .apply(session, synth, control.undo_label, change, set, cx);
            },
        ))
    }

    /// The envelope, with a handle at the end of each stage, and the waveform at its top.
    fn display(&self, state: &SynthState, cx: &mut Context<Self>) -> Display {
        use envelope::{BOTTOM, LEFT, TOP};
        let time = ATTACK_KNOB.range();
        let (attack, decay) = (state.attack_seconds, state.decay_seconds);
        let (sustain, release) = (state.sustain, state.release_seconds);
        let [peak, decayed, held, released] = envelope::stages(time, attack, decay, release);
        let level = BOTTOM + sustain.clamp(0., 1.) * (TOP - BOTTOM);
        let curve = [
            point(LEFT, BOTTOM),
            point(peak, TOP),
            point(decayed, level),
            point(held, level),
            point(released, BOTTOM),
        ];
        // The corner after the decay moves two values: its time sideways, the sustain level up
        // and down. One drag of it is one undo step.
        let corner = Handle::new(
            "decay",
            Axis::new(envelope::time_axis(time, peak), decay, DECAY.default),
            Axis::new(envelope::level_axis(), sustain, SUSTAIN.default),
        )
        .on_change(Self::callback(
            cx,
            |view, change: ValueChange<Point<f32>>, cx| {
                let (session, synth) = (&view.session, &view.synth);
                let set = |state: &mut SynthState, place: Point<f32>| {
                    state.decay_seconds = DECAY_KNOB.clamp(place.x);
                    state.sustain = SUSTAIN_KNOB.clamp(place.y);
                };
                let label = "Change decay and sustain";
                view.edit.apply(session, synth, label, change, set, cx);
            },
        ));
        let caption = format!(
            "A {} · D {} · S {} · R {}",
            readout(Unit::Seconds, attack),
            readout(Unit::Seconds, decay),
            readout(Unit::Part, sustain),
            readout(Unit::Seconds, release),
        );
        Display::new("envelope", DISPLAY_WIDTH)
            .curve(curve)
            .handle(self.time_handle(&ATTACK_KNOB, (LEFT, TOP), state, cx))
            .handle(corner)
            .handle(self.time_handle(&RELEASE_KNOB, (held, BOTTOM), state, cx))
            .caption(caption)
            .child(div().ml_auto().child(self.waveform(state, cx)))
    }

    fn waveform(&self, state: &SynthState, cx: &mut Context<Self>) -> SegmentedControl {
        let selected = WAVEFORMS
            .iter()
            .find(|(waveform, ..)| *waveform == state.waveform);
        let selected = selected.map_or("", |(_, value, _)| value);
        SegmentedControl::new("waveform", selected)
            .options(WAVEFORMS.map(|(_, value, label)| (value, label)))
            .on_change(Self::callback(cx, |view, value: SharedString, cx| {
                let picked = WAVEFORMS
                    .iter()
                    .find(|(_, name, _)| *name == value.as_ref());
                if let Some((waveform, ..)) = picked {
                    let change = ValueChange::Set(*waveform);
                    let (session, synth) = (&view.session, &view.synth);
                    let set = |state: &mut SynthState, waveform| state.waveform = waveform;
                    view.edit
                        .apply(session, synth, "Change waveform", change, set, cx);
                }
            }))
    }
}

impl Render for SynthView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self.session.read(cx).project().state(&self.synth).copied() else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(&CUTOFF_KNOB, cx))
                .bottom(knob(&GAIN_KNOB, cx)),
            Column::new().top(knob(&RESONANCE_KNOB, cx)),
        ];
        // Behind expand: the times and the level the handles of the display move, so the keys
        // reach every one of them. Read across as A, D, then S, R.
        let hidden = [
            Column::new()
                .top(knob(&ATTACK_KNOB, cx))
                .bottom(knob(&SUSTAIN_KNOB, cx)),
            Column::new()
                .top(knob(&DECAY_KNOB, cx))
                .bottom(knob(&RELEASE_KNOB, cx)),
        ];
        let expand = cx.listener(|view, _, _, cx| {
            view.expanded = !view.expanded;
            cx.notify();
        });
        let card = self
            .frame
            .card()
            .expand(self.expanded, expand)
            .display(self.display(&state, cx));
        let card = columns.into_iter().fold(card, |card, column| card.column(column));
        let card = hidden
            .into_iter()
            .fold(card, |card, column| card.hidden_column(column));
        card.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(readout(Unit::Hertz, 20.0), "20 Hz");
        assert_eq!(readout(Unit::Hertz, 632.0), "632 Hz");
        assert_eq!(readout(Unit::Hertz, 999.0), "999 Hz");
        assert_eq!(readout(Unit::Hertz, 1_000.0), "1 kHz");
        assert_eq!(readout(Unit::Hertz, 2_140.0), "2.14 kHz");
        assert_eq!(readout(Unit::Hertz, 20_000.0), "20 kHz");
        assert_eq!(readout(Unit::Seconds, 0.001), "1 ms");
        assert_eq!(readout(Unit::Seconds, 0.005), "5 ms");
        assert_eq!(readout(Unit::Seconds, 0.0155), "15.5 ms");
        assert_eq!(readout(Unit::Seconds, 0.3), "300 ms");
        assert_eq!(readout(Unit::Seconds, 1.0), "1 s");
        assert_eq!(readout(Unit::Seconds, 1.5), "1.5 s");
        assert_eq!(readout(Unit::Seconds, 10.0), "10 s");
        assert_eq!(readout(Unit::Part, 0.0), "0%");
        assert_eq!(readout(Unit::Part, 0.155), "15.5%");
        assert_eq!(readout(Unit::Part, 0.7), "70%");
        assert_eq!(readout(Unit::Part, 1.0), "100%");
    }

    #[test]
    fn a_value_written_by_hand_is_shown_short_too() {
        assert_eq!(readout(Unit::Hertz, 2_143.553), "2.14 kHz");
        assert_eq!(readout(Unit::Part, 0.123_456), "12.3%");
    }

    /// The defaults and both ends of every range, through the travel of its knob and back.
    #[test]
    fn every_knob_gives_the_ends_of_its_range_and_keeps_a_value_it_gave() {
        for control in KNOBS {
            let (range, parameter) = (control.range(), control.parameter);
            assert_eq!(range.value(0.0), parameter.min, "{}", parameter.field);
            assert_eq!(range.value(1.0), parameter.max, "{}", parameter.field);
            for value in [parameter.min, parameter.default, parameter.max] {
                let back = range.value(range.position(value));
                assert_eq!(back, value, "{}", parameter.field);
            }
        }
    }

    /// A handle at the place of a time gives that time back, with the digits its knob gives.
    #[test]
    fn a_time_handle_is_where_its_knob_says_and_gives_its_value_back() {
        let time = ATTACK_KNOB.range();
        for start in [envelope::LEFT, 0.3, 0.62] {
            let axis = envelope::time_axis(time, start);
            for value in [0.001, 0.005, 0.2, 1.5, 10.0] {
                let place = axis.position(value);
                let expected = start + envelope::ZONE * time.position(value);
                assert!((place - expected).abs() < 1e-4, "{start} {value}: {place}");
                assert!((axis.value(place) - value).abs() <= value * 1e-3, "{value}");
            }
        }
    }

    #[test]
    fn the_sustain_handle_runs_from_silence_to_full_level() {
        let level = envelope::level_axis();
        assert!((level.position(0.) - envelope::BOTTOM).abs() < 1e-6);
        assert!((level.position(1.) - envelope::TOP).abs() < 1e-6);
        assert_eq!(level.value(level.position(0.25)), 0.25);
    }

    /// The longest envelope still fits the display.
    #[test]
    fn every_envelope_fits_its_display() {
        let time = ATTACK_KNOB.range();
        let [_, _, _, end] = envelope::stages(time, 10., 10., 10.);
        assert!(end <= 1., "{end}");
    }
}
