//! The card of the synth in a rack: the envelope as a display whose handles drag, the waveform
//! at the top of it, the main knobs and the character next to it and the envelope knobs behind
//! expand. The rack
//! gives the frame of the card, whose title says "Synth" and is where another instrument is
//! picked.
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change goes
//! through the session, by [`DeviceEdit`]: a knob or a handle drag is one gesture and one undo
//! step, a key step, a reset or a waveform switch is one commit. A handle edits the same field as
//! its knob, under the same name in the history, and the knob is the way to that value from the
//! keys. The ranges, the defaults and the travel of each knob come from the
//! [`Parameter`](crate::Parameter)s of the crate. What is only about the interface is here: the
//! label, the unit, the name of the undo step and whether the card is expanded. A number that an
//! automation lane of the track moves shows the value that plays, on its knob and on the display,
//! and does not drag ([`Lanes`](sound_ui::Lanes)).

use gpui::{Context, Entity, Point, SharedString, Window, div, prelude::*};
use sound_core::Instance;
use sound_ui::components::cell::Cell;
use sound_ui::components::curves::{Adsr, EnvelopeHandle, envelope_display};
use sound_ui::components::device_card::{CardFrame, Column};
use sound_ui::components::display::Display;
use sound_ui::components::gesture::ValueChange;
use sound_ui::components::knob::{ParameterKnob, hertz_readout, percent_readout, seconds_readout};
use sound_ui::components::segmented_control::SegmentedControl;
use sound_ui::{DeviceEdit, DeviceView, Devices, OfferGroup, Session, Views, weak_callback};

use crate::{
    ATTACK, CUTOFF, DECAY, FilterCharacter, GAIN, RELEASE, RESONANCE, SUSTAIN, Synth, SynthState,
    Waveform,
};

/// The name the rack puts on the card of a synth.
pub const NAME: &str = "Synth";

/// Registers the card of the `instrument.synth` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(SynthView::new);
    devices.built_in::<SynthState>(
        NAME,
        OfferGroup::BuiltIn,
        "device-synth",
        crate::EXTENSION,
        "This project does not load the synth.",
    );
}

/// A knob of the view.
type Control = ParameterKnob<SynthState>;

const CUTOFF_KNOB: Control = Control::new(&CUTOFF, "Cutoff", "Change cutoff", hertz_readout);
const RESONANCE_KNOB: Control =
    Control::new(&RESONANCE, "Resonance", "Change resonance", percent_readout);
const GAIN_KNOB: Control = Control::new(&GAIN, "Gain", "Change gain", percent_readout);
const ATTACK_KNOB: Control = Control::new(&ATTACK, "Attack", "Change attack", seconds_readout);
const DECAY_KNOB: Control = Control::new(&DECAY, "Decay", "Change decay", seconds_readout);
const SUSTAIN_KNOB: Control = Control::new(&SUSTAIN, "Sustain", "Change sustain", percent_readout);
const RELEASE_KNOB: Control = Control::new(&RELEASE, "Release", "Change release", seconds_readout);

/// The value of the segmented control and the label of each waveform.
const WAVEFORMS: [(Waveform, &str, &str); 2] = [
    (Waveform::Saw, "saw", "Saw"),
    (Waveform::Square, "square", "Square"),
];

const CHARACTERS: [(FilterCharacter, &str, &str); 2] = [
    (FilterCharacter::Clean, "clean", "Clean"),
    (FilterCharacter::Warm, "warm", "Warm"),
];

/// The width of the display: the synth card is 352 pt, with two columns of cells.
const DISPLAY_WIDTH: f32 = 200.;

pub struct SynthView {
    device: DeviceEdit<SynthState>,
    /// The title and the close icon the rack gives the card.
    frame: CardFrame,
    /// Whether the card shows the envelope knobs. Interface state: nothing saves it.
    expanded: bool,
}

