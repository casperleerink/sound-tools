//! The card of the filter: the response curve with its handle and the type at its top, then
//! Cutoff, Resonance, Drive and Mix, and behind expand the slope, the character and the LFO. The rack gives the
//! view a [`CardFrame`]: the picker of the slot as the title, and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change goes
//! through the session, by [`DeviceEdit`]: a drag of a knob or of the handle is one gesture and
//! one undo step, a key step, a reset or a switch is one commit. The ranges, the defaults and the
//! travel of each knob come from the [`Parameter`](crate::Parameter)s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the card
//! is expanded. A number that an automation lane of the track moves shows the value that plays, on
//! its knob and on the display, and does not drag ([`Lanes`](sound_ui::Lanes)).

use gpui::{App, Context, Entity, Point, SharedString, Window, div, prelude::*};
use sound_core::Instance;
use sound_ui::components::cell::Cell;
use sound_ui::components::curves::{
    DRAWN_AT, RESPONSE_CAPTION, resonance_travel, response_curve, response_decades, response_height,
};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    ParameterKnob, decibels_readout, hertz_readout, percent_readout, short,
};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{DeviceEdit, DeviceView, Devices, OfferGroup, Session, Views, weak_callback};

use crate::{
    CUTOFF, DRIVE, Filter, FilterCharacter, FilterState, FilterType, LFO_DEPTH, LFO_RATE, MIX,
    RESONANCE, Slope, response,
};

/// The name the rack puts on the card of a filter.
pub const NAME: &str = "Filter";

/// The width of the display. With it and two columns of cells the card is 352 pt.
const DISPLAY_WIDTH: f32 = 200.;

/// Registers the view of the `filter` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(FilterView::new);
    devices.built_in::<FilterState>(
        NAME,
        OfferGroup::Tone,
        "device-filter",
        crate::EXTENSION,
        "This project does not load the filter.",
    );
}

/// A knob of the card.
type Control = ParameterKnob<FilterState>;

const CUTOFF_KNOB: Control = Control::new(&CUTOFF, "Cutoff", "Change cutoff", hertz_readout);
const RESONANCE_KNOB: Control =
    Control::new(&RESONANCE, "Resonance", "Change resonance", percent_readout);
const DRIVE_KNOB: Control = Control::new(&DRIVE, "Drive", "Change drive", decibels_readout);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);
const RATE_KNOB: Control = Control::new(&LFO_RATE, "LFO rate", "Change LFO rate", hertz_readout);
const DEPTH_KNOB: Control =
    Control::new(&LFO_DEPTH, "LFO depth", "Change LFO depth", octaves_readout);

/// The value of a segment and its label, for each type.
const TYPES: [(FilterType, &str, &str); 4] = [
    (FilterType::LowPass, "low_pass", "Low"),
    (FilterType::BandPass, "band_pass", "Band"),
    (FilterType::HighPass, "high_pass", "High"),
    (FilterType::Notch, "notch", "Notch"),
];

const SLOPES: [(Slope, &str, &str); 2] =
    [(Slope::Twelve, "12", "12"), (Slope::TwentyFour, "24", "24")];

const CHARACTERS: [(FilterCharacter, &str, &str); 2] = [
    (FilterCharacter::Clean, "clean", "Clean"),
    (FilterCharacter::Warm, "warm", "Warm"),
];

/// A depth in octaves: `2 oct`.
fn octaves_readout(octaves: f32) -> String {
    format!("{} oct", short(octaves))
}

pub struct FilterView {
    device: DeviceEdit<FilterState>,
    frame: CardFrame,
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
        Self {
            device: DeviceEdit::new(session, filter, Filter::AUTOMATION, cx),
            frame,
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
        self.device.state(cx)
    }

    /// The handle at the cutoff: sideways is cutoff, up and down is resonance. One drag of it
    /// is one undo step for both.
    fn handle(&self, state: &FilterState, cx: &mut Context<Self>) -> Handle {
        let x = Axis::new(CUTOFF_KNOB.range(), state.cutoff_hz, CUTOFF.default);
        let y = Axis::new(
            resonance_travel(state.slope),
            state.resonance,
            RESONANCE.default,
        );
        let device = &self.device;
        let automated =
            device.is_automated(CUTOFF.field, cx) || device.is_automated(RESONANCE.field, cx);
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
                view.device
                    .change("Change cutoff and resonance", change, set, cx);
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
                    let change = ValueChange::Set(*kind);
                    view.device.change("Change filter type", change, set, cx);
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
                    let change = ValueChange::Set(*slope);
                    view.device.change("Change slope", change, set, cx);
                }
            }));
        Cell::new(slopes).label("Slope").value("dB / oct")
    }

    fn character(&self, state: &FilterState, cx: &mut Context<Self>) -> Cell {
        let selected = CHARACTERS
            .iter()
            .find(|(character, ..)| *character == state.character);
        let selected = selected.map_or("", |(_, value, _)| value);
        let characters = SegmentedControl::new("character", selected)
            .options(CHARACTERS.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = CHARACTERS
                    .iter()
                    .find(|(_, name, _)| *name == value.as_ref());
                if let Some((character, ..)) = picked {
                    let set = |state: &mut FilterState, character| state.character = character;
                    let change = ValueChange::Set(*character);
                    view.device.change("Change character", change, set, cx);
                }
            }));
        Cell::new(characters).span(2).label("Character")
    }
}

impl DeviceView for FilterView {
    type State = FilterState;

    fn device(&mut self) -> &mut DeviceEdit<FilterState> {
        &mut self.device
    }
}

impl Render for FilterView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self.shown(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.device.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(CUTOFF_KNOB, cx))
                .bottom(knob(DRIVE_KNOB, cx)),
            Column::new()
                .top(knob(RESONANCE_KNOB, cx))
                .bottom(knob(MIX_KNOB, cx)),
        ];
        let hidden = [
            Column::new()
                .span(2)
                .top(self.slope(&state, cx))
                .bottom(self.character(&state, cx)),
            Column::new()
                .top(knob(RATE_KNOB, cx))
                .bottom(knob(DEPTH_KNOB, cx)),
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
    fn a_depth_reads_in_octaves() {
        assert_eq!(octaves_readout(2.0), "2 oct");
        assert_eq!(octaves_readout(0.25), "0.25 oct");
    }
}
