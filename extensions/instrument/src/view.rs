//! The view of the synth: one control for each saved parameter. Whatever hosts it gives it the
//! surface and the name: the track panel of the arrangement puts it into a device card whose
//! own first row says "Synth" and is where another instrument is picked.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every
//! change goes through the session, by [`ControlEdit`]: a knob drag is one gesture and one undo
//! step, a key step, a reset or a waveform switch is one commit. The ranges and the defaults come from the
//! [`Parameter`]s of the crate. What is only about the interface is here: the label, the
//! unit, the travel of the knob and the name of the undo step.

use gpui::{App, Context, Entity, SharedString, Window, div, prelude::*, px};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, KnobScale, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ActiveTheme, ControlEdit, DeviceLabel, Devices, Session, Views};

use crate::{
    ATTACK, CUTOFF, DECAY, GAIN, Parameter, RELEASE, RESONANCE, SUSTAIN, SynthState, Waveform,
};

/// The name the rack puts on the card of a synth.
pub const NAME: &str = "Synth";

/// Registers the view of the `instrument.synth` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register(SynthView::new);
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
}

/// The groups after the oscillator, left to right: filter, envelope, output. Air parts them.
const GROUPS: [&[Control]; 3] = [
    &[
        Control::new(&CUTOFF, "Cutoff", "Change cutoff", Unit::Hertz),
        Control::new(&RESONANCE, "Resonance", "Change resonance", Unit::Part),
    ],
    &[
        Control::new(&ATTACK, "Attack", "Change attack", Unit::Seconds),
        Control::new(&DECAY, "Decay", "Change decay", Unit::Seconds),
        Control::new(&SUSTAIN, "Sustain", "Change sustain", Unit::Part),
        Control::new(&RELEASE, "Release", "Change release", Unit::Seconds),
    ],
    &[Control::new(&GAIN, "Gain", "Change gain", Unit::Part)],
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

/// The height of a knob, where the waveform switch sits too.
const KNOB_SIZE: f32 = 36.;

pub struct SynthView {
    session: Entity<Session>,
    synth: Instance<SynthState>,
    /// The gesture of a knob drag.
    edit: ControlEdit,
}

impl SynthView {
    pub fn new(
        session: Entity<Session>,
        synth: Instance<SynthState>,
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
            edit: ControlEdit::default(),
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
}

impl Render for SynthView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self.session.read(cx).project().state(&self.synth).copied() else {
            return div();
        };
        let theme = cx.theme();
        let muted = theme.gray_700;

        let selected = WAVEFORMS
            .iter()
            .find(|(waveform, ..)| *waveform == state.waveform);
        let selected = selected.map_or("", |(_, value, _)| value);
        let waveform = SegmentedControl::new("waveform", selected)
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
            }));
        // The switch sits where the knobs are, and its label where theirs are.
        let oscillator = div()
            .flex()
            .flex_col()
            .items_center()
            .child(div().h(px(KNOB_SIZE)).flex().items_center().child(waveform))
            .child(
                div()
                    .mt(px(8.))
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(muted)
                    .child("Waveform"),
            );
        let groups = GROUPS.iter().map(|group| {
            let knobs = group.iter().map(|control| self.knob(control, &state, cx));
            div().flex().gap(px(8.)).children(knobs.collect::<Vec<_>>())
        });

        // No title: the card of a rack says what the device is, because that is also where a
        // composer picks another one.
        div()
            .flex()
            .items_start()
            .gap(px(32.))
            .child(oscillator)
            .children(groups.collect::<Vec<_>>())
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
        for control in GROUPS.iter().flat_map(|group| group.iter()) {
            let (range, parameter) = (control.range(), control.parameter);
            assert_eq!(range.value(0.0), parameter.min, "{}", parameter.field);
            assert_eq!(range.value(1.0), parameter.max, "{}", parameter.field);
            for value in [parameter.min, parameter.default, parameter.max] {
                let back = range.value(range.position(value));
                assert_eq!(back, value, "{}", parameter.field);
            }
        }
    }
}
