//! The card of the gate: the transfer curve, a step down by the range under the threshold, with
//! the threshold handle, the level now as a dot on it and the gain reduction as a bar. Then
//! Threshold, Range, Attack, Release, Transient and Sustain, and behind expand Hold and the
//! sidechain picker. The rack gives the view a [`CardFrame`].
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session by [`ControlEdit`], as on the compressor. A number that an
//! automation lane of the track moves shows the value that plays and does not drag
//! ([`Lanes`]).

use gpui::{Context, Entity, Point, Task, Window, div, point, prelude::*, px};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::device_card::{CardFrame, Column, Section};
use sound_ui::components::display::{Axis, Display, Handle, INSET_HEIGHT};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, KnobRange, ParameterKnob, decibels_readout, milliseconds_readout, short,
};
use sound_ui::components::meter::GainReduction;
use sound_ui::{
    ActiveTheme, ControlEdit, Devices, Lanes, OfferGroup, Session, Views, every_poll, weak_callback,
};

use crate::{ATTACK, Gate, GateState, HOLD, Meters, RANGE, RELEASE, SUSTAIN, THRESHOLD, TRANSIENT};

/// The name the rack puts on the card of a gate.
pub const NAME: &str = "Gate";

/// The width of the display, as on the compressor.
const DISPLAY_WIDTH: f32 = 136.;

/// The curve shows levels from here to there, in dBFS, the input across and the output up:
/// the range of the threshold.
const LEVELS_DB: (f32, f32) = (-80., 0.);

/// The curve is as wide as the display is tall, so an open gate is a diagonal. The gain
/// reduction bar has the rest, at the right edge.
const CURVE_WIDTH: f32 = INSET_HEIGHT / DISPLAY_WIDTH;

/// The dot of the level and the bar of the gain reduction, in points.
const LEVEL_DOT: f32 = 8.;
const BAR_INSET: f32 = 6.;

/// Registers the view of the `gate` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(GateView::new);
    devices.built_in::<GateState>(
        NAME,
        OfferGroup::Dynamics,
        "device-gate",
        crate::EXTENSION,
        "This project does not load the gate.",
    );
}

/// A knob of the card.
type Control = ParameterKnob<GateState>;

const THRESHOLD_KNOB: Control = Control::new(
    &THRESHOLD,
    "Threshold",
    "Change threshold",
    decibels_readout,
);
const RANGE_KNOB: Control = Control::new(&RANGE, "Range", "Change range", decibels_readout);
const ATTACK_KNOB: Control = Control::new(&ATTACK, "Attack", "Change attack", milliseconds_readout);
const RELEASE_KNOB: Control =
    Control::new(&RELEASE, "Release", "Change release", milliseconds_readout);
const HOLD_KNOB: Control = Control::new(&HOLD, "Hold", "Change hold", milliseconds_readout);
const TRANSIENT_KNOB: Control = Control::new(
    &TRANSIENT,
    "Transient",
    "Change transient",
    decibels_readout,
)
.bipolar();
const SUSTAIN_KNOB: Control =
    Control::new(&SUSTAIN, "Sustain", "Change sustain", decibels_readout).bipolar();

/// The gain reduction under the display, to a tenth of a dB: `GR 0 dB`, `GR -24 dB`.
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

/// The levels across the whole width of the display, so the handle drags as the dot moves.
fn threshold_travel() -> KnobRange {
    let (bottom, top) = LEVELS_DB;
    KnobRange::linear(bottom, bottom + (top - bottom) / CURVE_WIDTH)
}

/// The transfer curve: what comes out for each level that goes in. Four corners: under the
/// threshold down by the range, a step up at the threshold, then the diagonal.
fn curve(state: &GateState) -> Vec<Point<f32>> {
    let (bottom, top) = LEVELS_DB;
    let threshold = state.threshold_db;
    let below = threshold - state.range_db;
    [
        (bottom, bottom - state.range_db),
        (threshold, below),
        (threshold, threshold),
        (top, top),
    ]
    .into_iter()
    .map(|(input, output)| point(across(input), up(output)))
    .collect()
}

/// What the gate heard and did since the card last looked: the peak level and the most it
/// turned down, both in dB, rounded so a card whose sound holds still asks for no frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Reading {
    level_db: f32,
    reduction_db: f32,
}

impl Reading {
    fn new(level: f32, reduction_db: f32) -> Self {
        let (bottom, top) = LEVELS_DB;
        let quarter_point = (top - bottom) / INSET_HEIGHT / 4.;
        let level_db = match level > 0. {
            true => (20. * level.log10() / quarter_point).round() * quarter_point,
            false => f32::NEG_INFINITY,
        };
        Self {
            level_db,
            reduction_db: (reduction_db * 10.).round() / 10.,
        }
    }
}

impl Default for Reading {
    fn default() -> Self {
        Self::new(0., 0.)
    }
}

