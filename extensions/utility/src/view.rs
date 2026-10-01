//! The card of the utility: the stereo image with the channels at its top and one handle for
//! gain and pan, then Gain, Width, Pan and Mute, and behind expand bass mono and the inverts of
//! the two channels. The rack gives the view a [`CardFrame`]: the picker of the slot as the
//! title, and the power and close icons.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session, by [`ControlEdit`]: a drag of a knob or of the handle is one
//! gesture and one undo step, a key step, a reset or a switch is one commit. The ranges, the
//! defaults and the travel of each knob come from the [`Parameter`]s of the crate. What is only
//! about the interface is here: the label, the unit, the name of the undo step and whether the
//! card is expanded.

use std::f32::consts::FRAC_PI_4;

use gpui::{Context, Entity, Point, SharedString, Window, div, point, prelude::*};
use sound_core::{Instance, ProjectEvent, State};
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::{Axis, Display, Handle};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{Knob, KnobRange, pan_readout, short};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::components::toggle::Toggle;
use sound_ui::{ActiveTheme, ControlEdit, DeviceLabel, Devices, Session, Views, weak_callback};

use crate::{BASS_MONO_HZ, Channels, GAIN, PAN, Parameter, UtilityState, WIDTH, matrix};

/// The name the rack puts on the card of a utility.
pub const NAME: &str = "Utility";

/// The width of the display. With it and two columns of cells the card is 352 pt, as the
/// filter and the reverb are.
const DISPLAY_WIDTH: f32 = 200.;

/// Registers the view of the `utility` tool and what a rack calls one.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(UtilityView::new);
    devices.describe::<UtilityState>(|_| DeviceLabel {
        key: UtilityState::TOOL.into(),
        name: NAME.into(),
    });
}

#[derive(Clone, Copy)]
enum Unit {
    Decibels,
    Pan,
    /// A part of one, shown as a percentage.
    Part,
    Hertz,
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

    /// Gain, pan and width have their default in the middle, and their arc starts there.
    fn bipolar(&self) -> bool {
        !matches!(self.unit, Unit::Hertz)
    }

    /// The name of the knob: its field. Pan is `utility-pan`, because the mixer strip of the
    /// track, in the same panel, has a `pan` of its own, and a test finds a control by its name.
    fn id(&self) -> &'static str {
        match self.unit {
            Unit::Pan => "utility-pan",
            _ => self.parameter.field,
        }
    }
}

const GAIN_KNOB: Control = Control::new(&GAIN, "Gain", "Change gain", Unit::Decibels);
const PAN_KNOB: Control = Control::new(&PAN, "Pan", "Change pan", Unit::Pan);
const WIDTH_KNOB: Control = Control::new(&WIDTH, "Width", "Change width", Unit::Part);
const BASS_KNOB: Control = Control::new(
    &BASS_MONO_HZ,
    "Below",
    "Change bass mono frequency",
    Unit::Hertz,
);

/// A switch of the card, on or off.
struct Switch {
    id: &'static str,
    /// What the toggle says. `None` is `On` or `Off`.
    face: Option<&'static str>,
    label: &'static str,
    undo_label: &'static str,
    get: fn(&UtilityState) -> bool,
    set: fn(&mut UtilityState, bool),
}

/// `utility-mute`, apart from the `mute` of the track in the same panel.
const MUTE: Switch = Switch {
    id: "utility-mute",
    face: Some("M"),
    label: "Mute",
    undo_label: "Change mute",
    get: |state| state.mute,
    set: |state, on| state.mute = on,
};
const BASS_MONO: Switch = Switch {
    id: "bass_mono",
    face: None,
    label: "Bass mono",
    undo_label: "Change bass mono",
    get: |state| state.bass_mono,
    set: |state, on| state.bass_mono = on,
};
const INVERT_LEFT: Switch = Switch {
    id: "invert_left",
    face: Some("L"),
    label: "Invert",
    undo_label: "Change invert left",
    get: |state| state.invert_left,
    set: |state, on| state.invert_left = on,
};
const INVERT_RIGHT: Switch = Switch {
    id: "invert_right",
    face: Some("R"),
    label: "Invert",
    undo_label: "Change invert right",
    get: |state| state.invert_right,
    set: |state, on| state.invert_right = on,
};

