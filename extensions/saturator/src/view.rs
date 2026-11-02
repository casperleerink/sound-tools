//! The card of the saturator: the curve from the level that goes in to the level that comes
//! out, in dBFS, with a handle at its bend and the curves at its top, then Drive, Tone, Output
//! and Mix. The rack gives the view a [`CardFrame`]: the picker of the slot as the title, and
//! the power and close icons.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change goes
//! through the session, by [`DeviceEdit`]: a drag of a knob or of the handle is one gesture and
//! one undo step, a key step, a reset or a switch is one commit. The ranges, the defaults and the
//! travel of each knob come from the [`Parameter`](crate::Parameter)s of the crate. What is only
//! about the interface is here: the label, the unit and the name of the undo step. A number that an
//! automation lane of the track moves shows the value that plays, on its knob and on the display,
//! and does not drag ([`Lanes`](sound_ui::Lanes)).

use gpui::{Context, Entity, Point, SharedString, Window, div, point, prelude::*};
use sound_core::Instance;
use sound_dsp::amplitude;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    KnobRange, ParameterKnob, decibels_readout, percent_readout, short,
};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{DeviceEdit, DeviceView, Devices, OfferGroup, Session, Views, weak_callback};

use crate::{Curve, DRIVE, MIX, OUTPUT, Saturator, SaturatorState, TONE, auto_gain, transfer};

/// The name the rack puts on the card of a saturator.
pub const NAME: &str = "Saturator";

/// The width of the display: room for the four curves at its top. With two columns of cells
/// the card is 352 pt, as wide as the filter.
const DISPLAY_WIDTH: f32 = 200.;

/// Points of the curve across the display.
const CURVE_POINTS: usize = 96;

/// The display shows levels from here to there, in dBFS, the input across and the output up.
/// The bottom is under the bend at the most drive, and the top leaves room for a sound over
/// full scale and for the output turned up.
const LEVELS_DB: (f32, f32) = (-48., 6.);

/// The level a sine keeps at every drive, which the automatic gain holds to: a peak of 0.25.
const REFERENCE_DB: f32 = -12.;

/// Registers the view of the `saturator` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(SaturatorView::new);
    devices.built_in::<SaturatorState>(
        NAME,
        OfferGroup::Tone,
        "device-saturator",
        crate::EXTENSION,
        "This project does not load the saturator.",
    );
}

/// A knob of the card. A range that goes both ways from 0 has its arc start at the top.
type Control = ParameterKnob<SaturatorState>;

const DRIVE_KNOB: Control = Control::new(&DRIVE, "Drive", "Change drive", decibels_readout);
const TONE_KNOB: Control = Control::new(&TONE, "Tone", "Change tone", decibels_readout).bipolar();
const OUTPUT_KNOB: Control =
    Control::new(&OUTPUT, "Output", "Change output", decibels_readout).bipolar();
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);

/// The value of a segment and its label, for each curve.
const CURVES: [(Curve, &str, &str); 4] = [
    (Curve::Soft, "soft", "Soft"),
    (Curve::Tape, "tape", "Tape"),
    (Curve::Tube, "tube", "Tube"),
    (Curve::Clip, "clip", "Clip"),
];

/// The automatic gain under the display, to a tenth of a dB: `Auto gain -4.2 dB`.
fn auto_gain_readout(state: &SaturatorState) -> String {
    let db = 20. * auto_gain(state.curve, state.drive_db).log10();
    format!("Auto gain {} dB", short((db * 10.).round() / 10.))
}

/// The levels across and up the display.
const LEVELS: KnobRange = KnobRange::linear(LEVELS_DB.0, LEVELS_DB.1);

/// Where a level in dBFS is on the display, across or up, from 0 to 1.
fn place(db: f32) -> f32 {
    LEVELS.position(db)
}

/// The level in dBFS that comes out of a sine that goes in at `input_db`: the middle of its two
/// peaks, so a curve that leans shows the level it gives and not one side of it.
fn level_out(state: &SaturatorState, input_db: f32) -> f32 {
    let peak = amplitude(input_db);
    let out = (transfer(state, peak) - transfer(state, -peak)) / 2.;
    20. * out.max(1e-9).log10()
}