impl SynthView {
    pub fn new(
        session: Entity<Session>,
        synth: Instance<SynthState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            device: DeviceEdit::new(session, synth, Synth::AUTOMATION, cx),
            frame,
            expanded: false,
        }
    }

    /// The envelope, with a handle at the end of each stage, and the waveform at its top. A
    /// handle edits the field its knob edits, under the same name, and the corner after the
    /// decay moves the decay and the sustain in one undo step.
    fn display(&self, state: &SynthState, cx: &mut Context<Self>) -> Display {
        let adsr = Adsr {
            attack: state.attack_seconds,
            decay: state.decay_seconds,
            sustain: state.sustain,
            release: state.release_seconds,
        };
        let defaults = Adsr {
            attack: ATTACK.default,
            decay: DECAY.default,
            sustain: SUSTAIN.default,
            release: RELEASE.default,
        };
        let lanes = self.device.lanes.read(cx);
        let automated = Adsr {
            attack: lanes.is_automated(ATTACK.field),
            decay: lanes.is_automated(DECAY.field),
            sustain: lanes.is_automated(SUSTAIN.field),
            release: lanes.is_automated(RELEASE.field),
        };
        let on_change = weak_callback(
            cx,
            |view, (handle, change): (EnvelopeHandle, ValueChange<Point<f32>>), cx| {
                let (label, set): (_, fn(&mut SynthState, Point<f32>)) = match handle {
                    EnvelopeHandle::Attack => (ATTACK_KNOB.undo_label, |state, place| {
                        state.attack_seconds = ATTACK_KNOB.clamp(place.x)
                    }),
                    EnvelopeHandle::Decay => ("Change decay and sustain", |state, place| {
                        state.decay_seconds = DECAY_KNOB.clamp(place.x);
                        state.sustain = SUSTAIN_KNOB.clamp(place.y);
                    }),
                    EnvelopeHandle::Release => (RELEASE_KNOB.undo_label, |state, place| {
                        state.release_seconds = RELEASE_KNOB.clamp(place.x)
                    }),
                };
                view.device.change(label, change, set, cx);
            },
        );
        let caption = format!(
            "A {} · D {} · S {} · R {}",
            seconds_readout(adsr.attack),
            seconds_readout(adsr.decay),
            percent_readout(adsr.sustain),
            seconds_readout(adsr.release),
        );
        // The synth's own stages are drawn straight.
        let straight = [0.; 3];
        let time = ATTACK_KNOB.range();
        envelope_display(
            "envelope",
            DISPLAY_WIDTH,
            time,
            (adsr, defaults, automated),
            straight,
            on_change,
        )
        .caption(caption)
        .child(div().ml_auto().child(self.waveform(state, cx)))
    }

    fn waveform(&self, state: &SynthState, cx: &mut Context<Self>) -> SegmentedControl {
        let selected = WAVEFORMS
            .iter()
            .find(|(waveform, ..)| *waveform == state.waveform);
        let selected = selected.map_or("", |(_, value, _)| value);
        SegmentedControl::new("waveform", selected)
            .options(WAVEFORMS.map(|(_, value, label)| (value, label)))
            .on_change(weak_callback(cx, |view, value: SharedString, cx| {
                let picked = WAVEFORMS
                    .iter()
                    .find(|(_, name, _)| *name == value.as_ref());
                if let Some((waveform, ..)) = picked {
                    let change = ValueChange::Set(*waveform);
                    let set = |state: &mut SynthState, waveform| state.waveform = waveform;
                    view.device.change("Change waveform", change, set, cx);
                }
            }))
    }
}

impl SynthView {
    fn character(&self, state: &SynthState, cx: &mut Context<Self>) -> Cell {
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
                    let change = ValueChange::Set(*character);
                    let set = |state: &mut SynthState, character| state.character = character;
                    view.device.change("Change character", change, set, cx);
                }
            }));
        Cell::new(characters).span(2).label("Character")
    }
}

impl DeviceView for SynthView {
    type State = SynthState;

    fn device(&mut self) -> &mut DeviceEdit<SynthState> {
        &mut self.device
    }
}

impl Render for SynthView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.device.state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.device.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(CUTOFF_KNOB, cx))
                .bottom(knob(GAIN_KNOB, cx)),
            Column::new()
                .span(2)
                .top(knob(RESONANCE_KNOB, cx))
                .bottom(self.character(&state, cx)),
        ];
        // Behind expand: the times and the level the handles of the display move, so the keys
        // reach every one of them. Read across as A, D, then S, R.
        let hidden = [
            Column::new()
                .top(knob(ATTACK_KNOB, cx))
                .bottom(knob(SUSTAIN_KNOB, cx)),
            Column::new()
                .top(knob(DECAY_KNOB, cx))
                .bottom(knob(RELEASE_KNOB, cx)),
        ];
        let expand = cx.listener(|view, _, _, cx| {
            view.expanded = !view.expanded;
            cx.notify();
        });
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
