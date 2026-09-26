//! The card of the compressor: the transfer curve with the threshold and ratio handles, the
//! level now as a dot on it and the gain reduction as a bar, then Threshold, Ratio, Attack and
//! Release, and behind expand Knee, Makeup, Mix and Lookahead. The rack gives the view a
//! [`CardFrame`]: the picker of the slot as the title, and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of a handle is one
//! gesture and one undo step, a key step, a reset or a switch is one commit. The ranges and
//! the defaults come from the [`Parameter`]s of the crate. What is only about the interface is
//! here: the label, the unit, the travel of a knob, the name of the undo step and whether the
//! card is expanded.

use gpui::{Context, Entity, Point, SharedString, Window, div, point, prelude::*, px};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle, INSET_HEIGHT};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::meter::GainReduction;
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ActiveTheme, ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use crate::{
    ATTACK, CompressorState, KNEE, Lookahead, MAKEUP, MIX, Parameter, RATIO, RELEASE, THRESHOLD,
    reduction_db,
};

/// The name the rack puts on the card of a compressor.
pub const NAME: &str = "Compressor";

/// The width of the display. With it and two columns of cells the card is 288 pt, as DESIGN.md
/// gives the compressor.
const DISPLAY_WIDTH: f32 = 136.;

/// The curve shows levels from here to there, in dBFS, the input across and the output up:
/// the range of the threshold.
const LEVELS_DB: (f32, f32) = (-60., 0.);

/// The curve takes this part of the width, so that it is as wide as the display is tall and a
/// ratio of 1 is a diagonal. The gain reduction bar has the rest, at the right edge.
const CURVE_WIDTH: f32 = INSET_HEIGHT / DISPLAY_WIDTH;

/// Points of the curve across its width.
const CURVE_POINTS: usize = 60;

/// The dot of the level and the bar of the gain reduction, in points.
const LEVEL_DOT: f32 = 8.;
const BAR_INSET: f32 = 6.;

/// Registers the view of the `compressor` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(CompressorView::new);
    devices.describe::<CompressorState>(|_| DeviceLabel {
        key: CompressorState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Decibels,
    Ratio,
    Milliseconds,
    /// A part of one, shown as a percentage.
    Part,
}

/// A knob of the card.
struct Control {
    parameter: &'static Parameter,
    label: &'static str,
    undo_label: &'static str,
    scale: KnobRange,
    unit: Unit,
}

impl Control {
    /// Ratios and times are heard in ratios, so their knobs travel in ratios.
    const fn new(
        parameter: &'static Parameter,
        label: &'static str,
        undo_label: &'static str,
        unit: Unit,
    ) -> Self {
        let scale = match unit {
            Unit::Ratio | Unit::Milliseconds => {
                KnobRange::logarithmic(parameter.min, parameter.max)
            }
            Unit::Decibels | Unit::Part => KnobRange::linear(parameter.min, parameter.max),
        };
        Self {
            parameter,
            label,
            undo_label,
            scale,
            unit,
        }
    }
}

const THRESHOLD_KNOB: Control = Control::new(
    &THRESHOLD,
    "Threshold",
    "Change threshold",
    Unit::Decibels,
);
const RATIO_KNOB: Control = Control::new(&RATIO, "Ratio", "Change ratio", Unit::Ratio);
const ATTACK_KNOB: Control =
    Control::new(&ATTACK, "Attack", "Change attack", Unit::Milliseconds);
const RELEASE_KNOB: Control =
    Control::new(&RELEASE, "Release", "Change release", Unit::Milliseconds);
const KNEE_KNOB: Control = Control::new(&KNEE, "Knee", "Change knee", Unit::Decibels);
const MAKEUP_KNOB: Control = Control::new(&MAKEUP, "Makeup", "Change makeup", Unit::Decibels);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", Unit::Part);

/// Every knob, in the order of the card: shown, then hidden.
#[cfg(test)]
const KNOBS: [&Control; 7] = [
    &THRESHOLD_KNOB,
    &RATIO_KNOB,
    &ATTACK_KNOB,
    &RELEASE_KNOB,
    &KNEE_KNOB,
    &MAKEUP_KNOB,
    &MIX_KNOB,
];

/// The value of a segment for each lookahead.
const LOOKAHEADS: [(Lookahead, &str); 3] = [
    (Lookahead::Off, "0"),
    (Lookahead::One, "1"),
    (Lookahead::Ten, "10"),
];

/// A value with its unit, as a knob shows it: `-18 dB`, `4:1`, `10 ms`, `1.2 s`, `30%`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Ratio => format!("{}:1", short(value)),
        Unit::Milliseconds if value < 1_000.0 => format!("{} ms", short(value)),
        Unit::Milliseconds => format!("{} s", short(value / 1_000.0)),
        Unit::Part => format!("{}%", short(value * 100.0)),
    }
}

