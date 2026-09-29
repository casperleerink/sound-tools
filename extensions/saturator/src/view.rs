//! The card of the saturator: the curve from what goes in to what comes out, with a handle at
//! its bend and the curves at its top, then Drive, Tone, Output and Mix. The rack gives the view
//! a [`CardFrame`]: the picker of the slot as the title, and the power and close icons.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of the handle is one
//! gesture and one undo step, a key step, a reset or a switch is one commit. The ranges and
//! the defaults come from the [`Parameter`]s of the crate. What is only about the interface is
//! here: the label, the unit, the travel of a knob and the name of the undo step.

use gpui::{Context, Entity, Point, SharedString, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use crate::{Curve, DRIVE, MIX, OUTPUT, Parameter, SaturatorState, TONE, auto_gain, transfer};

/// The name the rack puts on the card of a saturator.
pub const NAME: &str = "Saturator";

/// The width of the display: room for the four curves at its top. With two columns of cells
/// the card is 352 pt, as wide as the filter.
const DISPLAY_WIDTH: f32 = 200.;

/// Points of the curve across the display.
const CURVE_POINTS: usize = 96;

/// Registers the view of the `saturator` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(SaturatorView::new);
    devices.describe::<SaturatorState>(|_| DeviceLabel {
        key: SaturatorState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Decibels,
    /// A part of one, shown as a percentage.
    Part,
}

/// A knob of the card. A range that goes both ways from 0 has its arc start at the top.
struct Control {
    parameter: &'static Parameter,
    label: &'static str,
    undo_label: &'static str,
    unit: Unit,
}

const DRIVE_KNOB: Control = Control {
    parameter: &DRIVE,
    label: "Drive",
    undo_label: "Change drive",
    unit: Unit::Decibels,
};
const TONE_KNOB: Control = Control {
    parameter: &TONE,
    label: "Tone",
    undo_label: "Change tone",
    unit: Unit::Decibels,
};
const OUTPUT_KNOB: Control = Control {
    parameter: &OUTPUT,
    label: "Output",
    undo_label: "Change output",
    unit: Unit::Decibels,
};
const MIX_KNOB: Control = Control {
    parameter: &MIX,
    label: "Mix",
    undo_label: "Change mix",
    unit: Unit::Part,
};

/// Every knob, in the order of the card.
#[cfg(test)]
const KNOBS: [&Control; 4] = [&DRIVE_KNOB, &TONE_KNOB, &OUTPUT_KNOB, &MIX_KNOB];

/// The value of a segment and its label, for each curve.
const CURVES: [(Curve, &str, &str); 4] = [
    (Curve::Soft, "soft", "Soft"),
    (Curve::Tape, "tape", "Tape"),
    (Curve::Tube, "tube", "Tube"),
    (Curve::Clip, "clip", "Clip"),
];

/// A value with its unit, as a knob shows it: `6 dB`, `-3.5 dB`, `30%`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Part => format!("{}%", short(value * 100.0)),
    }
}

/// The automatic gain under the display, to a tenth of a dB: `Auto gain -4.2 dB`.
fn auto_gain_readout(state: &SaturatorState) -> String {
    let db = 20. * auto_gain(state.curve, state.drive_db).log10();
    format!("Auto gain {} dB", short((db * 10.).round() / 10.))
}

/// Where a sample from -1 to 1 is on the display, across or up, from 0 to 1.
fn place(sample: f32) -> f32 {
    ((sample + 1.) / 2.).clamp(0., 1.)
}

/// What comes out for every level that goes in, from -1 at the left to 1 at the right.
fn curve(state: &SaturatorState) -> Vec<Point<f32>> {
    (0..=CURVE_POINTS)
        .map(|step| {
            let x = step as f32 / CURVE_POINTS as f32;
            point(x, place(transfer(state, 2. * x - 1.)))
        })
        .collect()
}

/// The input level at the bend: where the drive takes it to full scale, and every curve has
/// bent. A drive of `d` dB puts it at `-d` dBFS.
fn bend_of(drive_db: f32) -> f32 {
    10_f32.powf(-drive_db / 20.)
}

/// The drive that puts the bend at an input level, to a tenth of a dB, inside its range.
fn drive_at(bend: f32) -> f32 {
    let drive_db = -20. * bend.max(f32::MIN_POSITIVE).log10();
    ((drive_db * 10.).round() / 10.).clamp(DRIVE.min, DRIVE.max)
}

pub struct SaturatorView {
    session: Entity<Session>,
    saturator: Instance<SaturatorState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of the handle.
    edit: ControlEdit,
}

