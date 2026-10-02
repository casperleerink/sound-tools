//! The card of the delay: the repeats in time, with a handle on the second repeat and, while it
//! syncs, the feel at the top; then Time, Feedback, Sync and Mix, and behind expand the cuts and
//! ping-pong. The rack gives the view a [`CardFrame`]: the picker of the slot as the title, and
//! the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change goes
//! through the session, by [`ControlEdit`]: a drag of a knob or of the handle is one gesture and
//! one undo step, a key step, a reset or a switch is one commit. The ranges, the defaults and the
//! travel of each knob come from the [`Parameter`](crate::Parameter)s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the card
//! is expanded. A number that an automation lane of the track moves shows the value that plays, on
//! its knob and on the display, and does not drag ([`Lanes`]).

use gpui::{Context, Entity, Point, SharedString, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, KnobRange, ParameterKnob, hertz_readout, milliseconds_readout, percent_readout,
};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::toggle::Toggle;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Lanes, Session, Views, weak_callback};

use crate::{Delay, DelayState, Division, FEEDBACK, Feel, HIGH_CUT, LOW_CUT, MIX, TIME};

/// The name the rack puts on the card of a delay.
pub const NAME: &str = "Delay";

/// The width of the display. With it and two columns of cells the card is 352 pt, as wide as
/// the Filter and the Reverb.
const DISPLAY_WIDTH: f32 = 200.;

/// Registers the view of the `delay` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(DelayView::new);
    devices.describe::<DelayState>(|_| DeviceLabel {
        key: DelayState::TOOL.into(),
        name: NAME.into(),
    });
}

/// A knob of the card.
type Control = ParameterKnob<DelayState>;

const TIME_KNOB: Control = Control::new(&TIME, "Time", "Change time", milliseconds_readout);
const FEEDBACK_KNOB: Control =
    Control::new(&FEEDBACK, "Feedback", "Change feedback", percent_readout);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);
const LOW_CUT_KNOB: Control = Control::new(&LOW_CUT, "Low cut", "Change low cut", hertz_readout);
const HIGH_CUT_KNOB: Control =
    Control::new(&HIGH_CUT, "High cut", "Change high cut", hertz_readout);

/// Every knob of a number, in the order of the card: shown, then hidden.
#[cfg(test)]
const KNOBS: [&Control; 5] = [
    &TIME_KNOB,
    &FEEDBACK_KNOB,
    &MIX_KNOB,
    &LOW_CUT_KNOB,
    &HIGH_CUT_KNOB,
];

/// While it syncs, the Time knob steps through the divisions, by their place in
/// [`Division::ALL`].
const DIVISIONS: KnobRange = KnobRange::linear(0., (Division::ALL.len() - 1) as f32);

fn place_of(division: Division) -> f32 {
    let place = Division::ALL.iter().position(|each| *each == division);
    place.unwrap_or_default() as f32
}

/// The division at the place nearest `place`, at an end when it is past one.
fn division_at(place: f32) -> Division {
    let last = Division::ALL.len() - 1;
    let index = place.round().clamp(0., last as f32) as usize;
    Division::ALL[index.min(last)]
}

/// The value of a segment and its label, for each feel.
const FEELS: [(Feel, &str, &str); 3] = [
    (Feel::Straight, "straight", "Straight"),
    (Feel::Dotted, "dotted", "Dotted"),
    (Feel::Triplet, "triplet", "Triplet"),
];

/// The time as the Time knob shows it: the note while it syncs, with `.` when dotted and `t`
/// for a triplet, as in `1/8.` and `1/8t`, else the ms.
fn time_readout(state: &DelayState) -> String {
    if !state.sync {
        return milliseconds_readout(state.time_ms);
    }
    let feel = match state.feel {
        Feel::Straight => "",
        Feel::Dotted => ".",
        Feel::Triplet => "t",
    };
    format!("{}{feel}", state.division.name())
}

/// Where the repeats sit in the display, as places from 0 to 1, `y` up.
///
/// The sound comes in at the left, and the repeats follow it evenly: the first one after a gap
/// on the travel of the Time knob, each one after that as far again. So a short time is a dense
/// row of repeats and a long one a few. Each repeat is lower than the one before by the
/// feedback, on a straight scale of level. The handle is on the second repeat: sideways it moves
/// the time, up and down the feedback, and it stays on the drawing at every value.
mod layout {
    use sound_core::Scale;
    use sound_ui::components::knob::KnobRange;

