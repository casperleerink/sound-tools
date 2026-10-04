//! The card of the reverb: the decay in time with a handle at its start and its end, then Size,
//! Damping, Width and Mix, and behind expand the cuts, diffusion, freeze, and pre-delay and
//! decay as knobs. The rack gives the view a [`CardFrame`]: the picker of the slot as the title,
//! and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change goes
//! through the session, by [`ControlEdit`]: a drag of a knob or of a handle is one gesture and one
//! undo step, a key step, a reset or a switch is one commit. The ranges, the defaults and the
//! travel of each knob come from the [`Parameter`](crate::Parameter)s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the card
//! is expanded. A number that an automation lane of the track moves shows the value that plays, on
//! its knob and on the display, and does not drag ([`Lanes`]).

use gpui::{Context, Entity, Point, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, ParameterKnob, hertz_readout, milliseconds_readout, percent_readout, seconds_readout,
};
use sound_ui::components::toggle::Toggle;
use sound_ui::{ControlEdit, Devices, Lanes, OfferGroup, Session, Slot, Views, weak_callback};

use crate::{
    DAMPING, DECAY, DIFFUSION, HIGH_CUT, LOW_CUT, MIX, PRE_DELAY, Reverb, ReverbState, SIZE, WIDTH,
    high_decay_seconds, reflections,
};

/// The name the rack puts on the card of a reverb.
pub const NAME: &str = "Reverb";

/// The width of the display. With it and two columns of cells the card is 352 pt, as DESIGN.md
/// gives the reverb.
const DISPLAY_WIDTH: f32 = 200.;

/// Registers the view of the `reverb` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(ReverbView::new);
    devices.built_in::<ReverbState>(
        Slot::Effect,
        NAME,
        OfferGroup::Space,
        "device-reverb",
        crate::EXTENSION,
        "This project does not load the reverb.",
    );
}

/// A knob of the card.
type Control = ParameterKnob<ReverbState>;

const SIZE_KNOB: Control = Control::new(&SIZE, "Size", "Change size", percent_readout);
const DAMPING_KNOB: Control = Control::new(&DAMPING, "Damping", "Change damping", percent_readout);
const WIDTH_KNOB: Control = Control::new(&WIDTH, "Width", "Change width", percent_readout);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);
const LOW_CUT_KNOB: Control = Control::new(&LOW_CUT, "Low cut", "Change low cut", hertz_readout);
const HIGH_CUT_KNOB: Control =
    Control::new(&HIGH_CUT, "High cut", "Change high cut", hertz_readout);
const DIFFUSION_KNOB: Control =
    Control::new(&DIFFUSION, "Diffusion", "Change diffusion", percent_readout);
const PRE_DELAY_KNOB: Control = Control::new(
    &PRE_DELAY,
    "Pre-delay",
    "Change pre-delay",
    milliseconds_readout,
);
const DECAY_KNOB: Control = Control::new(&DECAY, "Decay", "Change decay", seconds_readout);

/// Every knob, in the order of the card: shown, then hidden.
#[cfg(test)]
const KNOBS: [&Control; 9] = [
    &SIZE_KNOB,
    &DAMPING_KNOB,
    &WIDTH_KNOB,
    &MIX_KNOB,
    &LOW_CUT_KNOB,
    &HIGH_CUT_KNOB,
    &DIFFUSION_KNOB,
    &PRE_DELAY_KNOB,
    &DECAY_KNOB,
];

/// Where the decay sits in its display, as places from 0 to 1, `y` up.
///
/// Pre-delay and decay each have a zone across, on the travel of their knobs, so any of their
/// values shows and a handle moves as its knob turns. The tail starts at the end of the
/// pre-delay, at full level, and is a straight line in dB to the floor at the end of the decay.
/// Between the two the time is linear, so the highs, which die in a part of the decay time, end
/// at that part of the way. The early reflections are drawn after the start in a zone of their
/// own, wider as the room grows: in the time of the tail they would be one line.
mod layout {
    use sound_ui::components::knob::KnobRange;

    /// Where the pre-delay starts.
    pub(super) const LEFT: f32 = 0.04;
    /// The zones of the pre-delay and of the decay.
    pub(super) const PRE_DELAY_ZONE: f32 = 0.14;
    pub(super) const DECAY_ZONE: f32 = 0.74;
    /// The shortest tail is this long, so it still falls and its end can be taken apart from
    /// its start.
    pub(super) const SHORTEST_TAIL: f32 = 0.04;
    /// How far after the start the reflections of the largest room reach. A smaller room
    /// takes a part of it, from half at size 0.
    pub(super) const EARLY_ZONE: f32 = 0.3;
    /// Every so many lines is drawn as a reflection, so the marks stay apart.
    pub(super) const EVERY: usize = 2;
    /// Full level and the floor 60 dB under it, clear of the edges so a handle there can be
    /// taken.
    pub(super) const TOP: f32 = 0.88;
    pub(super) const FLOOR: f32 = 0.08;
    /// A reflection is drawn up to this part of the height of the tail where it is.
    pub(super) const MARK_HEIGHT: f32 = 0.85;

