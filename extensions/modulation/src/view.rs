//! The card of the modulation: the LFO over two cycles, the left side as the line and the right
//! side dashed, with the mode at its top; then Rate, Depth, Feedback and Mix, and behind expand
//! the spread. The rack gives the view a [`CardFrame`]: the picker of the slot as the title,
//! and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change goes
//! through the session, by [`ControlEdit`]: a drag of a knob or of a handle is one gesture and one
//! undo step, a key step, a reset or a switch is one commit. The ranges, the defaults and the
//! travel of each knob come from the [`Parameter`](crate::Parameter)s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the card
//! is expanded. A number that an automation lane of the track moves shows the value that plays, on
//! its knob and on the display, and does not drag ([`Lanes`]).

use std::f32::consts::TAU;

use gpui::{Context, Entity, Point, SharedString, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, ParameterKnob, hertz_readout, percent_readout, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Lanes, Session, Views, weak_callback};

use crate::{DEPTH, FEEDBACK, MIX, Mode, Modulation, ModulationState, RATE, SPREAD, sweep};

/// The name the rack puts on the card of a modulation.
pub const NAME: &str = "Modulation";

/// The width of the display. With it and two columns of cells the card is 352 pt, as wide as
/// the filter.
const DISPLAY_WIDTH: f32 = 200.;

/// Points of each line across the display.
const CURVE_POINTS: usize = 96;

/// Registers the view of the `modulation` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(ModulationView::new);
    devices.describe::<ModulationState>(|_| DeviceLabel {
        key: ModulationState::TOOL.into(),
        name: NAME.into(),
    });
}

/// A knob of the card.
type Control = ParameterKnob<ModulationState>;

const RATE_KNOB: Control = Control::new(&RATE, "Rate", "Change rate", hertz_readout);
const DEPTH_KNOB: Control = Control::new(&DEPTH, "Depth", "Change depth", percent_readout);
const FEEDBACK_KNOB: Control =
    Control::new(&FEEDBACK, "Feedback", "Change feedback", percent_readout);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);
const SPREAD_KNOB: Control = Control::new(&SPREAD, "Spread", "Change spread", degrees_readout);

/// Every knob, in the order of the card: shown, then hidden.
#[cfg(test)]
const KNOBS: [&Control; 5] = [
    &RATE_KNOB,
    &DEPTH_KNOB,
    &FEEDBACK_KNOB,
    &MIX_KNOB,
    &SPREAD_KNOB,
];

/// The value of a segment and its label, for each mode.
const MODES: [(Mode, &str, &str); 3] = [
    (Mode::Chorus, "chorus", "Chorus"),
    (Mode::Flanger, "flanger", "Flanger"),
    (Mode::Phaser, "phaser", "Phaser"),
];

/// A part of half a cycle, in degrees: `90°`.
fn degrees_readout(half_cycles: f32) -> String {
    format!("{}°", short(half_cycles * 180.0))
}

/// How far the LFO swings, in the unit of the mode: `7.37 – 19.5 ms`, `250 Hz – 4 kHz`.
fn caption(state: &ModulationState) -> String {
    let [low, high] = sweep(state);
    let hertz = |value: f32| match value < 1_000.0 {
        true => format!("{} Hz", short(value)),
        false => format!("{} kHz", short(value / 1_000.0)),
    };
    match state.mode {
        Mode::Chorus | Mode::Flanger => format!("{} – {} ms", short(low), short(high)),
        Mode::Phaser => format!("{} – {}", hertz(low), hertz(high)),
    }
}

/// Where the LFO is drawn, as places from 0 to 1, `y` up. Two cycles across, the left side
/// starting at 0, so its peaks are at an eighth and five eighths. The right side lags by half
/// the spread in cycles, so its first peak moves from an eighth to three eighths.
mod layout {
    use sound_ui::components::knob::KnobRange;

    pub const CYCLES: f32 = 2.;
    /// The middle of the swing, and how far depth 1 reaches from it, under the mode at the top.
    pub const MIDDLE: f32 = 0.4;
    pub const SWING: f32 = 0.3;
    /// Where the depth handle sits across: the second peak of the left side, which the spread
    /// handle never reaches.
    pub const DEPTH_AT: f32 = 0.625;

    /// Up and down is depth, placed so that the handle sits on the peak of the line.
    pub const DEPTH_TRAVEL: KnobRange = KnobRange::linear(-MIDDLE / SWING, (1. - MIDDLE) / SWING);
    /// Sideways is spread, placed so that the handle sits on the first peak of the right side.
    pub const SPREAD_TRAVEL: KnobRange = KnobRange::linear(-0.5, 3.5);

    /// The height of the peak at a depth.
    pub fn peak(depth: f32) -> f32 {
        MIDDLE + SWING * depth
    }
}

/// One side of the LFO across the display, `lag` cycles behind the left.
fn line(depth: f32, lag: f32) -> Vec<Point<f32>> {
    use layout::{CYCLES, MIDDLE, SWING};
    (0..=CURVE_POINTS)
        .map(|step| {
            let x = step as f32 / CURVE_POINTS as f32;
            let lfo = (TAU * (CYCLES * x - lag)).sin();
            point(x, MIDDLE + SWING * depth * lfo)
        })
        .collect()
}

pub struct ModulationView {
    session: Entity<Session>,
    modulation: Instance<ModulationState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of a handle.
    edit: ControlEdit,
    lanes: Entity<Lanes<ModulationState>>,
    /// Whether the card shows the spread. Interface state: not saved.
    expanded: bool,
}

