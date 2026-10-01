//! The card of the filter: the response curve with its handle and the type at its top, then
//! Cutoff, Resonance, Drive and Mix, and behind expand the slope and the LFO. The rack gives the
//! view a [`CardFrame`]: the picker of the slot as the title, and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of the handle is one
//! gesture and one undo step, a key step, a reset or a switch is one commit. The ranges, the
//! defaults and the travel of each knob come from the [`Parameter`]s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the
//! card is expanded. A number that an automation lane of the track moves shows the value that
//! plays, on its knob and on the display, and does not drag ([`Lanes`]).

use gpui::{App, Context, Entity, Point, SharedString, Window, div, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::curves::{
    DRAWN_AT, RESPONSE_CAPTION, resonance_travel, response_curve, response_decades, response_height,
};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Lanes, Session, Views, weak_callback};

use crate::{
    CUTOFF, DRIVE, Filter, FilterState, FilterType, LFO_DEPTH, LFO_RATE, MIX, Parameter, RESONANCE,
    Slope, response,
};

/// The name the rack puts on the card of a filter.
pub const NAME: &str = "Filter";

/// The width of the display. With it and two columns of cells the card is 352 pt, as DESIGN.md
/// gives the filter.
const DISPLAY_WIDTH: f32 = 200.;

/// Registers the view of the `filter` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(FilterView::new);
    devices.describe::<FilterState>(|_| DeviceLabel {
        key: FilterState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Hertz,
    Decibels,
    /// A part of one, shown as a percentage.
    Part,
    Octaves,
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
    const fn new(
        parameter: &'static Parameter,
        label: &'static str,
        undo_label: &'static str,
        unit: Unit,
    ) -> Self {
        Self {
            parameter,
            label,
            undo_label,
            scale: KnobRange::of(parameter),
            unit,
        }
    }
}

const CUTOFF_KNOB: Control = Control::new(&CUTOFF, "Cutoff", "Change cutoff", Unit::Hertz);
const RESONANCE_KNOB: Control =
    Control::new(&RESONANCE, "Resonance", "Change resonance", Unit::Part);
const DRIVE_KNOB: Control = Control::new(&DRIVE, "Drive", "Change drive", Unit::Decibels);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", Unit::Part);
const RATE_KNOB: Control = Control::new(&LFO_RATE, "LFO rate", "Change LFO rate", Unit::Hertz);
const DEPTH_KNOB: Control =
    Control::new(&LFO_DEPTH, "LFO depth", "Change LFO depth", Unit::Octaves);

/// Every knob, in the order of the card: shown, then hidden.
#[cfg(test)]
const KNOBS: [&Control; 6] = [
    &CUTOFF_KNOB,
    &RESONANCE_KNOB,
    &DRIVE_KNOB,
    &MIX_KNOB,
    &RATE_KNOB,
    &DEPTH_KNOB,
];

/// The value of a segment and its label, for each type.
const TYPES: [(FilterType, &str, &str); 4] = [
    (FilterType::LowPass, "low_pass", "Low"),
    (FilterType::BandPass, "band_pass", "Band"),
    (FilterType::HighPass, "high_pass", "High"),
    (FilterType::Notch, "notch", "Notch"),
];

const SLOPES: [(Slope, &str, &str); 2] =
    [(Slope::Twelve, "12", "12"), (Slope::TwentyFour, "24", "24")];

/// A value with its unit, as a knob shows it: `632 Hz`, `1.2 kHz`, `6 dB`, `30%`, `2 oct`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Hertz if value < 1_000.0 => format!("{} Hz", short(value)),
        Unit::Hertz => format!("{} kHz", short(value / 1_000.0)),
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Part => format!("{}%", short(value * 100.0)),
        Unit::Octaves => format!("{} oct", short(value)),
    }
}

pub struct FilterView {
    session: Entity<Session>,
    filter: Instance<FilterState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of the handle.
    edit: ControlEdit,
    lanes: Entity<Lanes>,
    /// Whether the card shows the slope and the LFO. Interface state: not saved.
    expanded: bool,
}

impl FilterView {
    pub fn new(
        session: Entity<Session>,
        filter: Instance<FilterState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.filter.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.filter.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let lanes = Lanes::follow(&session, filter.id(), cx);
        Self {
            session,
            filter,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
        }
    }