    /// The range of a time whose zone starts at `start`: `time.position(value)` of the knob,
    /// squeezed into the zone and moved to its start. A logarithmic range stays one when it is
    /// stretched and moved, with other ends.
    pub(super) fn time_axis(time: KnobRange, start: f32, zone: f32) -> KnobRange {
        let ratio = time.max / time.min;
        let min = time.min * ratio.powf(-start / zone);
        KnobRange::logarithmic(min, min * ratio.powf(1. / zone))
    }
}

/// The places across of the start of the tail and of its end.
fn start_and_end(state: &ReverbState) -> (f32, f32) {
    use layout::{DECAY_ZONE, LEFT, PRE_DELAY_ZONE, SHORTEST_TAIL};
    let start = LEFT + PRE_DELAY_ZONE * PRE_DELAY_KNOB.range().position(state.pre_delay_ms);
    let end = start + SHORTEST_TAIL + DECAY_ZONE * DECAY_KNOB.range().position(state.decay_seconds);
    (start, end)
}

/// The tail, the highs and the early reflections, as places on the display.
struct Drawing {
    tail: [Point<f32>; 2],
    highs: [Point<f32>; 2],
    reflections: Vec<Point<f32>>,
}

fn drawing(state: &ReverbState) -> Drawing {
    use layout::{EARLY_ZONE, EVERY, FLOOR, MARK_HEIGHT, TOP};
    let (start, end) = start_and_end(state);
    let length = end - start;
    let highs_end = start + length * high_decay_seconds(state) / state.decay_seconds;
    let height_at = |x: f32| TOP + (FLOOR - TOP) * (x - start) / length;
    let zone = EARLY_ZONE * (0.5 + 0.5 * state.size);
    let reflections = reflections()
        .into_iter()
        .step_by(EVERY)
        .map(|part| start + zone * part)
        .filter(|x| *x < end)
        .map(|x| point(x, height_at(x) * MARK_HEIGHT))
        .collect();
    Drawing {
        tail: [point(start, TOP), point(end, FLOOR)],
        highs: [point(start, TOP), point(highs_end, FLOOR)],
        reflections,
    }
}

pub struct ReverbView {
    session: Entity<Session>,
    reverb: Instance<ReverbState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of a handle.
    edit: ControlEdit,
    lanes: Entity<Lanes<ReverbState>>,
    /// Whether the card shows the hidden controls. Interface state: not saved.
    expanded: bool,
}

impl ReverbView {
    pub fn new(
        session: Entity<Session>,
        reverb: Instance<ReverbState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.reverb.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.reverb.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let lanes = Lanes::follow(&session, reverb.id(), Reverb::AUTOMATION, cx);
        Self {
            session,
            reverb,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
        }
    }

