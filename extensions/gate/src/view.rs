//! The card of the gate: the transfer curve, a step down by the range under the threshold, with
//! the threshold handle, the level now as a dot on it and the gain reduction as a bar. Then
//! Threshold, Range, Attack, Release, Transient and Sustain, and behind expand Hold and the
//! sidechain picker. The rack gives the view a [`CardFrame`].
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session by [`ControlEdit`], as on the compressor. A number that an
//! automation lane of the track moves shows the value that plays and does not drag
//! ([`Lanes`]).

use gpui::{Context, Entity, Point, Task, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::device_card::{CardFrame, Column, Section};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::dynamics_display::{self, Levels, Reading};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, ParameterKnob, decibels_readout, milliseconds_readout};
use sound_ui::{
    ControlEdit, Devices, Lanes, OfferGroup, Session, Views, every_poll, weak_callback,
};

use crate::{ATTACK, Gate, GateState, HOLD, Meters, RANGE, RELEASE, SUSTAIN, THRESHOLD, TRANSIENT};

/// The name the rack puts on the card of a gate.
pub const NAME: &str = "Gate";

/// The curve shows levels from here to there, in dBFS: the range of the threshold.
const LEVELS: Levels = Levels {
    bottom_db: -80.,
    top_db: 0.,
};

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

/// The transfer curve: what comes out for each level that goes in. Four corners: under the
/// threshold down by the range, a step up at the threshold, then the diagonal.
fn curve(state: &GateState) -> Vec<Point<f32>> {
    let Levels {
        bottom_db: bottom,
        top_db: top,
    } = LEVELS;
    let threshold = state.threshold_db;
    [
        (bottom, bottom - state.range_db),
        (threshold, threshold - state.range_db),
        (threshold, threshold),
        (top, top),
    ]
    .into_iter()
    .map(|(input, output)| point(LEVELS.across(input), LEVELS.up(output)))
    .collect()
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
    fn read_meters(&mut self, cx: &mut Context<Self>) {
        let (project, id) = (self.session.read(cx).project(), self.gate.id());
        let take = |name| project.peaks(id, name).map_or(0., |peaks| peaks.take()[0]);
        let reading = LEVELS.reading(take(Meters::LEVEL), take(Meters::REDUCTION));
        if reading != self.reading {
            self.reading = reading;
            cx.notify();
        }
    }

    /// Shows or hides hold and the sidechain, as the expand icon does.
    fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
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
        let x = Axis::new(LEVELS.threshold_travel(), threshold, THRESHOLD.default);
        let y = Axis::fixed(LEVELS.up(threshold));
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
        let display = Display::new("display", dynamics_display::WIDTH)
            .curve(curve(state))
            .grid(vec![LEVELS.across(state.threshold_db)], Vec::new())
            .handle(self.threshold_handle(state, cx));
        LEVELS.meters(display, self.reading, "gate-level", cx)
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
