//! The card of the EQ: the summed curve with a numbered handle per band, then Frequency, Gain,
//! Q and Shape of the selected band, and behind expand the on and off of each band and the
//! output gain. The rack gives the view a [`CardFrame`]: the picker of the slot as the title,
//! and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of a handle is one gesture
//! and one undo step, a key step, a reset, a shape or a switch is one commit. The ranges, the
//! defaults and the travel of each knob come from the [`Parameter`](sound_core::Parameter)s of
//! the crate. What is only about the interface is here: the label, the unit, the name of the
//! undo step, which band is selected and whether the card is expanded. A number that an
//! automation lane of the track moves shows the value that plays, on its knob and on the
//! display, and does not drag ([`Lanes`]).

use gpui::{
    App, Context, Entity, KeyDownEvent, Point, SharedString, Window, div, point, prelude::*,
};
use sound_core::{Instance, ProjectEvent};
use sound_ui::components::cell::Cell;
use sound_ui::components::curves::{DRAWN_AT, RESPONSE_ACROSS, RESPONSE_CAPTION, response_decades};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{
    Knob, KnobRange, ParameterKnob, decibels_readout, hertz_readout, short,
};
use sound_ui::components::toggle::Toggle;
use sound_ui::lanes::object_of;
use sound_ui::{ControlEdit, Devices, Lanes, OfferGroup, Session, Slot, Views, weak_callback};

use crate::{
    BAND_LANES, BANDS, Band, Eq, EqState, FREQUENCIES, GAIN, OUTPUT_GAIN, Q, Shape, response,
};

/// The name the rack puts on the card of an EQ.
pub const NAME: &str = "EQ";

/// The width of the display. With it and two columns of cells the card is 464 pt, as DESIGN.md
/// gives the EQ.
const DISPLAY_WIDTH: f32 = 312.;

/// The display shows gains from here to there, in dB: the ±15 dB of a band with room for its
/// handle.
const DISPLAY_DB: (f32, f32) = (-18., 18.);

/// Points of the curve across the display: two points per point of width or so, for the
/// narrow dip of a notch.
const CURVE_POINTS: usize = 156;

/// Up and down on the display is the gain of a band, placed so that the handle is at its gain
/// on the scale of the display.
const GAIN_TRAVEL: KnobRange = KnobRange::linear(DISPLAY_DB.0, DISPLAY_DB.1);

/// Registers the view of the `eq` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(EqView::new);
    devices.built_in::<EqState>(
        Slot::Effect,
        NAME,
        OfferGroup::Tone,
        "device-eq",
        crate::EXTENSION,
        "This project does not load the EQ.",
    );
}

fn frequency_knob(band: usize) -> ParameterKnob<Band> {
    ParameterKnob::new(
        &FREQUENCIES[band],
        "Freq",
        "Change frequency",
        hertz_readout,
    )
}

/// A gain goes both ways from 0 dB, so its arc starts at the top.
const GAIN_KNOB: ParameterKnob<Band> =
    ParameterKnob::new(&GAIN, "Gain", "Change gain", decibels_readout).bipolar();
/// A Q is a number with no unit.
const Q_KNOB: ParameterKnob<Band> = ParameterKnob::new(&Q, "Q", "Change Q", short);
const OUTPUT_KNOB: ParameterKnob<EqState> =
    ParameterKnob::new(&OUTPUT_GAIN, "Output", "Change output", decibels_readout).bipolar();

/// The value of each shape in the select, its label and its icon. The select shows the icon,
/// because no name of a shape but `Bell` fits in a cell, and the cell says the name under it.
const SHAPES: [(Shape, &str, &str, &str); 6] = [
    (Shape::LowCut, "low_cut", "Low cut", "eq-low-cut"),
    (Shape::LowShelf, "low_shelf", "Low shelf", "eq-low-shelf"),
    (Shape::Bell, "bell", "Bell", "eq-bell"),
    (Shape::Notch, "notch", "Notch", "eq-notch"),
    (
        Shape::HighShelf,
        "high_shelf",
        "High shelf",
        "eq-high-shelf",
    ),
    (Shape::HighCut, "high_cut", "High cut", "eq-high-cut"),
];

