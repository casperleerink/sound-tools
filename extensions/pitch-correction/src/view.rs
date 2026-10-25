//! The card of the pitch correction: across its display, a semitone around the note the voice
//! is moved to, at the middle line. A thin line is what the voice sings, and a block from it
//! to a bright line is how far it is moved, to where it plays. Key and Scale sit at its top,
//! Retune and Humanize at its right, and behind expand the voice range, formants, Mix and
//! Ignore vibrato. The rack gives the view a [`CardFrame`].
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session by [`DeviceEdit`]. What the voice sings comes from the watches of
//! the processor, read once per poll of the session, and the card draws again only when that
//! changed.

use gpui::{Context, Entity, Point, SharedString, Task, Window, div, point, prelude::*};
use sound_core::Instance;
use sound_notes::Pitch;
use sound_ui::components::cell::Cell;
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::Display;
use sound_ui::components::dropdown_menu::{MenuEntry, MenuGroup, MenuItem};
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{ParameterKnob, milliseconds_readout, percent_readout, signed};
use sound_ui::components::select::Select;
use sound_ui::components::toggle::Toggle;
use sound_ui::{
    DeviceEdit, DeviceView, Devices, OfferGroup, Session, Views, every_poll, weak_callback,
};

use crate::{
    HUMANIZE, Key, MIX, Meters, PitchCorrection, PitchCorrectionState, RETUNE, Scale, Voice,
};

/// The name the rack puts on the card of a pitch correction.
pub const NAME: &str = "Pitch correction";

/// The width of the display, as the filter's.
const DISPLAY_WIDTH: f32 = 200.;
/// The display reaches this far each side of the note moved to, in cents.
const REACH_CENTS: f32 = 50.;
/// How high the line of the voice and the block of the move reach, 0 to 1.
const HEARD_HEIGHT: f32 = 0.8;
const MOVE_HEIGHT: f32 = 0.55;

/// Registers the view of the `pitch-correction` tool, what a rack calls one and its offer in
/// a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(PitchCorrectionView::new);
    devices.built_in::<PitchCorrectionState>(
        NAME,
        OfferGroup::Tone,
        "device-pitch-correction",
        crate::EXTENSION,
        "This project does not load the pitch correction.",
    );
}

/// A knob of the card.
type Control = ParameterKnob<PitchCorrectionState>;

const RETUNE_KNOB: Control = Control::new(&RETUNE, "Retune", "Change retune", milliseconds_readout);
const HUMANIZE_KNOB: Control =
    Control::new(&HUMANIZE, "Humanize", "Change humanize", percent_readout);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);

/// The words the composer sees for each option of a choice.
const KEYS: [(Key, &str); 12] = [
    (Key::C, "C"),
    (Key::CSharp, "C#"),
    (Key::D, "D"),
    (Key::DSharp, "D#"),
    (Key::E, "E"),
    (Key::F, "F"),
    (Key::FSharp, "F#"),
    (Key::G, "G"),
    (Key::GSharp, "G#"),
    (Key::A, "A"),
    (Key::ASharp, "A#"),
    (Key::B, "B"),
];
const SCALES: [(Scale, &str); 5] = [
    (Scale::Chromatic, "Chromatic"),
    (Scale::Major, "Major"),
    (Scale::Minor, "Minor"),
    (Scale::MajorPentatonic, "Major pentatonic"),
    (Scale::MinorPentatonic, "Minor pentatonic"),
];
const VOICES: [(Voice, &str); 5] = [
    (Voice::High, "High"),
    (Voice::Middle, "Middle"),
    (Voice::Low, "Low"),
    (Voice::Instrument, "Instrument"),
    (Voice::BassInstrument, "Bass"),
];

/// What the processor last said: the note heard and the note moved to, 0 while none, and how
/// far it moves now, in semitones.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Reading {
    heard: f32,
    target: f32,
    correction: f32,
}

/// The name of the note nearest to `note`, a MIDI note number with its fraction.
fn name(note: f32) -> String {
    Pitch::nearest(note.round() as i64).name()
}

/// The line under the display: `A3 −32 ct → A3 · +30 ct`, or a dash while no note is heard.
fn caption(reading: Reading) -> String {
    if reading.heard <= 0. {
        return "–".into();
    }
    let off = ((reading.heard - reading.heard.round()) * 100.).round();
    let heard = format!("{} {} ct", name(reading.heard), signed(off));
    if reading.target <= 0. {
        return heard;
    }
    let moved = (reading.correction * 100.).round();
    format!("{heard} → {} · {} ct", name(reading.target), signed(moved))
}

/// Where a pitch `cents` from the note moved to is across the display, 0 to 1.
fn across(cents: f32) -> f32 {
    (0.5 + cents / (2. * REACH_CENTS)).clamp(0., 1.)
}

/// The block from what is sung to where it plays, left to right.
fn moved(reading: Reading) -> Vec<Point<f32>> {
    if reading.heard <= 0. || reading.target <= 0. {
        return Vec::new();
    }
    let sung = across((reading.heard - reading.target) * 100.);
    let plays = across((reading.heard + reading.correction - reading.target) * 100.);
    let (left, right) = (sung.min(plays), sung.max(plays));
    vec![
        point(left, 0.),
        point(left, MOVE_HEIGHT),
        point(right, MOVE_HEIGHT),
        point(right, 0.),
    ]
}

pub struct PitchCorrectionView {
    device: DeviceEdit<PitchCorrectionState>,
    frame: CardFrame,
    /// Whether the card shows the hidden controls. Interface state: not saved.
    expanded: bool,
    reading: Reading,
    _metering: Task<()>,
}