    /// Shows or hides the slope and the LFO, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    /// The record as the card shows it: what plays, with the value of each lane over it. `None`
    /// once the record is deleted.
    pub fn shown(&self, cx: &App) -> Option<FilterState> {
        let mut state = *self.session.read(cx).project().state(&self.filter)?;
        let parameters = Filter::AUTOMATION.parameters();
        self.lanes.read(cx).apply(parameters, &mut state);
        Some(state)
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut FilterState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, filter) = (&self.session, &self.filter);
        self.edit.apply(session, filter, label, change, set, cx);
    }

    fn knob(&self, control: &'static Control, state: &FilterState, cx: &mut Context<Self>) -> Knob {
        let value = (control.parameter.get)(state);
        let automated = self.lanes.read(cx).is_automated(control.parameter.field);
        Knob::new(control.parameter.field)
            .range(control.scale)
            .value(value)
            .automated(automated)
            .default_value(control.parameter.default)
            .label(control.label)
            .readout(readout(control.unit, value))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = control.parameter.set;
                view.change(control.undo_label, change, set, cx);
            }))
    }

    /// The handle at the cutoff: sideways is cutoff, up and down is resonance. One drag of it
    /// is one undo step for both.
    fn handle(&self, state: &FilterState, cx: &mut Context<Self>) -> Handle {
        let x = Axis::new(CUTOFF_KNOB.scale, state.cutoff_hz, CUTOFF.default);
        let y = Axis::new(
            resonance_travel(state.slope),
            state.resonance,
            RESONANCE.default,
        );
        let lanes = self.lanes.read(cx);
        let automated = lanes.is_automated(CUTOFF.field) || lanes.is_automated(RESONANCE.field);
        let handle = Handle::new("cutoff-resonance", x, y).automated(automated);
        handle.on_change(weak_callback(
            cx,
            |view, change: ValueChange<Point<f32>>, cx| {
                let set = |state: &mut FilterState, at: Point<f32>| {
                    state.cutoff_hz = at.x;
                    // The travel reaches past both ends of the range, so that the handle can
                    // sit on the curve.
                    state.resonance = at.y.clamp(RESONANCE.min, RESONANCE.max);
                };
                view.change("Change cutoff and resonance", change, set, cx);
            },
        ))
    }

    fn display(&self, state: &FilterState, cx: &mut Context<Self>) -> Display {
        let selected = TYPES.iter().find(|(kind, ..)| *kind == state.kind);
        let selected = selected.map_or("", |(_, value, _)| value);
        let types = SegmentedControl::new("type", selected)
            .options(TYPES.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = TYPES.iter().find(|(_, name, _)| *name == value.as_ref());
                if let Some((kind, ..)) = picked {
                    let set = |state: &mut FilterState, kind| state.kind = kind;
                    view.change("Change filter type", ValueChange::Set(*kind), set, cx);
                }
            }));
        Display::new("display", DISPLAY_WIDTH)
            .curve(response_curve(|hz| response(state, hz, DRAWN_AT)))
            .grid(response_decades(), Vec::new())
            .zero_line(response_height(0.))
            .handle(self.handle(state, cx))
            .caption(RESPONSE_CAPTION)
            .child(types)
    }

    fn slope(&self, state: &FilterState, cx: &mut Context<Self>) -> Cell {
        let selected = SLOPES.iter().find(|(slope, ..)| *slope == state.slope);
        let selected = selected.map_or("", |(_, value, _)| value);
        let slopes = SegmentedControl::new("slope", selected)
            .options(SLOPES.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = SLOPES.iter().find(|(_, name, _)| *name == value.as_ref());
                if let Some((slope, ..)) = picked {
                    let set = |state: &mut FilterState, slope| state.slope = slope;
                    view.change("Change slope", ValueChange::Set(*slope), set, cx);
                }
            }));
        Cell::new(slopes).label("Slope").value("dB / oct")
    }
}

impl Render for FilterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self.shown(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(&CUTOFF_KNOB, cx))
                .bottom(knob(&DRIVE_KNOB, cx)),
            Column::new()
                .top(knob(&RESONANCE_KNOB, cx))
                .bottom(knob(&MIX_KNOB, cx)),
        ];
        let hidden = [
            Column::new().top(self.slope(&state, cx)),
            Column::new()
                .top(knob(&RATE_KNOB, cx))
                .bottom(knob(&DEPTH_KNOB, cx)),
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
        assert_eq!(readout(Unit::Hertz, 20.0), "20 Hz");
        assert_eq!(readout(Unit::Hertz, 1_200.0), "1.2 kHz");
        assert_eq!(readout(Unit::Hertz, 0.05), "0.05 Hz");
        assert_eq!(readout(Unit::Decibels, 0.0), "0 dB");
        assert_eq!(readout(Unit::Decibels, 12.5), "12.5 dB");
        assert_eq!(readout(Unit::Part, 0.3), "30%");
        assert_eq!(readout(Unit::Octaves, 2.0), "2 oct");
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
}