impl SaturatorView {
    pub fn new(
        session: Entity<Session>,
        saturator: Instance<SaturatorState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.saturator.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.saturator.id() => {
                view.edit.finish(&view.session, cx);
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
            saturator,
            frame,
            edit: ControlEdit::default(),
        }
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut SaturatorState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, saturator) = (&self.session, &self.saturator);
        self.edit.apply(session, saturator, label, change, set, cx);
    }

    fn knob(
        &self,
        control: &'static Control,
        state: &SaturatorState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let parameter = control.parameter;
        let value = (parameter.get)(state);
        Knob::new(parameter.field)
            .range(KnobRange::linear(parameter.min, parameter.max))
            .value(value)
            .default_value(parameter.default)
            .bipolar(parameter.min < 0.)
            .label(control.label)
            .readout(readout(control.unit, value))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = control.parameter.set;
                view.change(control.undo_label, change, set, cx);
            }))
    }

    /// The handle at the bend, on the curve: sideways is drive. To the left the bend comes
    /// earlier, so more of the sound is bent.
    fn handle(&self, state: &SaturatorState, cx: &mut Context<Self>) -> Handle {
        let bend = bend_of(state.drive_db);
        let across = KnobRange::linear(-1., 1.);
        let x = Axis::new(across, bend, bend_of(DRIVE.default));
        let y = Axis::fixed(place(transfer(state, bend)));
        Handle::new("drive", x, y).on_change(weak_callback(
            cx,
            |view, change: ValueChange<Point<f32>>, cx| {
                let set = |state: &mut SaturatorState, at: Point<f32>| {
                    state.drive_db = drive_at(at.x);
                };
                view.change(DRIVE_KNOB.undo_label, change, set, cx);
            },
        ))
    }

    fn display(&self, state: &SaturatorState, cx: &mut Context<Self>) -> Display {
        let selected = CURVES.iter().find(|(curve, ..)| *curve == state.curve);
        let selected = selected.map_or("", |(_, value, _)| value);
        let curves = SegmentedControl::new("curve", selected)
            .options(CURVES.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = CURVES.iter().find(|(_, name, _)| *name == value.as_ref());
                if let Some((curve, ..)) = picked {
                    let set = |state: &mut SaturatorState, curve| state.curve = curve;
                    view.change("Change curve", ValueChange::Set(*curve), set, cx);
                }
            }));
        Display::new("display", DISPLAY_WIDTH)
            .curve(curve(state))
            // The sound as it came in, for the eye to hold the curve to.
            .dashed([point(0., 0.), point(1., 1.)])
            .grid(vec![place(0.)], vec![place(0.)])
            .handle(self.handle(state, cx))
            .caption(auto_gain_readout(state))
            .child(curves)
    }
}

impl Render for SaturatorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self
            .session
            .read(cx)
            .project()
            .state(&self.saturator)
            .copied()
        else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(&DRIVE_KNOB, cx))
                .bottom(knob(&OUTPUT_KNOB, cx)),
            Column::new()
                .top(knob(&TONE_KNOB, cx))
                .bottom(knob(&MIX_KNOB, cx)),
        ];
        let card = self.frame.card().display(self.display(&state, cx));
        let card = columns
            .into_iter()
            .fold(card, |card, column| card.column(column));
        card.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(readout(Unit::Decibels, 0.0), "0 dB");
        assert_eq!(readout(Unit::Decibels, -3.5), "-3.5 dB");
        assert_eq!(readout(Unit::Decibels, 12.26), "12.3 dB");
        assert_eq!(readout(Unit::Part, 0.3), "30%");
        let state = SaturatorState {
            curve: Curve::Clip,
            drive_db: 24.0,
            ..SaturatorState::default()
        };
        assert_eq!(auto_gain_readout(&state), "Auto gain -15.6 dB");
    }

    /// The defaults and both ends of every range, through the travel of its knob and back.
    #[test]
    fn every_knob_gives_the_ends_of_its_range_and_keeps_a_value_it_gave() {
        for control in KNOBS {
            let parameter = control.parameter;
            let range = KnobRange::linear(parameter.min, parameter.max);
            assert_eq!(range.value(0.0), parameter.min, "{}", parameter.field);
            assert_eq!(range.value(1.0), parameter.max, "{}", parameter.field);
            for value in [parameter.min, parameter.default, parameter.max] {
                let back = range.value(range.position(value));
                assert_eq!(back, value, "{}", parameter.field);
            }
        }
    }

    /// The handle is where the curve bends, and a drag of it gives the drive back.
    #[test]
    fn the_handle_gives_back_the_drive_it_shows() {
        for drive_db in [DRIVE.min, 0.5, DRIVE.default, 12.0, 30.1, DRIVE.max] {
            assert_eq!(drive_at(bend_of(drive_db)), drive_db);
        }
        assert_eq!(drive_at(2.0), DRIVE.min);
        assert_eq!(drive_at(-1.0), DRIVE.max);
    }
}