impl ModulationView {
    pub fn new(
        session: Entity<Session>,
        modulation: Instance<ModulationState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.modulation.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.modulation.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let lanes = Lanes::follow(&session, modulation.id(), Modulation::AUTOMATION, cx);
        Self {
            session,
            modulation,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
        }
    }

    /// Shows or hides the spread, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut ModulationState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, modulation) = (&self.session, &self.modulation);
        self.edit.apply(session, modulation, label, change, set, cx);
    }

    fn knob(&self, control: Control, state: &ModulationState, cx: &mut Context<Self>) -> Knob {
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        control
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                view.change(control.undo_label, change, control.parameter.set, cx);
            }))
    }

    /// A handle that moves one value, on the `x` or the `y` axis, and stays put on the other.
    /// It edits what its knob edits, as one undo step with the same name.
    fn handle(
        &self,
        id: &'static str,
        control: Control,
        (x, y): (Axis, Axis),
        cx: &mut Context<Self>,
    ) -> Handle {
        let sideways = x.drags;
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        Handle::new(id, x, y)
            .automated(automated)
            .on_change(weak_callback(
                cx,
                move |view, change: ValueChange<Point<f32>>, cx| {
                    let set = |state: &mut ModulationState, place: Point<f32>| {
                        let value = if sideways { place.x } else { place.y };
                        (control.parameter.set)(state, control.clamp(value))
                    };
                    view.change(control.undo_label, change, set, cx);
                },
            ))
    }

    /// The LFO: the peak of the left side drags the depth, the first peak of the right side
    /// drags the spread.
    fn display(&self, state: &ModulationState, cx: &mut Context<Self>) -> Display {
        use layout::{DEPTH_AT, DEPTH_TRAVEL, SPREAD_TRAVEL, peak};
        let selected = MODES.iter().find(|(mode, ..)| *mode == state.mode);
        let selected = selected.map_or("", |(_, value, _)| value);
        let modes = SegmentedControl::new("mode", selected)
            .options(MODES.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = MODES.iter().find(|(_, name, _)| *name == value.as_ref());
                if let Some((mode, ..)) = picked {
                    let set = |state: &mut ModulationState, mode| state.mode = mode;
                    view.change("Change mode", ValueChange::Set(*mode), set, cx);
                }
            }));
        let depth = Axis::new(DEPTH_TRAVEL, state.depth, DEPTH.default);
        let depth = self.handle("depth", DEPTH_KNOB, (Axis::fixed(DEPTH_AT), depth), cx);
        let spread = Axis::new(SPREAD_TRAVEL, state.spread, SPREAD.default);
        let height = Axis::fixed(peak(state.depth));
        let spread = self.handle("spread", SPREAD_KNOB, (spread, height), cx);
        Display::new("display", DISPLAY_WIDTH)
            .curve(line(state.depth, 0.))
            .dashed(line(state.depth, 0.5 * state.spread))
            .zero_line(layout::MIDDLE)
            .handle(spread.hollow(true))
            .handle(depth)
            .caption(caption(state))
            .child(modes)
    }
}

impl Render for ModulationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.lanes.read(cx).state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(RATE_KNOB, cx))
                .bottom(knob(FEEDBACK_KNOB, cx)),
            Column::new()
                .top(knob(DEPTH_KNOB, cx))
                .bottom(knob(MIX_KNOB, cx)),
        ];
        let hidden = [Column::new().top(knob(SPREAD_KNOB, cx))];
        let expand = cx.listener(|view, _, _, cx| view.set_expanded(!view.expanded, cx));
        let card = self
            .frame
            .card()
            .expand(self.expanded, expand)
            .display(self.display(&state, cx));
        let card = columns
            .into_iter()
            .fold(card, |card, column| card.column(column));
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
    fn a_spread_reads_in_degrees() {
        assert_eq!(degrees_readout(0.5), "90°");
        assert_eq!(degrees_readout(1.0), "180°");
    }

    #[test]
    fn the_caption_is_the_sweep_in_the_unit_of_the_mode() {
        let at = |mode, depth| {
            caption(&ModulationState {
                mode,
                depth,
                ..ModulationState::default()
            })
        };
        assert_eq!(at(Mode::Chorus, 0.0), "12 – 12 ms");
        assert_eq!(at(Mode::Flanger, 1.0), "0.265 – 8.49 ms");
        assert_eq!(at(Mode::Phaser, 1.0), "250 Hz – 4 kHz");
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

    /// The depth handle is on the second peak of the left side, and the spread handle on the
    /// first peak of the right side, at every depth and spread. They never meet.
    #[test]
    fn the_handles_are_on_the_peaks_of_the_lines() {
        use layout::{DEPTH_AT, DEPTH_TRAVEL, SPREAD_TRAVEL, peak};
        let height_at = |line: &[Point<f32>], x: f32| {
            let index = (x * CURVE_POINTS as f32).round() as usize;
            line[index].y
        };
        for depth in [0.0, 0.3, 1.0] {
            for spread in [0.0, 0.5, 1.0] {
                let left = line(depth, 0.0);
                let right = line(depth, 0.5 * spread);
                let at_depth = DEPTH_TRAVEL.position(depth);
                assert!((at_depth - peak(depth)).abs() < 1e-5);
                assert!((height_at(&left, DEPTH_AT) - at_depth).abs() < 1e-5);
                let at_spread = SPREAD_TRAVEL.position(spread);
                assert!((height_at(&right, at_spread) - peak(depth)).abs() < 1e-3);
                assert!(DEPTH_AT - at_spread > 0.2, "{spread}");
            }
        }
    }
}
