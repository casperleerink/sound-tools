//! The card of the EQ: the summed curve with a numbered handle per band, then Frequency, Gain,
//! Q and Shape of the selected band, and behind expand the on and off of each band and the
//! output gain. The rack gives the view a [`CardFrame`]: the picker of the slot as the title,
//! and the close icon.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of a handle is one gesture
//! and one undo step, a key step, a reset, a shape or a switch is one commit. The ranges and
//! the defaults come from the [`Parameter`](sound_core::Parameter)s of the crate. What is only
//! about the interface is here: the label, the unit, the travel of a knob, the name of the
//! undo step, which band is selected and whether the card is expanded.

use gpui::{
    Context, Entity, KeyDownEvent, Point, SharedString, Window, div, point, prelude::*,
};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::dropdown_menu::{
    DropdownMenu, MenuEntry, MenuGroup, MenuItem, MenuPicked, Trigger,
};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, short};
use sound_ui::components::toggle::Toggle;
use sound_ui::{ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use crate::{
    BANDS, Band, EqState, FREQUENCIES, GAIN, OUTPUT_GAIN, Q, Shape, response,
};

/// The name the rack puts on the card of an EQ.
pub const NAME: &str = "EQ";

/// The width of the display. With it and two columns of cells the card is 464 pt, as DESIGN.md
/// gives the EQ.
const DISPLAY_WIDTH: f32 = 312.;

/// The display shows gains from here to there, in dB: the ±15 dB of a band with room for its
/// handle.
const DISPLAY_DB: (f32, f32) = (-18., 18.);

/// The sample rate the curve is drawn for. The curve of another rate differs only near the top
/// of the scale.
const DRAWN_AT: f32 = 48_000.;

/// Points of the curve across the display: two points per point of width or so, for the
/// narrow dip of a notch.
const CURVE_POINTS: usize = 156;

/// Frequencies are heard in ratios, so the display and the frequency knob go across them in
/// ratios, from 20 Hz to 20 kHz.
const ACROSS: KnobRange = KnobRange::logarithmic(20., 20_000.);

/// Up and down on the display is the gain of a band, placed so that the handle is at its gain
/// on the scale of the display.
const GAIN_TRAVEL: KnobRange = KnobRange::linear(DISPLAY_DB.0, DISPLAY_DB.1);

/// Registers the view of the `eq` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(EqView::new);
    devices.describe::<EqState>(|_| DeviceLabel {
        key: EqState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Hertz,
    Decibels,
    /// A Q: a number with no unit.
    Plain,
}

/// A knob of the card, on a number of `S`: a band, or the whole EQ.
struct Control<S: 'static> {
    parameter: &'static sound_core::Parameter<S>,
    label: &'static str,
    undo_label: &'static str,
    unit: Unit,
}

impl<S> Control<S> {
    /// A frequency and a Q are heard in ratios, so their knobs travel in ratios. A gain goes
    /// both ways from 0 dB, so its arc starts at the top.
    fn scale(&self) -> KnobRange {
        let parameter = self.parameter;
        match self.unit {
            Unit::Hertz | Unit::Plain => KnobRange::logarithmic(parameter.min, parameter.max),
            Unit::Decibels => KnobRange::linear(parameter.min, parameter.max),
        }
    }

    fn knob(&self, value: f32) -> Knob {
        let parameter = self.parameter;
        Knob::new(parameter.field)
            .range(self.scale())
            .value(value)
            .default_value(parameter.default)
            .bipolar(matches!(self.unit, Unit::Decibels))
            .label(self.label)
            .readout(readout(self.unit, value))
    }
}

fn frequency_knob(band: usize) -> Control<Band> {
    Control {
        parameter: &FREQUENCIES[band],
        label: "Freq",
        undo_label: "Change frequency",
        unit: Unit::Hertz,
    }
}

const GAIN_KNOB: Control<Band> = Control {
    parameter: &GAIN,
    label: "Gain",
    undo_label: "Change gain",
    unit: Unit::Decibels,
};
const Q_KNOB: Control<Band> = Control {
    parameter: &Q,
    label: "Q",
    undo_label: "Change Q",
    unit: Unit::Plain,
};
const OUTPUT_KNOB: Control<EqState> = Control {
    parameter: &OUTPUT_GAIN,
    label: "Output",
    undo_label: "Change output",
    unit: Unit::Decibels,
};

/// The value of each shape in the select, its label and its icon. The select shows the icon,
/// because no name of a shape but `Bell` fits in a cell, and the cell says the name under it.
const SHAPES: [(Shape, &str, &str, &str); 6] = [
    (Shape::LowCut, "low_cut", "Low cut", "eq-low-cut"),
    (Shape::LowShelf, "low_shelf", "Low shelf", "eq-low-shelf"),
    (Shape::Bell, "bell", "Bell", "eq-bell"),
    (Shape::Notch, "notch", "Notch", "eq-notch"),
    (Shape::HighShelf, "high_shelf", "High shelf", "eq-high-shelf"),
    (Shape::HighCut, "high_cut", "High cut", "eq-high-cut"),
];

/// A value with its unit, as a knob shows it: `632 Hz`, `1.2 kHz`, `-4.5 dB`, `0.71`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Hertz if value < 1_000.0 => format!("{} Hz", short(value)),
        Unit::Hertz => format!("{} kHz", short(value / 1_000.0)),
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Plain => short(value).to_string(),
    }
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
            let gain = response(state, ACROSS.value(x), DRAWN_AT);
            point(x, height_of(20. * gain.max(1e-6).log10()))
        })
        .collect()
}

