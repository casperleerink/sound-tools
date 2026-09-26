//! The card of the filter: the response curve with its handle and the type at its top, then
//! Cutoff, Resonance, Drive and Mix, and behind expand the slope and the LFO. The rack gives the
//! view a [`CardFrame`]: the picker of the slot as the title, and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of the handle is one
//! gesture and one undo step, a key step, a reset or a switch is one commit. The ranges and
//! the defaults come from the [`Parameter`]s of the crate. What is only about the interface is
//! here: the label, the unit, the travel of a knob, the name of the undo step and whether the
//! card is expanded.

use gpui::{Context, Entity, Point, SharedString, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use crate::{
    CUTOFF, DRIVE, FilterState, FilterType, LFO_DEPTH, LFO_RATE, MIX, Parameter, RESONANCE, Slope,
    response,
};

/// The name the rack puts on the card of a filter.
pub const NAME: &str = "Filter";

/// The width of the display. With it and two columns of cells the card is 352 pt, as DESIGN.md
/// gives the filter.
const DISPLAY_WIDTH: f32 = 200.;

/// The display shows gains from here to there, in dB. The top leaves room for the peak of full
/// resonance, +11.5 dB.
const DISPLAY_DB: (f32, f32) = (-36., 18.);

/// The sample rate the curve is drawn for. The curve of another rate differs only near the top
/// of the scale, where the display has no room to show it.
const DRAWN_AT: f32 = 48_000.;

/// Points of the curve across the display.
const CURVE_POINTS: usize = 96;

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
    /// Frequencies are heard in ratios, so their knobs travel in ratios.
    const fn new(
        parameter: &'static Parameter,
        label: &'static str,
        undo_label: &'static str,
        unit: Unit,
    ) -> Self {
        let scale = match unit {
            Unit::Hertz => KnobRange::logarithmic(parameter.min, parameter.max),
            Unit::Decibels | Unit::Part | Unit::Octaves => {
                KnobRange::linear(parameter.min, parameter.max)
            }
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

/// Where a gain in dB is on the display, from 0 at the bottom to 1 at the top.
fn height_of(db: f32) -> f32 {
    let (bottom, top) = DISPLAY_DB;
    ((db - bottom) / (top - bottom)).clamp(0., 1.)
}

/// The response curve across the display, which spans the range of the cutoff.
fn curve(state: &FilterState) -> Vec<Point<f32>> {
    let across = CUTOFF_KNOB.scale;
    (0..=CURVE_POINTS)
        .map(|step| {
            let x = step as f32 / CURVE_POINTS as f32;
            let gain = response(state, across.value(x), DRAWN_AT);
            point(x, height_of(20. * gain.max(1e-6).log10()))
        })
        .collect()
}

/// Up and down on the display is resonance, placed so that the handle sits on the peak of a
/// low or high pass: its gain at the cutoff is the product of the Qs of the sections, which is
/// a straight line in dB over resonance. So the handle is where the curve is, and the range of
/// its travel is that line stretched over the height of the display.
fn resonance_travel(slope: Slope) -> KnobRange {
    let db_at = |resonance| {
        let state = FilterState {
            resonance,
            slope,
            mix: 1.,
            kind: FilterType::LowPass,
            ..FilterState::default()
        };
        20. * response(&state, state.cutoff_hz, DRAWN_AT).log10()
    };
    let (at_none, at_full) = (height_of(db_at(0.)), height_of(db_at(1.)));
    let per_resonance = at_full - at_none;
    KnobRange::linear(-at_none / per_resonance, (1. - at_none) / per_resonance)
}

/// The places across of 100 Hz, 1 kHz and 10 kHz, the scale under the display.
fn decades() -> Vec<f32> {
    [100., 1_000., 10_000.]
        .map(|hz| CUTOFF_KNOB.scale.position(hz))
        .to_vec()
}

pub struct FilterView {
    session: Entity<Session>,
    filter: Instance<FilterState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of the handle.
    edit: ControlEdit,
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
        Self {
            session,
            filter,
            frame,
            edit: ControlEdit::default(),
            expanded: false,
        }
    }

    /// Shows or hides the slope and the LFO, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
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

    /// The handle at the cutoff: sideways is cutoff, up and down is resonance. One drag of it
    /// is one undo step for both.
    fn handle(&self, state: &FilterState, cx: &mut Context<Self>) -> Handle {
        let x = Axis::new(CUTOFF_KNOB.scale, state.cutoff_hz, CUTOFF.default);
        let y = Axis::new(
            resonance_travel(state.slope),
            state.resonance,
            RESONANCE.default,
        );
        Handle::new("cutoff-resonance", x, y).on_change(weak_callback(
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
            .curve(curve(state))
            .grid(decades(), Vec::new())
            .zero_line(height_of(0.))
            .handle(self.handle(state, cx))
            .caption("100 · 1k · 10k")
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
        let Some(state) = self.session.read(cx).project().state(&self.filter).copied() else {
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

    /// The handle sits on the peak of a low pass, at every resonance and both slopes.
    #[test]
    fn the_handle_is_on_the_curve_at_the_cutoff() {
        for slope in Slope::ALL {
            for resonance in [0., 0.3, 0.7, 1.] {
                let state = FilterState {
                    resonance,
                    slope,
                    ..FilterState::default()
                };
                let handle = resonance_travel(slope).position(resonance);
                let gain = response(&state, state.cutoff_hz, DRAWN_AT);
                let curve = height_of(20. * gain.log10());
                assert!((handle - curve).abs() < 1e-4, "{slope:?} {resonance}");
            }
        }
    }
}