/// The gain reduction under the display, to a tenth of a dB: `GR 0 dB`, `GR -6.8 dB`.
fn reduction_readout(db: f32) -> String {
    let tenths = (db * 10.).round() / 10.;
    match tenths > 0. {
        true => format!("GR -{} dB", short(tenths)),
        false => "GR 0 dB".into(),
    }
}

/// Where a level in dBFS is across the display, from 0 at the left to [`CURVE_WIDTH`].
fn across(db: f32) -> f32 {
    let (bottom, top) = LEVELS_DB;
    CURVE_WIDTH * ((db - bottom) / (top - bottom)).clamp(0., 1.)
}

/// Where a level in dBFS is up the display, from 0 at the bottom to 1 at the top.
fn up(db: f32) -> f32 {
    let (bottom, top) = LEVELS_DB;
    ((db - bottom) / (top - bottom)).clamp(0., 1.)
}

/// The levels across the whole width of the display: past the right end of the curve are
/// levels over 0 dBFS, which the threshold does not reach.
fn threshold_travel() -> KnobRange {
    let (bottom, top) = LEVELS_DB;
    KnobRange::linear(bottom, bottom + (top - bottom) / CURVE_WIDTH)
}

/// The transfer curve: what comes out for each level that goes in, before makeup and mix.
fn curve(state: &CompressorState) -> Vec<Point<f32>> {
    let (bottom, top) = LEVELS_DB;
    (0..=CURVE_POINTS)
        .map(|step| {
            let input = bottom + (top - bottom) * step as f32 / CURVE_POINTS as f32;
            point(across(input), up(input - reduction_db(state, input)))
        })
        .collect()
}

/// The ratio handle sits on the curve at its right end, at an input of 0 dBFS, where the output
/// is `threshold (1 - 1 / ratio)`: a straight line in `1 / ratio` over the height. So the handle
/// moves `1 / ratio`, and its travel is that line stretched over the height, from what puts it
/// at the bottom to 1 at the top. `None` when the threshold is so close to 0 dBFS that the line
/// above it has no length to drag.
fn inverse_ratio_travel(threshold_db: f32) -> Option<KnobRange> {
    let (bottom, _) = LEVELS_DB;
    (threshold_db < -1.).then(|| KnobRange::linear(1. - bottom / threshold_db, 1.))
}

/// What the compressor did since the card last looked: the peak of what came in and the most it
/// turned down, both in dB. The view reads them from the audio thread once per poll.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Reading {
    level_db: f32,
    reduction_db: f32,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            level_db: f32::NEG_INFINITY,
            reduction_db: 0.,
        }
    }
}

pub struct CompressorView {
    session: Entity<Session>,
    compressor: Instance<CompressorState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of a handle.
    edit: ControlEdit,
    /// Whether the card shows knee, makeup, mix and lookahead. Interface state: not saved.
    expanded: bool,
    reading: Reading,
}

impl CompressorView {
    pub fn new(
        session: Entity<Session>,
        compressor: Instance<CompressorState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.compressor.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.compressor.id() => {
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
            compressor,
            frame,
            edit: ControlEdit::default(),
            expanded: false,
            reading: Reading::default(),
        }
    }