/// The value of `shape` in the select.
fn shape_value(shape: Shape) -> &'static str {
    SHAPES[shape.index()].1
}

/// Where a gain in dB is on the display, from 0 at the bottom to 1 at the top.
fn height_of(db: f32) -> f32 {
    GAIN_TRAVEL.position(db).clamp(0., 1.)
}

/// The summed curve across the display.
fn curve(state: &EqState) -> Vec<Point<f32>> {
    (0..=CURVE_POINTS)
        .map(|step| {
            let x = step as f32 / CURVE_POINTS as f32;
            let gain = response(state, RESPONSE_ACROSS.value(x), DRAWN_AT);
            point(x, height_of(20. * gain.max(1e-6).log10()))
        })
        .collect()
}

/// The band a key selects: `1` to `4`, with no modifier.
fn band_of_key(event: &KeyDownEvent) -> Option<usize> {
    let keystroke = &event.keystroke;
    let modifiers = keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
        return None;
    }
    let number: usize = keystroke.key.parse().ok()?;
    (1..=BANDS).contains(&number).then(|| number - 1)
}

pub struct EqView {
    session: Entity<Session>,
    eq: Instance<EqState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of a handle.
    edit: ControlEdit,
    lanes: Entity<Lanes<EqState>>,
    /// Whether the card shows the on and off of the bands and the output. Interface state: not
    /// saved.
    expanded: bool,
    /// The band whose settings the knobs show, from 0. Interface state: not saved.
    selected: usize,
    /// The select of the shape of the selected band.
    shapes: Entity<DropdownMenu>,
}