/// Every knob, in the order of the card: shown, then hidden.
#[cfg(test)]
const KNOBS: [&Control; 4] = [&GAIN_KNOB, &WIDTH_KNOB, &PAN_KNOB, &BASS_KNOB];

/// The value of a segment and its label, for each choice of channels.
const CHANNELS: [(Channels, &str, &str); 4] = [
    (Channels::Left, "left", "Left"),
    (Channels::Stereo, "stereo", "Stereo"),
    (Channels::Right, "right", "Right"),
    (Channels::Swap, "swap", "Swap"),
];

/// A value with its unit, as a knob shows it: `-6 dB`, `30L`, `C`, `150%`, `120 Hz`.
fn readout(unit: Unit, value: f32) -> String {
    match unit {
        Unit::Decibels => format!("{} dB", short(value)),
        Unit::Pan => pan_readout(value),
        Unit::Part => format!("{}%", short(value * 100.0)),
        Unit::Hertz => format!("{} Hz", short(value)),
    }
}

/// Where things are on the display, as places from 0 to 1, `y` up.
///
/// Across is the stereo field: the left speaker at a quarter, the right one at three quarters,
/// and past them what is wider than the speakers, to twice as wide at the edges. Up is the gain,
/// from its least at the floor to its most at the top, which leaves room for the channels
/// above it.
mod layout {
    use sound_ui::components::knob::KnobRange;

    use crate::GAIN;

    /// A place in the stereo field: -1 is the left speaker and 1 the right.
    pub const WIDEST: f32 = 2.;
    pub const FLOOR: f32 = 0.08;
    pub const TOP: f32 = 0.72;

    pub fn across(place: f32) -> f32 {
        0.5 + place / (2. * WIDEST)
    }

    /// The places of the gain up the display: `position` of a gain is where it is drawn.
    pub fn gain_axis() -> KnobRange {
        let span = (GAIN.max - GAIN.min) / (TOP - FLOOR);
        let min = GAIN.min - FLOOR * span;
        KnobRange::linear(min, min + span)
    }

    /// The places across of the handle: those of the stereo field.
    pub fn place_axis() -> KnobRange {
        KnobRange::linear(-WIDEST, WIDEST)
    }
}

/// Where a sound that comes out at these levels on the left and on the right is heard in the
/// stereo field: the part of it on the right less the part on the left, so -1 is the left
/// speaker and 0 the middle. A sound that is upside down on one side against the other is wider
/// than the speakers. `None` for a sound that is not heard.
fn place(left: f32, right: f32) -> Option<f32> {
    if left == 0. && right == 0. {
        return None;
    }
    // Infinite where the two sides cancel: as wide as the display shows.
    let place = (right - left) / (left + right);
    Some(place.clamp(-layout::WIDEST, layout::WIDEST))
}

/// Where the sound of a mono source is heard at a pan: the ratio of the pan law, `tan(pan π / 4)`.
/// So the handle is on the line of a mono sound.
fn pan_place(pan: f32) -> f32 {
    (pan * FRAC_PI_4).tan()
}

fn place_pan(place: f32) -> f32 {
    (place.atan() / FRAC_PI_4).clamp(PAN.min, PAN.max)
}

/// The stereo image: the input channels where they end up, as a block from the leftmost to the
/// rightmost at the height of the gain. Nothing while muted.
fn image(state: &UtilityState) -> Vec<Point<f32>> {
    use layout::{across, gain_axis};
    let [[left_left, left_right], [right_left, right_right]] = matrix(state);
    // A column of the matrix is where one input channel goes.
    let places = [place(left_left, right_left), place(left_right, right_right)];
    let mut places = places.into_iter().flatten().map(across);
    let Some(first) = places.next() else {
        return Vec::new();
    };
    let (low, high) = places.fold((first, first), |(low, high), x| (low.min(x), high.max(x)));
    let height = gain_axis().position(state.gain_db);
    vec![
        point(low, 0.),
        point(low, height),
        point(high, height),
        point(high, 0.),
    ]
}