    /// Shows or hides knee, makeup, mix and lookahead, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut CompressorState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, compressor) = (&self.session, &self.compressor);
        self.edit.apply(session, compressor, label, change, set, cx);
    }

    fn knob(
        &self,
        control: &'static Control,
        state: &CompressorState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let value = (control.parameter.get)(state);
        Knob::new(control.parameter.field)
            .range(control.scale)
            .value(value)
            .default_value(control.parameter.default)
            .label(control.label)
            .readout(readout(control.unit, value))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = control.parameter.set;
                view.change(control.undo_label, change, set, cx);
            }))
    }

    /// The handle at the threshold, on the curve: sideways is threshold.
    fn threshold_handle(&self, state: &CompressorState, cx: &mut Context<Self>) -> Handle {
        let threshold = state.threshold_db;
        let x = Axis::new(threshold_travel(), threshold, THRESHOLD.default);
        let y = Axis::fixed(up(threshold - reduction_db(state, threshold)));
        Handle::new("threshold", x, y).on_change(weak_callback(
            cx,
            |view, change: ValueChange<Point<f32>>, cx| {
                // The travel reaches past 0 dBFS, the end of the curve.
                let set = |state: &mut CompressorState, at: Point<f32>| {
                    state.threshold_db = at.x.clamp(THRESHOLD.min, THRESHOLD.max);
                };
                view.change(THRESHOLD_KNOB.undo_label, change, set, cx);
            },
        ))
    }

    /// The handle at the end of the line above the threshold: up and down is ratio.
    fn ratio_handle(&self, state: &CompressorState, cx: &mut Context<Self>) -> Handle {
        let (_, top) = LEVELS_DB;
        let x = Axis::fixed(across(top));
        let y = match inverse_ratio_travel(state.threshold_db) {
            Some(travel) => Axis::new(travel, 1. / state.ratio, 1. / RATIO.default),
            None => Axis::fixed(up(top - reduction_db(state, top))),
        };
        Handle::new("ratio", x, y)
            .hollow(true)
            .on_change(weak_callback(
                cx,
                |view, change: ValueChange<Point<f32>>, cx| {
                    // The travel reaches below the steepest ratio, so that the handle can sit
                    // on the curve. Kept to two decimals, as a knob keeps three digits.
                    let set = |state: &mut CompressorState, at: Point<f32>| {
                        let ratio = (1. / at.y.max(1. / RATIO.max)).clamp(RATIO.min, RATIO.max);
                        state.ratio = (ratio * 100.).round() / 100.;
                    };
                    view.change(RATIO_KNOB.undo_label, change, set, cx);
                },
            ))
    }

    fn display(&self, state: &CompressorState, cx: &mut Context<Self>) -> Display {
        let theme = cx.theme();
        let Reading {
            level_db,
            reduction_db,
        } = self.reading;
        let (bottom, _) = LEVELS_DB;
        // The level now, where it is on the curve: what came in, and that less the reduction.
        let level = (level_db > bottom).then(|| {
            let (x, y) = (across(level_db), up(level_db - reduction_db));
            div()
                .absolute()
                .left(px(x * DISPLAY_WIDTH - LEVEL_DOT / 2.))
                .top(px((1. - y) * INSET_HEIGHT - LEVEL_DOT / 2.))
                .size(px(LEVEL_DOT))
                .rounded_full()
                .bg(theme.green)
        });
        let bar = GainReduction::new(reduction_db)
            .absolute()
            .top(px(BAR_INSET))
            .right(px(BAR_INSET))
            .h(px(INSET_HEIGHT - 2. * BAR_INSET));
        Display::new("display", DISPLAY_WIDTH)
            .curve(curve(state))
            .grid(vec![across(state.threshold_db)], Vec::new())
            .handle(self.threshold_handle(state, cx))
            .handle(self.ratio_handle(state, cx))
            .caption(reduction_readout(reduction_db))
            .children(level)
            .child(bar)
    }

    fn lookahead(&self, state: &CompressorState, cx: &mut Context<Self>) -> Cell {
        let selected = LOOKAHEADS.iter().find(|(value, _)| *value == state.lookahead);
        let selected = selected.map_or("", |(_, label)| label);
        let segments = SegmentedControl::new("lookahead", selected)
            .options(LOOKAHEADS.map(|(_, label)| (label, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = LOOKAHEADS.iter().find(|(_, label)| *label == value.as_ref());
                if let Some((lookahead, _)) = picked {
                    let set = |state: &mut CompressorState, lookahead| state.lookahead = lookahead;
                    view.change("Change lookahead", ValueChange::Set(*lookahead), set, cx);
                }
            }));
        Cell::new(segments).label("Lookahead").value("ms")
    }
}

