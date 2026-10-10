//! The card of the compressor: the transfer curve with the threshold and ratio handles, the
//! level now as a dot on it and the gain reduction as a bar, then Threshold, Ratio, Attack and
//! Release, and behind expand Knee, Makeup, Mix and Lookahead, and the sidechain picker in a
//! section of its own. The rack gives the view a [`CardFrame`]: the picker of the slot as the
//! title, the power and close icons, and the sidechain picker.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of a handle is one
//! gesture and one undo step, a key step, a reset or a switch is one commit. The ranges, the
//! defaults and the travel of each knob come from the [`Parameter`]s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the
//! card is expanded. A number that an automation lane of the track moves shows the value that
//! plays, on its knob and on the display, and does not drag ([`Lanes`]).

use gpui::{Context, Entity, Point, Task, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column, Section};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::dynamics_display::{self, Levels, Reading};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, KnobRange, ParameterKnob, decibels_readout, milliseconds_readout, percent_readout, short,
};
use sound_ui::{
    ControlEdit, Devices, Lanes, OfferGroup, Session, Views, every_poll, weak_callback,
};

use crate::{
    ATTACK, Compressor, CompressorState, KNEE, Lookahead, MAKEUP, MIX, Meters, RATIO, RELEASE,
    THRESHOLD, reduction_db,
};

/// The name the rack puts on the card of a compressor.
pub const NAME: &str = "Compressor";

/// The curve shows levels from here to there, in dBFS: the range of the threshold. With the
/// display and two columns of cells the card is 288 pt, as DESIGN.md gives the compressor.
const LEVELS: Levels = Levels {
    bottom_db: -60.,
    top_db: 0.,
};

/// Points of the curve across its width.
const CURVE_POINTS: usize = 60;

/// Registers the view of the `compressor` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(CompressorView::new);
    devices.built_in::<CompressorState>(
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

/// The transfer curve: what comes out for each level that goes in, before makeup and mix.
fn curve(state: &CompressorState) -> Vec<Point<f32>> {
    let Levels {
        bottom_db: bottom,
        top_db: top,
    } = LEVELS;
    (0..=CURVE_POINTS)
        .map(|step| {
            let input = bottom + (top - bottom) * step as f32 / CURVE_POINTS as f32;
            point(
                LEVELS.across(input),
                LEVELS.up(input - reduction_db(state, input)),
            )
        })
        .collect()
}

/// The ratio handle sits on the curve at its right end, at an input of 0 dBFS, where the output
/// is `threshold (1 - 1 / ratio)`: a straight line in `1 / ratio` over the height. So the handle
/// moves `1 / ratio`, and its travel is that line stretched over the height, from what puts it
/// at the bottom to 1 at the top. `None` when the threshold is so close to 0 dBFS that the line
/// above it has no length to drag.
fn inverse_ratio_travel(threshold_db: f32) -> Option<KnobRange> {
    (threshold_db < -1.).then(|| KnobRange::linear(1. - LEVELS.bottom_db / threshold_db, 1.))
}

pub struct CompressorView {
    session: Entity<Session>,
    compressor: Instance<CompressorState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of a handle.
    edit: ControlEdit,
    lanes: Entity<Lanes<CompressorState>>,
    /// Whether the card shows knee, makeup, mix, lookahead and the sidechain. Interface state:
    /// not saved.
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
        let reading = LEVELS.reading(take(Meters::LEVEL), take(Meters::REDUCTION));
        if reading != self.reading {
            self.reading = reading;
            cx.notify();
        }
    }

    /// Shows or hides knee, makeup, mix, lookahead and the sidechain, as the expand icon does.
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
        let x = Axis::new(LEVELS.threshold_travel(), threshold, THRESHOLD.default);
        let y = Axis::fixed(LEVELS.up(threshold - reduction_db(state, threshold)));
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
        let top = LEVELS.top_db;
        let x = Axis::fixed(LEVELS.across(top));
        let y = match inverse_ratio_travel(state.threshold_db) {
            Some(travel) => Axis::new(travel, 1. / state.ratio, 1. / RATIO.default),
            None => Axis::fixed(LEVELS.up(top - reduction_db(state, top))),
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
        let display = Display::new("display", dynamics_display::WIDTH)
            .curve(curve(state))
            .grid(vec![LEVELS.across(state.threshold_db)], Vec::new())
            .handle(self.threshold_handle(state, cx))
            .handle(self.ratio_handle(state, cx));
        LEVELS.meters(display, self.reading, "compressor-level", cx)
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
        // Made only while it shows: it reads the tracks.
        let sidechain = self.expanded.then(|| self.frame.sidechain_column(cx));
        let card = match sidechain.flatten() {
            Some(column) => card.section(Section::new().column(column)),
            None => card,
        };
        card.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ratio_reads_as_a_ratio() {
        assert_eq!(ratio_readout(4.0), "4:1");
        assert_eq!(ratio_readout(2.5), "2.5:1");
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
                let across_at = |db: f32| LEVELS.threshold_travel().position(db);
                assert!((across_at(threshold_db) - LEVELS.across(threshold_db)).abs() < 1e-5);
                let travel = inverse_ratio_travel(threshold_db).unwrap();
                let handle = travel.position(1. / ratio);
                let top = LEVELS.top_db;
                let curve = LEVELS.up(top - reduction_db(&state, top));
                assert!(
                    (handle - curve).abs() < 1e-4,
                    "{threshold_db} {ratio}: {handle} {curve}"
                );
            }
        }
    }
}