pub struct GateView {
    session: Entity<Session>,
    gate: Instance<GateState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of the handle.
    edit: ControlEdit,
    lanes: Entity<Lanes<GateState>>,
    /// Whether the card shows hold and the sidechain. Interface state: not saved.
    expanded: bool,
    reading: Reading,
    _metering: Task<()>,
}

impl GateView {
    pub fn new(
        session: Entity<Session>,
        gate: Instance<GateState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.gate.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.gate.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        // What the gate did before this card was made is not what it does now.
        let project = session.read(cx).project();
        for name in [Meters::LEVEL, Meters::REDUCTION] {
            if let Some(peaks) = project.peaks(gate.id(), name) {
                peaks.take();
            }
        }
        let metering = every_poll(cx, Self::read_meters);
        let lanes = Lanes::follow(&session, gate.id(), Gate::AUTOMATION, cx);
        Self {
            session,
            gate,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
            reading: Reading::default(),
            _metering: metering,
        }
    }

    /// Takes what the gate heard and did since the last look, and draws again when that
    /// changes what the card shows. Called once per poll of the session.
    pub fn read_meters(&mut self, cx: &mut Context<Self>) {
        let (project, id) = (self.session.read(cx).project(), self.gate.id());
        let take = |name| project.peaks(id, name).map_or(0., |peaks| peaks.take()[0]);
        let reading = Reading::new(take(Meters::LEVEL), take(Meters::REDUCTION));
        if reading != self.reading {
            self.reading = reading;
            cx.notify();
        }
    }

    /// Shows or hides hold and the sidechain, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut GateState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, gate) = (&self.session, &self.gate);
        self.edit.apply(session, gate, label, change, set, cx);
    }

    fn knob(&self, control: Control, state: &GateState, cx: &mut Context<Self>) -> Knob {
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        control
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                view.change(control.undo_label, change, control.parameter.set, cx);
            }))
    }

    /// The handle at the top of the step: sideways is threshold.
    fn threshold_handle(&self, state: &GateState, cx: &mut Context<Self>) -> Handle {
        let threshold = state.threshold_db;
        let x = Axis::new(threshold_travel(), threshold, THRESHOLD.default);
        let y = Axis::fixed(up(threshold));
        let automated = self.lanes.read(cx).is_automated(THRESHOLD.field);
        Handle::new("threshold", x, y)
            .automated(automated)
            .on_change(weak_callback(
                cx,
                |view, change: ValueChange<Point<f32>>, cx| {
                    // The travel reaches past 0 dBFS, the end of the curve.
                    let set = |state: &mut GateState, at: Point<f32>| {
                        state.threshold_db = THRESHOLD_KNOB.clamp(at.x);
                    };
                    view.change(THRESHOLD_KNOB.undo_label, change, set, cx);
                },
            ))
    }

    fn display(&self, state: &GateState, cx: &mut Context<Self>) -> Display {
        let theme = cx.theme();
        let Reading {
            level_db,
            reduction_db,
        } = self.reading;
        let (bottom, _) = LEVELS_DB;
        // The level now: what came in, and that less the reduction.
        let level = (level_db > bottom).then(|| {
            let (x, y) = (across(level_db), up(level_db - reduction_db));
            div()
                .debug_selector(|| "gate-level".into())
                .absolute()
                .left(px(x * DISPLAY_WIDTH - LEVEL_DOT / 2.))
                .top(px((1. - y) * INSET_HEIGHT - LEVEL_DOT / 2.))
                .size(px(LEVEL_DOT))
                .rounded_full()
                .bg(theme.green)
        });
        let bar = div()
            .absolute()
            .top(px(BAR_INSET))
            .right(px(BAR_INSET))
            .child(GainReduction::new(reduction_db).h(px(INSET_HEIGHT - 2. * BAR_INSET)));
        Display::new("display", DISPLAY_WIDTH)
            .curve(curve(state))
            .grid(vec![across(state.threshold_db)], Vec::new())
            .handle(self.threshold_handle(state, cx))
            .caption(reduction_readout(reduction_db))
            .children(level)
            .child(bar)
    }
}

impl Render for GateView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.lanes.read(cx).state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            (THRESHOLD_KNOB, RANGE_KNOB),
            (ATTACK_KNOB, RELEASE_KNOB),
            (TRANSIENT_KNOB, SUSTAIN_KNOB),
        ]
        .map(|(top, bottom)| Column::new().top(knob(top, cx)).bottom(knob(bottom, cx)));
        let expand = cx.listener(|view, _, _, cx| view.set_expanded(!view.expanded, cx));
        let card = self
            .frame
            .card()
            .expand(self.expanded, expand)
            .display(self.display(&state, cx));
        let card = columns
            .into_iter()
            .fold(card, |card, column| card.column(column))
            .hidden_column(Column::new().top(knob(HOLD_KNOB, cx)));
        // Made only while it shows: it reads the tracks.
        let sidechain = self.expanded.then(|| self.frame.sidechain_column(cx));
        let card = match sidechain.flatten() {
            Some(column) => card.section(Section::new().column(column)),
            None => card,
        };
        card.into_any_element()
    }
}