    /// Where the sound comes in, and the air after the last repeat drawn.
    pub(super) const LEFT: f32 = 0.04;
    /// The gap to the first repeat at the shortest time, so it stays apart from the sound.
    pub(super) const SHORTEST_GAP: f32 = 0.03;
    /// How much further the first repeat is at the longest time.
    pub(super) const GAP_ZONE: f32 = 0.4;
    /// Full level: under the feel at the top of the display.
    pub(super) const TOP: f32 = 0.7;
    /// Half the width of a repeat at its foot.
    pub(super) const HALF_WIDTH: f32 = 0.006;
    /// A repeat lower than this part of full level is not drawn.
    pub(super) const LOWEST: f32 = 0.01;
    /// The most repeats drawn, however short the time.
    pub(super) const MOST: usize = 64;

    /// A range whose place is `start + zone × range.position(value)`: the range stretched over
    /// the zone and moved to its start. A logarithmic range stays one, with other ends. No range
    /// here is on a fader scale, so it is taken as linear.
    pub(super) fn stretched(range: KnobRange, start: f32, zone: f32) -> KnobRange {
        match range.scale {
            Scale::Linear | Scale::Fader => {
                let width = range.max - range.min;
                let min = range.min - width * start / zone;
                KnobRange::linear(min, min + width / zone)
            }
            Scale::Logarithmic => {
                let ratio = range.max / range.min;
                let min = range.min * ratio.powf(-start / zone);
                KnobRange::logarithmic(min, min * ratio.powf(1. / zone))
            }
        }
    }
}

/// The range and the value of the time as the Time knob has it: the ms, or the place of the
/// division while it syncs.
fn time_travel(state: &DelayState) -> (KnobRange, f32, f32) {
    if state.sync {
        let default = place_of(DelayState::default().division);
        (DIVISIONS, place_of(state.division), default)
    } else {
        (TIME_KNOB.range(), state.time_ms, TIME.default)
    }
}

/// The places across of the sound and of its first repeat.
fn gap(state: &DelayState) -> f32 {
    let (range, value, _) = time_travel(state);
    layout::SHORTEST_GAP + layout::GAP_ZONE * range.position(value)
}

/// The height of a level, from 0 to 1 of full level.
fn height_of(level: f32) -> f32 {
    layout::TOP * level
}

/// The repeats, as the curve of the display: a narrow peak for each, from the left.
fn repeats(state: &DelayState) -> Vec<Point<f32>> {
    use layout::{HALF_WIDTH, LEFT, LOWEST, MOST};
    let gap = gap(state);
    let mut curve = vec![point(0., 0.)];
    let mut level = 1.0_f32;
    for number in 1..=MOST {
        let x = LEFT + gap * number as f32;
        if x + HALF_WIDTH > 1. - LEFT || level < LOWEST {
            break;
        }
        curve.extend([
            point(x - HALF_WIDTH, 0.),
            point(x, height_of(level)),
            point(x + HALF_WIDTH, 0.),
        ]);
        level *= state.feedback;
    }
    curve.push(point(1., 0.));
    curve
}

pub struct DelayView {
    session: Entity<Session>,
    delay: Instance<DelayState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of the handle.
    edit: ControlEdit,
    lanes: Entity<Lanes<DelayState>>,
    /// Whether the card shows the cuts and ping-pong. Interface state: not saved.
    expanded: bool,
}

impl DelayView {
    pub fn new(
        session: Entity<Session>,
        delay: Instance<DelayState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.delay.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.delay.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let lanes = Lanes::follow(&session, delay.id(), Delay::AUTOMATION, cx);
        Self {
            session,
            delay,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
        }
    }