pub struct UtilityView {
    session: Entity<Session>,
    utility: Instance<UtilityState>,
    frame: CardFrame,
    /// The gesture of a drag of a knob or of the handle.
    edit: ControlEdit,
    /// Whether the card shows the hidden controls. Interface state: not saved.
    expanded: bool,
}

impl UtilityView {
    pub fn new(
        session: Entity<Session>,
        utility: Instance<UtilityState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&session, |view, _, event, cx| match event {
            ProjectEvent::Changed(id) if id == view.utility.id() => cx.notify(),
            // Deleted under a drag, from outside. The delete was the last write, so the
            // gesture finishes and does not cancel: a cancel would bring the record back.
            ProjectEvent::Deleted(id) if id == view.utility.id() => {
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
            utility,
            frame,
            edit: ControlEdit::default(),
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
        set: impl FnOnce(&mut UtilityState, V),
        cx: &mut Context<Self>,
    ) {
        let (session, utility) = (&self.session, &self.utility);
        self.edit.apply(session, utility, label, change, set, cx);
    }

    fn knob(
        &self,
        control: &'static Control,
        state: &UtilityState,
        cx: &mut Context<Self>,
    ) -> Knob {
        let value = (control.parameter.get)(state);
        Knob::new(control.id())
            .range(control.scale)
            .bipolar(control.bipolar())
            .value(value)
            .default_value(control.parameter.default)
            .label(control.label)
            .readout(readout(control.unit, value))
            .on_change(weak_callback(cx, move |view, change, cx| {
                let set = control.parameter.set;
                view.change(control.undo_label, change, set, cx);
            }))
    }

    /// A switch: one click and one step.
    fn toggle(
        &self,
        switch: &'static Switch,
        state: &UtilityState,
        cx: &mut Context<Self>,
    ) -> Toggle {
        let on = (switch.get)(state);
        let face = switch.face.unwrap_or(if on { "On" } else { "Off" });
        Toggle::new(switch.id, face, on).on_change(weak_callback(cx, move |view, on: bool, cx| {
            view.change(switch.undo_label, ValueChange::Set(on), switch.set, cx);
        }))
    }

    /// The handle on the top of the image: sideways is pan, where a mono sound is heard, and up
    /// and down is gain. Both are one gesture and one undo step.
    fn handle(&self, state: &UtilityState, cx: &mut Context<Self>) -> Handle {
        let x = Axis::new(layout::place_axis(), pan_place(state.pan), 0.);
        let y = Axis::new(layout::gain_axis(), state.gain_db, GAIN.default);
        Handle::new("gain-pan", x, y)
            .dimmed(state.mute)
            .on_change(weak_callback(
                cx,
                |view, change: ValueChange<Point<f32>>, cx| {
                    // The travel reaches past both ends of the ranges.
                    let set = |state: &mut UtilityState, at: Point<f32>| {
                        state.pan = place_pan(at.x);
                        state.gain_db = at.y.clamp(GAIN.min, GAIN.max);
                    };
                    view.change("Change gain and pan", change, set, cx);
                },
            ))
    }

    fn display(&self, state: &UtilityState, cx: &mut Context<Self>) -> Display {
        use layout::{across, gain_axis};
        let selected = CHANNELS
            .iter()
            .find(|(channels, ..)| *channels == state.channels);
        let selected = selected.map_or("", |(_, value, _)| value);
        let channels = SegmentedControl::new("channels", selected)
            .options(CHANNELS.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = CHANNELS.iter().find(|(_, name, _)| *name == value.as_ref());
                if let Some((channels, ..)) = picked {
                    let set = |state: &mut UtilityState, channels| state.channels = channels;
                    view.change("Change channels", ValueChange::Set(*channels), set, cx);
                }
            }));
        let caption = format!(
            "Gain {} · Pan {}",
            readout(Unit::Decibels, state.gain_db),
            pan_readout(state.pan)
        );
        Display::new("display", DISPLAY_WIDTH)
            .curve(image(state))
            .grid([-1., 0., 1.].map(across).to_vec(), Vec::new())
            .zero_line(gain_axis().position(0.))
            .handle(self.handle(state, cx))
            .caption(caption)
            .child(channels)
    }
}