/// What comes out for every level that goes in.
fn curve(state: &SaturatorState) -> Vec<Point<f32>> {
    let (bottom, top) = LEVELS_DB;
    (0..=CURVE_POINTS)
        .map(|step| {
            let input_db = bottom + (top - bottom) * step as f32 / CURVE_POINTS as f32;
            point(place(input_db), place(level_out(state, input_db)))
        })
        .collect()
}

/// The input level at the bend, in dBFS: where the drive takes a sound to full scale, and
/// every curve has bent. A drive of `d` dB puts it at `-d` dBFS, so the handle travels in dB of
/// drive.
fn bend_of(drive_db: f32) -> f32 {
    -drive_db
}

/// The drive that puts the bend at an input level, to a tenth of a dB, inside its range.
fn drive_at(bend_db: f32) -> f32 {
    ((-bend_db * 10.).round() / 10.).clamp(DRIVE.min, DRIVE.max)
}

pub struct SaturatorView {
    device: DeviceEdit<SaturatorState>,
    frame: CardFrame,
}

impl SaturatorView {
    pub fn new(
        session: Entity<Session>,
        saturator: Instance<SaturatorState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            device: DeviceEdit::new(session, saturator, Saturator::AUTOMATION, cx),
            frame,
        }
    }

    /// The handle at the bend, on the curve: sideways is drive. To the left the bend comes
    /// earlier, so more of the sound is bent.
    fn handle(&self, state: &SaturatorState, cx: &mut Context<Self>) -> Handle {
        let bend = bend_of(state.drive_db);
        let x = Axis::new(LEVELS, bend, bend_of(DRIVE.default));
        let y = Axis::fixed(place(level_out(state, bend)));
        let automated = self.device.is_automated(DRIVE.field, cx);
        Handle::new("drive", x, y)
            .automated(automated)
            .on_change(weak_callback(
                cx,
                |view, change: ValueChange<Point<f32>>, cx| {
                    let set = |state: &mut SaturatorState, at: Point<f32>| {
                        state.drive_db = drive_at(at.x);
                    };
                    view.device.change(DRIVE_KNOB.undo_label, change, set, cx);
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
                    let change = ValueChange::Set(*curve);
                    view.device.change("Change curve", change, set, cx);
                }
            }));
        Display::new("display", DISPLAY_WIDTH)
            .curve(curve(state))
            // The sound as it came in, for the eye to hold the curve to.
            .dashed([point(0., 0.), point(1., 1.)])
            .grid(
                vec![place(REFERENCE_DB), place(0.)],
                vec![place(REFERENCE_DB)],
            )
            .zero_line(place(0.))
            .handle(self.handle(state, cx))
            .caption(auto_gain_readout(state))
            .child(curves)
    }
}

impl DeviceView for SaturatorView {
    type State = SaturatorState;

    fn device(&mut self) -> &mut DeviceEdit<SaturatorState> {
        &mut self.device
    }
}

impl Render for SaturatorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.device.state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.device.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(DRIVE_KNOB, cx))
                .bottom(knob(OUTPUT_KNOB, cx)),
            Column::new()
                .top(knob(TONE_KNOB, cx))
                .bottom(knob(MIX_KNOB, cx)),
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
    fn the_auto_gain_reads_to_a_tenth_of_a_db() {
        let state = SaturatorState {
            curve: Curve::Clip,
            drive_db: 24.0,
            ..SaturatorState::default()
        };
        assert_eq!(auto_gain_readout(&state), "Auto gain −15.6 dB");
    }

    /// The handle is where the curve bends, and a drag of it gives the drive back.
    #[test]
    fn the_handle_gives_back_the_drive_it_shows() {
        for drive_db in [DRIVE.min, 0.5, DRIVE.default, 12.0, 30.1, DRIVE.max] {
            assert_eq!(drive_at(bend_of(drive_db)), drive_db);
        }
        assert_eq!(drive_at(LEVELS_DB.1), DRIVE.min);
        assert_eq!(drive_at(LEVELS_DB.0), DRIVE.max);
    }
}
