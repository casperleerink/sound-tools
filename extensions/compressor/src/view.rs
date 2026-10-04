//! The card of the compressor: the transfer curve with the threshold and ratio handles, the
//! level now as a dot on it and the gain reduction as a bar, then Threshold, Ratio, Attack and
//! Release, and behind expand Knee, Makeup, Mix and Lookahead. The rack gives the view a
//! [`CardFrame`]: the picker of the slot as the title, and the power and close icons.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of a handle is one
//! gesture and one undo step, a key step, a reset or a switch is one commit. The ranges, the
//! defaults and the travel of each knob come from the [`Parameter`]s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the
//! card is expanded. A number that an automation lane of the track moves shows the value that
//! plays, on its knob and on the display, and does not drag ([`Lanes`]).

use gpui::{Context, Entity, Point, Task, Window, div, point, prelude::*, px};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle, INSET_HEIGHT};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, KnobRange, ParameterKnob, decibels_readout, milliseconds_readout, percent_readout, short,
};
use sound_ui::components::meter::GainReduction;
use sound_ui::{
    ActiveTheme, ControlEdit, Devices, Lanes, OfferGroup, Session, Slot, Views, every_poll,
    weak_callback,
};

use crate::{
    ATTACK, Compressor, CompressorState, KNEE, Lookahead, MAKEUP, MIX, Meters, RATIO, RELEASE,
    THRESHOLD, reduction_db,
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

/// Registers the view of the `compressor` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(CompressorView::new);
    devices.built_in::<CompressorState>(
        Slot::Effect,
        NAME,
        OfferGroup::Dynamics,
        "device-compressor",
        crate::EXTENSION,
        "This project does not load the compressor.",
    );
}

/// A knob of the card.
type Control = ParameterKnob<CompressorState>;

const THRESHOLD_KNOB: Control = Control::new(
    &THRESHOLD,
    "Threshold",
    "Change threshold",
    decibels_readout,
);
const RATIO_KNOB: Control = Control::new(&RATIO, "Ratio", "Change ratio", ratio_readout);
const ATTACK_KNOB: Control = Control::new(&ATTACK, "Attack", "Change attack", milliseconds_readout);
const RELEASE_KNOB: Control =
    Control::new(&RELEASE, "Release", "Change release", milliseconds_readout);
const KNEE_KNOB: Control = Control::new(&KNEE, "Knee", "Change knee", decibels_readout);
const MAKEUP_KNOB: Control = Control::new(&MAKEUP, "Makeup", "Change makeup", decibels_readout);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);

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

/// The value of a row of the lookahead select for each lookahead. The select says the number
/// and its cell says `ms`: `10 ms` does not fit in a cell.
const LOOKAHEADS: [(Lookahead, &str); 3] = [
    (Lookahead::Off, "0"),
    (Lookahead::One, "1"),
    (Lookahead::Ten, "10"),
];

fn lookahead_value(lookahead: Lookahead) -> &'static str {
    LOOKAHEADS
        .iter()
        .find(|(value, _)| *value == lookahead)
        .map_or("", |(_, label)| label)
}

/// The width of the open list of the lookahead select.
const LOOKAHEAD_MENU_WIDTH: f32 = 96.;