    /// Shows or hides the hidden controls, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut ReverbState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, reverb) = (&self.session, &self.reverb);
        self.edit.apply(session, reverb, label, change, set, cx);
    }

    fn knob(&self, control: Control, state: &ReverbState, cx: &mut Context<Self>) -> Knob {
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        control
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                view.change(control.undo_label, change, control.parameter.set, cx);
            }))
    }

    /// A handle that moves one time sideways, at a fixed height, in the zone that starts at
    /// `start`. It edits what its knob edits, as one undo step with the same name.
    fn time_handle(
        &self,
        id: &'static str,
        control: Control,
        (start, zone, height): (f32, f32, f32),
        state: &ReverbState,
        cx: &mut Context<Self>,
    ) -> Handle {
        let parameter = control.parameter;
        let x = Axis::new(
            layout::time_axis(control.range(), start, zone),
            (parameter.get)(state),
            parameter.default,
        );
        let automated = self.lanes.read(cx).is_automated(parameter.field);
        let handle = Handle::new(id, x, Axis::fixed(height)).automated(automated);
        handle.on_change(weak_callback(
            cx,
            move |view, change: ValueChange<Point<f32>>, cx| {
                let set = |state: &mut ReverbState, place: Point<f32>| {
                    (parameter.set)(state, control.clamp(place.x))
                };
                view.change(control.undo_label, change, set, cx);
            },
        ))
    }

    /// The decay in time: the start drags the pre-delay, the end drags the decay.
    fn display(&self, state: &ReverbState, cx: &mut Context<Self>) -> Display {
        use layout::{DECAY_ZONE, FLOOR, LEFT, PRE_DELAY_ZONE, SHORTEST_TAIL, TOP};
        let Drawing {
            tail,
            highs,
            reflections,
        } = drawing(state);
        let start = tail[0].x;
        let pre_delay = (LEFT, PRE_DELAY_ZONE, TOP);
        let pre_delay = self.time_handle("pre-delay", PRE_DELAY_KNOB, pre_delay, state, cx);
        let decay = (start + SHORTEST_TAIL, DECAY_ZONE, FLOOR);
        let decay = self.time_handle("decay", DECAY_KNOB, decay, state, cx);
        let caption = format!(
            "Pre-delay {} · Decay {}",
            milliseconds_readout(state.pre_delay_ms),
            seconds_readout(state.decay_seconds),
        );
        Display::new("display", DISPLAY_WIDTH)
            .curve(tail)
            .marks(reflections)
            .dashed(highs)
            .handle(pre_delay.hollow(true))
            .handle(decay)
            .caption(caption)
    }

    fn freeze(&self, state: &ReverbState, cx: &mut Context<Self>) -> Cell {
        let frozen = state.freeze;
        let label = if frozen { "On" } else { "Off" };
        let toggle = Toggle::new("freeze", label, frozen).on_change(weak_callback(
            cx,
            |view, frozen: bool, cx| {
                let set = |state: &mut ReverbState, frozen| state.freeze = frozen;
                view.change("Change freeze", ValueChange::Set(frozen), set, cx);
            },
        ));
        Cell::new(toggle).label("Freeze")
    }
}

impl Render for ReverbView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.lanes.read(cx).state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(SIZE_KNOB, cx))
                .bottom(knob(WIDTH_KNOB, cx)),
            Column::new()
                .top(knob(DAMPING_KNOB, cx))
                .bottom(knob(MIX_KNOB, cx)),
        ];
        let hidden = [
            Column::new()
                .top(knob(LOW_CUT_KNOB, cx))
                .bottom(knob(DIFFUSION_KNOB, cx)),
            Column::new()
                .top(knob(HIGH_CUT_KNOB, cx))
                .bottom(knob(PRE_DELAY_KNOB, cx)),
            Column::new()
                .top(self.freeze(&state, cx))
                .bottom(knob(DECAY_KNOB, cx)),
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

    /// A handle is where its knob says, and the tail is drawn between the two handles, inside
    /// the display at every end of both ranges.
    #[test]
    fn the_handles_are_at_the_ends_of_the_tail_and_inside_the_display() {
        for pre_delay_ms in [PRE_DELAY.min, PRE_DELAY.default, PRE_DELAY.max] {
            for decay_seconds in [DECAY.min, DECAY.default, DECAY.max] {
                let state = ReverbState {
                    pre_delay_ms,
                    decay_seconds,
                    ..ReverbState::default()
                };
                let (start, end) = start_and_end(&state);
                let at = |start, zone, control: Control| {
                    let value = (control.parameter.get)(&state);
                    layout::time_axis(control.range(), start, zone).position(value)
                };
                let handle_start = at(layout::LEFT, layout::PRE_DELAY_ZONE, PRE_DELAY_KNOB);
                let decay_start = start + layout::SHORTEST_TAIL;
                let handle_end = at(decay_start, layout::DECAY_ZONE, DECAY_KNOB);
                assert!((handle_start - start).abs() < 1e-4, "{state:?}");
                assert!((handle_end - end).abs() < 1e-4, "{state:?}");
                assert!(start > 0.0 && end < 1.0 && start < end, "{state:?}");
            }
        }
    }

    /// The highs end sooner the more damping, and at the end of the tail with none. The
    /// reflections are under the tail, and move apart as the room grows.
    #[test]
    fn the_highs_fall_sooner_with_damping_and_the_reflections_spread_with_size() {
        let at = |damping, size| {
            drawing(&ReverbState {
                damping,
                size,
                ..ReverbState::default()
            })
        };
        let none = at(0.0, 0.5);
        assert_eq!(none.highs[1].x, none.tail[1].x);
        let (half, full) = (at(0.5, 0.5), at(1.0, 0.5));
        assert!(half.highs[1].x < none.highs[1].x);
        assert!(full.highs[1].x < half.highs[1].x);
        let spread = |drawing: &Drawing| {
            let last = drawing.reflections.last().map_or(0.0, |mark| mark.x);
            last - drawing.tail[0].x
        };
        assert!(spread(&at(0.5, 1.0)) > spread(&at(0.5, 0.2)));
        for mark in &none.reflections {
            assert!(mark.y < layout::TOP && mark.y > 0.0, "{mark:?}");
        }
    }
}