impl Render for UtilityView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        let Some(state) = self
            .session
            .read(cx)
            .project()
            .state(&self.utility)
            .copied()
        else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.knob(control, &state, cx);
        let toggle = |switch: &'static Switch, cx: &mut Context<Self>| {
            Cell::new(self.toggle(switch, &state, cx)).label(switch.label)
        };
        // Peach, as the mute of a track.
        let peach = cx.theme().peach;
        let mute = Cell::new(self.toggle(&MUTE, &state, cx).color(peach)).label(MUTE.label);
        let columns = [
            Column::new()
                .top(knob(&GAIN_KNOB, cx))
                .bottom(knob(&WIDTH_KNOB, cx)),
            Column::new().top(knob(&PAN_KNOB, cx)).bottom(mute),
        ];
        let hidden = [
            Column::new()
                .top(toggle(&BASS_MONO, cx))
                .bottom(knob(&BASS_KNOB, cx)),
            Column::new()
                .top(toggle(&INVERT_LEFT, cx))
                .bottom(toggle(&INVERT_RIGHT, cx)),
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
        assert_eq!(readout(Unit::Decibels, -6.0), "-6 dB");
        assert_eq!(readout(Unit::Decibels, 0.0), "0 dB");
        assert_eq!(readout(Unit::Pan, -0.3), "30L");
        assert_eq!(readout(Unit::Pan, 0.0), "C");
        assert_eq!(readout(Unit::Part, 1.5), "150%");
        assert_eq!(readout(Unit::Hertz, 120.0), "120 Hz");
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

    /// The image spans the speakers as it came, one line in the middle in mono, past the speakers
    /// when wider, and one speaker when only that channel plays and is panned there.
    #[test]
    fn the_image_shows_the_width_and_where_the_sound_is() {
        let spread = |state: UtilityState| {
            let image = image(&state);
            (image[0].x, image[3].x)
        };
        let default = UtilityState::default();
        assert_eq!(spread(default), (0.25, 0.75));
        let mono = UtilityState {
            width: 0.0,
            ..default
        };
        assert_eq!(spread(mono), (0.5, 0.5));
        let wide = UtilityState {
            width: 2.0,
            ..default
        };
        assert_eq!(spread(wide), (0.0, 1.0));
        let right = UtilityState {
            pan: 1.0,
            ..default
        };
        assert_eq!(spread(right), (0.75, 0.75));
        let left_only = UtilityState {
            channels: Channels::Left,
            ..default
        };
        assert_eq!(spread(left_only), (0.5, 0.5));
        let muted = UtilityState {
            mute: true,
            ..default
        };
        assert!(image(&muted).is_empty());
    }

    /// The handle is at the place of a mono sound at its pan, on the top of the image, and a
    /// place dragged to gives that pan back.
    #[test]
    fn the_handle_is_on_the_line_of_a_mono_sound() {
        for pan in [-1.0, -0.4, 0.0, 0.3, 1.0] {
            let state = UtilityState {
                pan,
                width: 0.0,
                gain_db: 6.0,
                ..UtilityState::default()
            };
            let image = image(&state);
            let handle_x = layout::place_axis().position(pan_place(pan));
            assert!((handle_x - image[0].x).abs() < 1e-5, "{pan}: {image:?}");
            assert_eq!(layout::gain_axis().position(6.0), image[1].y);
            assert!((place_pan(pan_place(pan)) - pan).abs() < 1e-6, "{pan}");
        }
        assert_eq!(place_pan(layout::WIDEST), PAN.max);
        let axis = layout::gain_axis();
        assert!((axis.position(GAIN.min) - layout::FLOOR).abs() < 1e-6);
        assert!((axis.position(GAIN.max) - layout::TOP).abs() < 1e-6);
    }
}