impl EqView {
    pub fn new(
        session: Entity<Session>,
        eq: Instance<EqState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.eq.id() => {
                view.show_shape(cx);
                cx.notify();
            }
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.eq.id() => {
                view.edit.finish(&view.session, cx);
                cx.notify();
            }
            _ => {}
        })
        .detach();
        // The net under every other way to go: undo and redo wait for an open gesture.
        cx.on_release(|view, cx| view.edit.finish(&view.session, cx))
            .detach();
        let items = SHAPES.map(|(_, value, label, icon)| MenuItem::new(value, label).icon(icon));
        let shapes = cx.new(|cx| {
            let entries = vec![MenuEntry::Group(MenuGroup::new().items(items))];
            // Band 1, the one selected at first.
            let shown = session.read(cx).project().state(&eq);
            let shown = shown.map_or(Band::default_at(0).shape, |state| state.bands[0].shape);
            DropdownMenu::new("Shape", entries, cx)
                .selected(shape_value(shown))
                .trigger(Trigger::Select)
                .width(160.)
                .debug_name("shape")
        });
        cx.subscribe(&shapes, |view, _, MenuPicked(value), cx| {
            let picked = SHAPES.iter().find(|(_, name, ..)| *name == value.as_ref());
            if let Some((shape, ..)) = picked {
                let band = view.selected;
                let set = move |state: &mut EqState, shape| state.bands[band].shape = shape;
                view.change("Change shape", ValueChange::Set(*shape), set, cx);
            }
        })
        .detach();
        let lanes = Lanes::follow(&session, eq.id(), Eq::AUTOMATION, cx);
        Self {
            session,
            eq,
            frame,
            edit: ControlEdit::default(),
            lanes,
            expanded: false,
            selected: 0,
            shapes,
        }
    }

    /// Shows or hides the on and off of the bands and the output, as the expand icon does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    /// Makes band `band`, from 0, the one the knobs show, as a click on its handle does.
    pub fn select(&mut self, band: usize, cx: &mut Context<Self>) {
        if band < BANDS && band != self.selected {
            self.selected = band;
            self.show_shape(cx);
            cx.notify();
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The value the shape select shows: `bell`, `low_cut`.
    pub fn shown_shape<'a>(&self, cx: &'a App) -> Option<&'a str> {
        self.shapes.read(cx).value().map(SharedString::as_ref)
    }

    /// Puts the shape of the selected band in its select, after any change of the record or of
    /// the selected band: a pick, an undo, an outside edit or a click on a handle.
    fn show_shape(&mut self, cx: &mut Context<Self>) {
        let state = self.session.read(cx).project().state(&self.eq);
        let Some(shown) = state.map(|state| shape_value(state.bands[self.selected].shape)) else {
            return;
        };
        self.shapes.update(cx, |select, cx| {
            if select.value().map(AsRef::as_ref) != Some(shown) {
                select.set_selected(shown, cx);
            }
        });
    }

    /// Whether a lane of the track moves the number `field` of band `band`, which it names by
    /// its path: `bands[0].gain_db`.
    fn is_automated(&self, band: usize, field: &str, cx: &Context<Self>) -> bool {
        let band = object_of(BAND_LANES[band][0].field);
        self.lanes.read(cx).is_automated_in(band, field)
    }

    fn change<V>(
        &mut self,
        label: &str,
        change: ValueChange<V>,
        set: impl FnOnce(&mut EqState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, eq) = (&self.session, &self.eq);
        self.edit.apply(session, eq, label, change, set, cx);
    }

    /// A knob on a number of the selected band.
    fn band_knob(
        &self,
        control: ParameterKnob<Band>,
        state: &EqState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let band = self.selected;
        let (set, undo_label) = (control.parameter.set, control.undo_label);
        let automated = self.is_automated(band, control.parameter.field, cx);
        control
            .knob(&state.bands[band])
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = move |state: &mut EqState, value| set(&mut state.bands[band], value);
                view.change(undo_label, change, set, cx);
            }))
    }

    fn output_knob(&self, state: &EqState, cx: &mut Context<Self>) -> Knob {
        let automated = self.lanes.read(cx).is_automated(OUTPUT_GAIN.field);
        OUTPUT_KNOB
            .knob(state)
            .automated(automated)
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = OUTPUT_KNOB.parameter.set;
                view.change(OUTPUT_KNOB.undo_label, change, set, cx);
            }))
    }

    /// The handle of one band: sideways is frequency, up and down is gain. A cut or a notch has
    /// no gain, so its handle sits on the 0 dB line and moves only sideways. A press selects
    /// the band.
    fn handle(&self, band: usize, state: &EqState, cx: &mut Context<Self>) -> Handle {
        let settings = state.bands[band];
        let with_gain = settings.shape.has_gain();
        let x = Axis::new(
            RESPONSE_ACROSS,
            settings.frequency_hz,
            FREQUENCIES[band].default,
        );
        let y = match with_gain {
            true => Axis::new(GAIN_TRAVEL, settings.gain_db, GAIN.default),
            false => Axis::fixed(height_of(0.)),
        };
        let undo_label = match with_gain {
            true => "Change frequency and gain",
            false => "Change frequency",
        };
        let select = weak_callback(cx, move |view: &mut Self, (), cx| view.select(band, cx));
        let number = band + 1;
        let automated = (with_gain && self.is_automated(band, GAIN.field, cx))
            || self.is_automated(band, FREQUENCIES[band].field, cx);
        Handle::new(SharedString::from(format!("band-{number}")), x, y)
            .label(number.to_string())
            .automated(automated)
            .hollow(band != self.selected)
            .dimmed(!settings.on)
            .on_press(move |window, cx| select((), window, cx))
            .on_change(weak_callback(
                cx,
                move |view, change: ValueChange<Point<f32>>, cx| {
                    let set = move |state: &mut EqState, at: Point<f32>| {
                        let settings = &mut state.bands[band];
                        settings.frequency_hz = at.x;
                        // The travel reaches past the range, so that the handle is at its gain
                        // on the scale of the display.
                        if settings.shape.has_gain() {
                            settings.gain_db = at.y.clamp(GAIN.min, GAIN.max);
                        }
                    };
                    view.change(undo_label, change, set, cx);
                },
            ))
    }

    fn display(&self, state: &EqState, cx: &mut Context<Self>) -> Display {
        let display = Display::new("display", DISPLAY_WIDTH)
            .curve(curve(state))
            .grid(response_decades(), Vec::new())
            .zero_line(height_of(0.))
            .caption(RESPONSE_CAPTION);
        // The selected handle last, so that it is on top of any other at its place.
        let order = (0..BANDS)
            .filter(|band| *band != self.selected)
            .chain([self.selected]);
        order.fold(display, |display, band| {
            display.handle(self.handle(band, state, cx))
        })
    }

    /// The on and off of one band, behind expand.
    fn switch(&self, band: usize, state: &EqState, cx: &mut Context<Self>) -> Cell {
        let on = state.bands[band].on;
        let number = band + 1;
        let toggle = Toggle::new(
            SharedString::from(format!("band-{number}-on")),
            if on { "On" } else { "Off" },
            on,
        )
        .on_change(weak_callback(cx, move |view, on: bool, cx| {
            let label = if on { "Turn band on" } else { "Turn band off" };
            let set = move |state: &mut EqState, on| state.bands[band].on = on;
            view.change(label, ValueChange::Set(on), set, cx);
        }));
        Cell::new(toggle).label(format!("Band {number}"))
    }
}