/// A ratio: `4:1`, `2.5:1`.
fn ratio_readout(ratio: f32) -> String {
    format!("{}:1", short(ratio))
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

impl Reading {
    /// A reading of the amplitude of the level and the reduction in dB, to a quarter of a point
    /// of the display and a tenth of a dB, so that a card whose sound holds still asks for no
    /// frame. Silence has no level.
    fn new(level: f32, reduction_db: f32) -> Self {
        let (bottom, top) = LEVELS_DB;
        let quarter_point = (top - bottom) / INSET_HEIGHT / 4.;
        let level_db = match level > 0. {
            true => (20. * level.log10() / quarter_point).round() * quarter_point,
            false => f32::NEG_INFINITY,
        };
        let reduction_db = (reduction_db * 10.).round() / 10.;
        Self {
            level_db,
            reduction_db,
        }
    }
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
    lanes: Entity<Lanes<CompressorState>>,
    /// Whether the card shows knee, makeup, mix and lookahead. Interface state: not saved.
    expanded: bool,
    /// The select of the lookahead. It is a view of its own because it opens a list; it shows
    /// what the record says, see [`Self::show_lookahead`].
    lookahead: Entity<DropdownMenu>,
    reading: Reading,
    _metering: Task<()>,
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
            ProjectEvent::Changed(id) if id == view.compressor.id() => {
                view.show_lookahead(cx);
                cx.notify();
            }
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
        let lookahead = cx.new(|cx| {
            let rows = LOOKAHEADS.map(|(_, label)| MenuItem::new(label, label));
            let rows = vec![MenuEntry::Group(
                MenuGroup::new().label("Lookahead, ms").items(rows),
            )];
            let shown = session.read(cx).project().state(&compressor);
            let shown = shown.map_or(Lookahead::Off, |state| state.lookahead);
            DropdownMenu::new("Lookahead", rows, cx)
                .selected(lookahead_value(shown))
                .trigger(Trigger::Select)
                .width(LOOKAHEAD_MENU_WIDTH)
                .debug_name("lookahead")
        });
        cx.subscribe(&lookahead, |view, _, MenuPicked(value), cx| {
            let picked = LOOKAHEADS
                .iter()
                .find(|(_, label)| *label == value.as_ref());
            if let Some((lookahead, _)) = picked {
                let set = |state: &mut CompressorState, lookahead| state.lookahead = lookahead;
                view.change("Change lookahead", ValueChange::Set(*lookahead), set, cx);
            }
        })
        .detach();
        // What the compressor did before this card was made, such as while its panel was
        // closed, is not what it does now.
        let project = session.read(cx).project();
        for name in [Meters::LEVEL, Meters::REDUCTION] {
            if let Some(peaks) = project.peaks(compressor.id(), name) {
                peaks.take();
            }
        }
        // The clock of the meters: as often as the session looks at the project.
        let metering = every_poll(cx, Self::read_meters);
        let lanes = Lanes::follow(&session, compressor.id(), Compressor::AUTOMATION, cx);
        Self {
            session,
            compressor,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
            lookahead,
            reading: Reading::default(),
            _metering: metering,
        }
    }

    /// Puts the lookahead of the record in its select, after any change of the record: a pick,
    /// an undo or an outside edit.
    fn show_lookahead(&mut self, cx: &mut Context<Self>) {
        let state = self.session.read(cx).project().state(&self.compressor);
        let Some(shown) = state.map(|state| lookahead_value(state.lookahead)) else {
            return;
        };
        self.lookahead.update(cx, |select, cx| {
            if select.value().map(AsRef::as_ref) != Some(shown) {
                select.set_selected(shown, cx);
            }
        });
    }

    /// Takes what the compressor heard and did since the last look, and draws again when that
    /// changes what the card shows. Called once per poll of the session.
    pub fn read_meters(&mut self, cx: &mut Context<Self>) {
        let (project, id) = (self.session.read(cx).project(), self.compressor.id());
        let take = |name| project.peaks(id, name).map_or(0., |peaks| peaks.take()[0]);
        let reading = Reading::new(take(Meters::LEVEL), take(Meters::REDUCTION));
        if reading != self.reading {
            self.reading = reading;
            cx.notify();
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

    fn knob(&self, control: Control, state: &CompressorState, cx: &mut Context<Self>) -> Knob {
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        control
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                view.change(control.undo_label, change, control.parameter.set, cx);
            }))
    }

    /// The handle at the threshold, on the curve: sideways is threshold.
    fn threshold_handle(&self, state: &CompressorState, cx: &mut Context<Self>) -> Handle {
        let threshold = state.threshold_db;
        let x = Axis::new(threshold_travel(), threshold, THRESHOLD.default);
        let y = Axis::fixed(up(threshold - reduction_db(state, threshold)));
        let automated = self.lanes.read(cx).is_automated(THRESHOLD.field);
        Handle::new("threshold", x, y)
            .automated(automated)
            .on_change(weak_callback(
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
        let automated = self.lanes.read(cx).is_automated(RATIO.field);
        Handle::new("ratio", x, y)
            .hollow(true)
            .automated(automated)
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
                .debug_selector(|| "compressor-level".into())
                .absolute()
                .left(px(x * DISPLAY_WIDTH - LEVEL_DOT / 2.))
                .top(px((1. - y) * INSET_HEIGHT - LEVEL_DOT / 2.))
                .size(px(LEVEL_DOT))
                .rounded_full()
                .bg(theme.green)
        });
        // In a place of its own: the bar positions itself relative to its own box.
        let bar = div()
            .absolute()
            .top(px(BAR_INSET))
            .right(px(BAR_INSET))
            .child(GainReduction::new(reduction_db).h(px(INSET_HEIGHT - 2. * BAR_INSET)));
        Display::new("display", DISPLAY_WIDTH)
            .curve(curve(state))
            .grid(vec![across(state.threshold_db)], Vec::new())
            .handle(self.threshold_handle(state, cx))
            .handle(self.ratio_handle(state, cx))
            .caption(reduction_readout(reduction_db))
            .children(level)
            .child(bar)
    }
}

impl Render for CompressorView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.lanes.read(cx).state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(THRESHOLD_KNOB, cx))
                .bottom(knob(ATTACK_KNOB, cx)),
            Column::new()
                .top(knob(RATIO_KNOB, cx))
                .bottom(knob(RELEASE_KNOB, cx)),
        ];
        let hidden = [
            Column::new()
                .top(knob(KNEE_KNOB, cx))
                .bottom(knob(MAKEUP_KNOB, cx)),
            Column::new().top(knob(MIX_KNOB, cx)).bottom(
                Cell::new(self.lookahead.clone())
                    .label("Lookahead")
                    .value("ms"),
            ),
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
        assert_eq!(ratio_readout(4.0), "4:1");
        assert_eq!(ratio_readout(2.5), "2.5:1");
        assert_eq!(reduction_readout(0.0), "GR 0 dB");
        assert_eq!(reduction_readout(0.04), "GR 0 dB");
        assert_eq!(reduction_readout(6.83), "GR -6.8 dB");
    }

    #[test]
    fn a_reading_of_silence_has_no_level_and_a_steady_one_does_not_move() {
        assert_eq!(Reading::new(0., 0.), Reading::default());
        let reading = Reading::new(0.5, 6.83);
        assert!((reading.level_db + 6.02).abs() < 0.2, "{reading:?}");
        assert_eq!(reading.reduction_db, 6.8);
        assert_eq!(Reading::new(0.5001, 6.8301), reading);
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