    /// Shows or hides the cuts and ping-pong, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut DelayState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, delay) = (&self.session, &self.delay);
        self.edit.apply(session, delay, label, change, set, cx);
    }

    fn knob(&self, control: Control, state: &DelayState, cx: &mut Context<Self>) -> Knob {
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        control
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                view.change(control.undo_label, change, control.parameter.set, cx);
            }))
    }

    /// The Time knob: the ms, or while it syncs the division, a step at a time.
    fn time_knob(&self, state: &DelayState, cx: &mut Context<Self>) -> Knob {
        if !state.sync {
            return self.knob(TIME_KNOB, state, cx);
        }
        let (range, value, default) = time_travel(state);
        Knob::new("division")
            .range(range)
            .step(1.)
            .value(value)
            .default_value(default)
            .label(TIME_KNOB.label)
            .readout(time_readout(state))
            .on_change(weak_callback(cx, |view, change, cx| {
                let set = |state: &mut DelayState, place| state.division = division_at(place);
                view.change(TIME_KNOB.undo_label, change, set, cx);
            }))
    }

    /// The handle on the second repeat: sideways is the time, up and down the feedback. One
    /// drag of it is one undo step for both.
    fn handle(&self, state: &DelayState, cx: &mut Context<Self>) -> Handle {
        use layout::{GAP_ZONE, LEFT, SHORTEST_GAP, TOP};
        let (range, value, default) = time_travel(state);
        let across = layout::stretched(range, LEFT + 2. * SHORTEST_GAP, 2. * GAP_ZONE);
        let x = Axis::new(across, value, default);
        let up = KnobRange::linear(0., 1. / TOP);
        let y = Axis::new(up, state.feedback, FEEDBACK.default);
        let sync = state.sync;
        let lanes = self.lanes.read(cx);
        // While it syncs, the division is the time and a lane of the ms moves nothing here.
        let automated =
            lanes.is_automated(FEEDBACK.field) || (!sync && lanes.is_automated(TIME.field));
        let handle = Handle::new("time-feedback", x, y).automated(automated);
        handle.on_change(weak_callback(
            cx,
            move |view, change: ValueChange<Point<f32>>, cx| {
                let set = move |state: &mut DelayState, at: Point<f32>| {
                    // The travel reaches past both ends of each range, so the handle can sit
                    // on the drawing.
                    if sync {
                        state.division = division_at(at.x);
                    } else {
                        state.time_ms = at.x.clamp(TIME.min, TIME.max);
                    }
                    state.feedback = at.y.clamp(FEEDBACK.min, FEEDBACK.max);
                };
                view.change("Change time and feedback", change, set, cx);
            },
        ))
    }

    fn display(&self, state: &DelayState, cx: &mut Context<Self>) -> Display {
        let caption = format!(
            "Time {} · Feedback {}",
            time_readout(state),
            percent_readout(state.feedback),
        );
        let display = Display::new("display", DISPLAY_WIDTH)
            .curve(repeats(state))
            .marks([point(layout::LEFT, height_of(1.))])
            .handle(self.handle(state, cx))
            .caption(caption);
        // A control that does nothing is not shown: the feel is only for a synced time.
        if !state.sync {
            return display;
        }
        let selected = FEELS.iter().find(|(feel, ..)| *feel == state.feel);
        let selected = selected.map_or("", |(_, value, _)| value);
        let feels = SegmentedControl::new("feel", selected)
            .options(FEELS.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = FEELS.iter().find(|(_, name, _)| *name == value.as_ref());
                if let Some((feel, ..)) = picked {
                    let set = |state: &mut DelayState, feel| state.feel = feel;
                    view.change("Change feel", ValueChange::Set(*feel), set, cx);
                }
            }));
        display.child(feels)
    }

    /// A switch in a cell, which says `On` or `Off`.
    fn switch(
        &self,
        id: &'static str,
        label: &'static str,
        undo_label: &'static str,
        on: bool,
        set: fn(&mut DelayState, bool),
        cx: &mut Context<Self>,
    ) -> Cell {
        let text = if on { "On" } else { "Off" };
        let toggle = Toggle::new(id, text, on)
            .on_change(weak_callback(cx, move |view, on: bool, cx| {
                view.change(undo_label, ValueChange::Set(on), set, cx)
            }));
        Cell::new(toggle).label(label)
    }
}