impl PitchCorrectionView {
    pub fn new(
        session: Entity<Session>,
        correction: Instance<PitchCorrectionState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            device: DeviceEdit::new(session, correction, PitchCorrection::AUTOMATION, cx),
            frame,
            expanded: false,
            reading: Reading::default(),
            _metering: every_poll(cx, Self::read_meters),
        }
    }

    /// Takes what the processor said last, and draws again when it changed.
    pub fn read_meters(&mut self, cx: &mut Context<Self>) {
        let device = &self.device;
        let (project, id) = (device.session.read(cx).project(), device.instance.id());
        let read = |name| project.watched(id, name).unwrap_or(0.);
        let reading = Reading {
            heard: read(Meters::HEARD),
            target: read(Meters::TARGET),
            correction: read(Meters::CORRECTION),
        };
        if reading != self.reading {
            self.reading = reading;
            cx.notify();
        }
    }

    /// Shows or hides the voice range, formants, mix and ignore vibrato, as the expand icon
    /// does.
    pub fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        self.expanded = expanded;
        cx.notify();
    }

    /// A select of one choice of the record: one pick is one step.
    fn select<C: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: &'static [(C, &'static str)],
        value: C,
        undo_label: &'static str,
        set: fn(&mut PitchCorrectionState, C),
        cx: &mut Context<Self>,
    ) -> Select {
        let shown = options.iter().find(|(option, _)| *option == value);
        let items = (options.iter()).map(|(_, label)| MenuItem::new(*label, *label));
        Select::new(id, shown.map_or("", |(_, label)| *label))
            .entries(vec![MenuEntry::Group(MenuGroup::new().items(items))])
            .menu_width(160.)
            .on_change(weak_callback(cx, move |view, picked: SharedString, cx| {
                let option = options.iter().find(|(_, label)| *label == picked.as_ref());
                if let Some((option, _)) = option {
                    view.device
                        .change(undo_label, ValueChange::Set(*option), set, cx);
                }
            }))
    }

    /// A switch of one on or off of the record: one click is one step.
    fn toggle(
        &self,
        id: &'static str,
        on: bool,
        undo_label: &'static str,
        set: fn(&mut PitchCorrectionState, bool),
        cx: &mut Context<Self>,
    ) -> Toggle {
        Toggle::new(id, if on { "On" } else { "Off" }, on).on_change(weak_callback(
            cx,
            move |view, on: bool, cx| {
                view.device
                    .change(undo_label, ValueChange::Set(on), set, cx);
            },
        ))
    }

    fn display(&self, choices: impl IntoElement) -> Display {
        let reading = self.reading;
        let display = Display::new("display", DISPLAY_WIDTH)
            .child(choices)
            .grid([0.25, 0.5, 0.75].to_vec(), Vec::new())
            .curve(moved(reading))
            .caption(caption(reading));
        match reading.heard > 0. && reading.target > 0. {
            true => {
                let sung = across((reading.heard - reading.target) * 100.);
                display.marks([point(sung, HEARD_HEIGHT)])
            }
            false => display,
        }
    }
}

impl DeviceView for PitchCorrectionView {
    type State = PitchCorrectionState;

    fn device(&mut self) -> &mut DeviceEdit<PitchCorrectionState> {
        &mut self.device
    }
}

impl Render for PitchCorrectionView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.device.state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.device.knob(control, &state, cx);
        let key = self.select(
            "key",
            &KEYS,
            state.key,
            "Change key",
            |state, key| state.key = key,
            cx,
        );
        let scale = self.select(
            "scale",
            &SCALES,
            state.scale,
            "Change scale",
            |state, scale| state.scale = scale,
            cx,
        );
        let voice = self.select(
            "voice",
            &VOICES,
            state.voice,
            "Change voice",
            |state, voice| state.voice = voice,
            cx,
        );
        let formants = self.toggle(
            "keep_formants",
            state.keep_formants,
            "Change formants",
            |state, on| state.keep_formants = on,
            cx,
        );
        let vibrato = self.toggle(
            "ignore_vibrato",
            state.ignore_vibrato,
            "Change ignore vibrato",
            |state, on| state.ignore_vibrato = on,
            cx,
        );
        let choices = div().flex().gap_1().child(key).child(scale);
        let retune = Column::new()
            .top(knob(RETUNE_KNOB, cx))
            .bottom(knob(HUMANIZE_KNOB, cx));
        let hidden = [
            Column::new()
                .span(2)
                .top(Cell::new(voice).span(2).label("Voice"))
                .bottom(Cell::new(formants).label("Formants")),
            Column::new()
                .top(knob(MIX_KNOB, cx))
                .bottom(Cell::new(vibrato).label("Ignore vibrato")),
        ];
        let expand = cx.listener(|view, _, _, cx| view.set_expanded(!view.expanded, cx));
        let card = (self.frame.card())
            .expand(self.expanded, expand)
            .display(self.display(choices))
            .column(retune);
        hidden
            .into_iter()
            .fold(card, |card, column| card.hidden_column(column))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caption_says_what_is_sung_what_it_is_moved_to_and_how_far() {
        let reading = Reading {
            heard: 56.68,
            target: 57.,
            correction: 0.3,
        };
        assert_eq!(caption(reading), "A3 −32 ct → A3 · +30 ct");
        let unmoved = Reading {
            target: 0.,
            ..reading
        };
        assert_eq!(caption(unmoved), "A3 −32 ct");
        assert_eq!(caption(Reading::default()), "–");
    }
}