/// The places across of 100 Hz, 1 kHz and 10 kHz, the scale under the display.
fn decades() -> Vec<f32> {
    [100., 1_000., 10_000.]
        .map(|hz| ACROSS.position(hz))
        .to_vec()
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
            ProjectEvent::Changed(id) if id == view.eq.id() => cx.notify(),
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
            DropdownMenu::new("Shape", entries, cx)
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
        Self {
            session,
            eq,
            frame,
            edit: ControlEdit::default(),
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
            cx.notify();
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
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
    fn band_knob(&self, control: Control<Band>, state: &EqState, cx: &mut Context<Self>) -> Knob {
        let band = self.selected;
        let value = (control.parameter.get)(&state.bands[band]);
        let (set, undo_label) = (control.parameter.set, control.undo_label);
        control
            .knob(value)
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = move |state: &mut EqState, value| set(&mut state.bands[band], value);
                view.change(undo_label, change, set, cx);
            }))
    }

    fn output_knob(&self, state: &EqState, cx: &mut Context<Self>) -> Knob {
        let value = (OUTPUT_KNOB.parameter.get)(state);
        OUTPUT_KNOB
            .knob(value)
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
        let x = Axis::new(ACROSS, settings.frequency_hz, FREQUENCIES[band].default);
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
        Handle::new(SharedString::from(format!("band-{number}")), x, y)
            .label(number.to_string())
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
            .grid(decades(), Vec::new())
            .zero_line(height_of(0.))
            .caption("100 · 1k · 10k");
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
        let Some(state) = self.session.read(cx).project().state(&self.eq).copied() else {
            return div().into_any_element();
        };
        let band = state.bands[self.selected];
        // The select shows the shape of the selected band, also after an outside edit.
        let (_, shape, shape_name, _) = SHAPES[band.shape.index()];
        if self.shapes.read(cx).value().map(SharedString::as_ref) != Some(shape) {
            self.shapes
                .update(cx, |menu, cx| menu.set_selected(shape, cx));
        }
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

    #[test]
    fn a_readout_has_its_unit_and_three_digits_at_most() {
        assert_eq!(readout(Unit::Hertz, 20.0), "20 Hz");
        assert_eq!(readout(Unit::Hertz, 1_200.0), "1.2 kHz");
        assert_eq!(readout(Unit::Decibels, -4.5), "-4.5 dB");
        assert_eq!(readout(Unit::Decibels, 0.0), "0 dB");
        assert_eq!(readout(Unit::Plain, 0.71), "0.71");
        assert_eq!(readout(Unit::Plain, 18.0), "18");
    }

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
                check(control.scale(), parameter.field, min, max, &values);
            }
        }
        let output: &crate::Parameter = OUTPUT_KNOB.parameter;
        let values = [output.min, output.default, output.max];
        check(OUTPUT_KNOB.scale(), output.field, output.min, output.max, &values);
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