impl Render for DelayView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.lanes.read(cx).state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let sync = self.switch(
            "sync",
            "Sync",
            "Change sync",
            state.sync,
            |state, on| state.sync = on,
            cx,
        );
        let ping_pong = self.switch(
            "ping-pong",
            "Ping-pong",
            "Change ping-pong",
            state.ping_pong,
            |state, on| state.ping_pong = on,
            cx,
        );
        let columns = [
            Column::new()
                .top(self.time_knob(&state, cx))
                .bottom(knob(FEEDBACK_KNOB, cx)),
            Column::new().top(sync).bottom(knob(MIX_KNOB, cx)),
        ];
        let hidden = [
            Column::new()
                .top(knob(LOW_CUT_KNOB, cx))
                .bottom(knob(HIGH_CUT_KNOB, cx)),
            Column::new().top(ping_pong),
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
    fn the_time_reads_as_a_note_while_it_syncs_and_as_ms_else() {
        let at = |sync, division, feel| {
            time_readout(&DelayState {
                sync,
                division,
                feel,
                time_ms: 330.0,
                ..DelayState::default()
            })
        };
        assert_eq!(at(true, Division::Eighth, Feel::Straight), "1/8");
        assert_eq!(at(true, Division::Eighth, Feel::Dotted), "1/8.");
        assert_eq!(at(true, Division::Sixteenth, Feel::Triplet), "1/16t");
        assert_eq!(at(false, Division::Eighth, Feel::Dotted), "330 ms");
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

    /// Every place of the Time knob while it syncs is a division, and each division is at its
    /// own place.
    #[test]
    fn the_divisions_are_the_steps_of_the_time_knob() {
        for division in Division::ALL {
            assert_eq!(division_at(place_of(division)), division);
            assert_eq!(division_at(place_of(division) + 0.4), division);
        }
        assert_eq!(division_at(-3.), Division::ThirtySecond);
        assert_eq!(division_at(40.), Division::Whole);
        assert_eq!(division_at(DIVISIONS.value(0.)), Division::ThirtySecond);
        assert_eq!(division_at(DIVISIONS.value(1.)), Division::Whole);
    }

    /// The handle is on the second repeat, at every time and at every feedback that draws one,
    /// synced or not, and the repeats are inside the display from left to right.
    #[test]
    fn the_handle_is_on_the_second_repeat() {
        use layout::{GAP_ZONE, LEFT, SHORTEST_GAP};
        let mut states = Vec::new();
        for feedback in [0.1, FEEDBACK.default, FEEDBACK.max] {
            for time_ms in [TIME.min, TIME.default, TIME.max] {
                states.push(DelayState {
                    sync: false,
                    time_ms,
                    feedback,
                    ..DelayState::default()
                });
            }
            for division in Division::ALL {
                states.push(DelayState {
                    division,
                    feedback,
                    ..DelayState::default()
                });
            }
        }
        for state in states {
            let curve = repeats(&state);
            let (range, value, _) = time_travel(&state);
            let across = layout::stretched(range, LEFT + 2. * SHORTEST_GAP, 2. * GAP_ZONE);
            let handle = point(
                across.position(value),
                KnobRange::linear(0., 1. / layout::TOP).position(state.feedback),
            );
            // The peaks are every third point from the second.
            let second = curve[5];
            assert!((second.x - handle.x).abs() < 1e-4, "{state:?}");
            assert!((second.y - handle.y).abs() < 1e-4, "{state:?}");
            for pair in curve.windows(2) {
                assert!(pair[0].x <= pair[1].x, "{state:?}");
            }
            assert!(curve.iter().all(|at| (0. ..=1.).contains(&at.x)));
            assert!(curve.iter().all(|at| (0. ..=layout::TOP).contains(&at.y)));
        }
    }

    /// A stretched range is the same range over its zone, for both scales.
    #[test]
    fn a_stretched_range_puts_the_travel_in_its_zone() {
        for range in [DIVISIONS, TIME_KNOB.range()] {
            let stretched = layout::stretched(range, 0.1, 0.8);
            for position in [0., 0.25, 0.5, 1.] {
                let value = range.scale.value(range.min, range.max, position);
                let place = stretched.position(value);
                assert!((place - (0.1 + 0.8 * position)).abs() < 1e-4, "{range:?}");
            }
        }
    }
}