impl Render for CompressorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self
            .session
            .read(cx)
            .project()
            .state(&self.compressor)
            .copied()
        else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(&THRESHOLD_KNOB, cx))
                .bottom(knob(&ATTACK_KNOB, cx)),
            Column::new()
                .top(knob(&RATIO_KNOB, cx))
                .bottom(knob(&RELEASE_KNOB, cx)),
        ];
        let hidden = [
            Column::new()
                .top(knob(&KNEE_KNOB, cx))
                .bottom(knob(&MAKEUP_KNOB, cx)),
            Column::new()
                .top(knob(&MIX_KNOB, cx))
                .bottom(self.lookahead(&state, cx)),
        ];
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
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(readout(Unit::Decibels, -18.0), "-18 dB");
        assert_eq!(readout(Unit::Ratio, 4.0), "4:1");
        assert_eq!(readout(Unit::Ratio, 2.5), "2.5:1");
        assert_eq!(readout(Unit::Milliseconds, 0.1), "0.1 ms");
        assert_eq!(readout(Unit::Milliseconds, 120.0), "120 ms");
        assert_eq!(readout(Unit::Milliseconds, 1_200.0), "1.2 s");
        assert_eq!(readout(Unit::Part, 0.3), "30%");
        assert_eq!(reduction_readout(0.0), "GR 0 dB");
        assert_eq!(reduction_readout(0.04), "GR 0 dB");
        assert_eq!(reduction_readout(6.83), "GR -6.8 dB");
    }

    /// The defaults and both ends of every range, through the travel of its knob and back.
    #[test]
    fn every_knob_gives_the_ends_of_its_range_and_keeps_a_value_it_gave() {
        for control in KNOBS {
            let (range, parameter) = (control.scale, control.parameter);
            assert_eq!(range.value(0.0), parameter.min, "{}", parameter.field);
            assert_eq!(range.value(1.0), parameter.max, "{}", parameter.field);
            for value in [parameter.min, parameter.default, parameter.max] {
                let back = range.value(range.position(value));
                assert_eq!(back, value, "{}", parameter.field);
            }
        }
    }

    /// The threshold handle is where the curve is at the threshold, and the ratio handle where
    /// it is at 0 dBFS, for any threshold and ratio.
    #[test]
    fn both_handles_are_on_the_curve() {
        for threshold_db in [-60., -40., -18., -3.] {
            for ratio in [1., 2., 4., 20., 100.] {
                let state = CompressorState {
                    threshold_db,
                    ratio,
                    knee_db: 0.,
                    ..CompressorState::default()
                };
                let across_at = |db: f32| threshold_travel().position(db);
                assert!((across_at(threshold_db) - across(threshold_db)).abs() < 1e-5);
                let travel = inverse_ratio_travel(threshold_db).unwrap();
                let handle = travel.position(1. / ratio);
                let (_, top) = LEVELS_DB;
                let curve = up(top - reduction_db(&state, top));
                assert!(
                    (handle - curve).abs() < 1e-4,
                    "{threshold_db} {ratio}: {handle} {curve}"
                );
            }
        }
    }
}