impl Render for EqView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.lanes.read(cx).state(cx) else {
            return div().into_any_element();
        };
        let band = state.bands[self.selected];
        let (_, _, shape_name, _) = SHAPES[band.shape.index()];
        let frequency = self.band_knob(frequency_knob(self.selected), &state, cx);
        let gain = self
            .band_knob(GAIN_KNOB, &state, cx)
            .disabled(!band.shape.has_gain());
        let q = self.band_knob(Q_KNOB, &state, cx);
        let shape = Cell::new(self.shapes.clone())
            .label(format!("Band {}", self.selected + 1))
            .value(shape_name);
        let columns = [
            Column::new().top(frequency).bottom(q),
            Column::new().top(gain).bottom(shape),
        ];
        let switch = |band, cx: &mut Context<Self>| self.switch(band, &state, cx);
        let hidden = [
            // Bands 1 and 2 on the first row, so they read in order across.
            Column::new().top(switch(0, cx)).bottom(switch(2, cx)),
            Column::new().top(switch(1, cx)).bottom(switch(3, cx)),
            Column::new().top(self.output_knob(&state, cx)),
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
        // The keys 1 to 4 select a band from any control of the card that has the focus, which
        // is how the keyboard reaches the bands that the handles select with the pointer.
        div()
            .flex_none()
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                if let Some(band) = band_of_key(event) {
                    view.select(band, cx);
                    cx.stop_propagation();
                }
            }))
            .child(card)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::band_parameters;

    /// The defaults and both ends of every range, through the travel of its knob and back.
    #[test]
    fn every_knob_gives_the_ends_of_its_range_and_keeps_a_value_it_gave() {
        let check = |range: KnobRange, field, min, max, values: &[f32]| {
            assert_eq!(range.value(0.0), min, "{field}");
            assert_eq!(range.value(1.0), max, "{field}");
            for value in values {
                assert_eq!(range.value(range.position(*value)), *value, "{field}");
            }
        };
        for band in 0..BANDS {
            let controls = [frequency_knob(band), GAIN_KNOB, Q_KNOB];
            for (control, parameter) in controls.iter().zip(band_parameters(band)) {
                let values = [parameter.min, parameter.default, parameter.max];
                let (min, max) = (parameter.min, parameter.max);
                check(
                    KnobRange::of(control.parameter),
                    parameter.field,
                    min,
                    max,
                    &values,
                );
            }
        }
        let output: &crate::Parameter = OUTPUT_KNOB.parameter;
        let values = [output.min, output.default, output.max];
        check(
            KnobRange::of(output),
            output.field,
            output.min,
            output.max,
            &values,
        );
    }

    /// The handle of a band with a gain is at its gain on the display, so it sits on the curve
    /// of a band alone: a bell's curve is its gain at its frequency.
    #[test]
    fn the_handle_of_a_bell_is_on_the_curve() {
        for gain_db in [-15., -6., 0., 4.5, 15.] {
            let mut state = EqState::default();
            state.bands[2] = Band {
                gain_db,
                ..state.bands[2]
            };
            let hz = state.bands[2].frequency_hz;
            let curve = height_of(20. * response(&state, hz, DRAWN_AT).log10());
            let handle = GAIN_TRAVEL.position(gain_db);
            assert!((handle - curve).abs() < 1e-3, "{gain_db}: {handle} {curve}");
        }
    }

    #[test]
    fn every_shape_has_a_value_in_the_select() {
        for shape in Shape::ALL {
            assert_eq!(SHAPES[shape.index()].0, shape);
        }
    }
}
