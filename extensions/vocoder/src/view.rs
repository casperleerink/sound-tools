//! The card of the vocoder: its bands across the spectrum, a line each, then Bands, Formant,
//! Release and Mix, and the sidechain picker, always shown: without a voice a vocoder does
//! little. The rack gives the view a [`CardFrame`].
//!
//! The view keeps no copy of the state. It reads the record when it renders, and every change
//! goes through the session by [`DeviceEdit`]. A number that an automation lane of the track
//! moves shows the value that plays and does not drag ([`Lanes`](sound_ui::Lanes)).

use gpui::{Context, Entity, Point, Window, div, point, prelude::*};
use sound_core::Instance;
use sound_ui::components::device_card::{CardFrame, Column, Section};
use sound_ui::components::display::Display;
use sound_ui::components::knob::{
    ParameterKnob, milliseconds_readout, percent_readout, short, signed,
};
use sound_ui::{DeviceEdit, DeviceView, Devices, OfferGroup, Session, Views};

use crate::{BANDS, FORMANT, HIGHEST_HZ, LOWEST_HZ, MIX, RELEASE, Vocoder, VocoderState};

/// The name the rack puts on the card of a vocoder.
pub const NAME: &str = "Vocoder";

/// The width of the display: with two columns of cells the card is as wide as the
/// modulation's.
const DISPLAY_WIDTH: f32 = 200.;

/// The display shows the spectrum from here to there, in octaves.
const LEFT_HZ: f32 = 50.;
const RIGHT_HZ: f32 = 20_000.;

/// How high the line of a band reaches, 0 to 1.
const BAND_HEIGHT: f32 = 0.6;

/// Registers the view of the `vocoder` tool, what a rack calls one and its offer in a picker.
pub fn register(views: &mut Views, devices: &mut Devices) {
    views.register_card(VocoderView::new);
    devices.built_in::<VocoderState>(
        NAME,
        OfferGroup::Tone,
        "device-vocoder",
        crate::EXTENSION,
        "This project does not load the vocoder.",
    );
}

/// A knob of the card.
type Control = ParameterKnob<VocoderState>;

const BANDS_KNOB: Control = Control::new(&BANDS, "Bands", "Change bands", bands_readout);
const FORMANT_KNOB: Control =
    Control::new(&FORMANT, "Formant", "Change formant", formant_readout).bipolar();
const RELEASE_KNOB: Control =
    Control::new(&RELEASE, "Release", "Change release", milliseconds_readout);
const MIX_KNOB: Control = Control::new(&MIX, "Mix", "Change mix", percent_readout);

/// A number of bands, whole as the vocoder plays it: `24`.
fn bands_readout(bands: f32) -> String {
    short(bands.round())
}

/// A move of the formant, with its sign: `+3 st`.
fn formant_readout(semitones: f32) -> String {
    format!("{} st", signed(semitones))
}

/// Where `hz` is across the display, 0 to 1.
fn across(hz: f32) -> f32 {
    (hz / LEFT_HZ).log2() / (RIGHT_HZ / LEFT_HZ).log2()
}

/// A line up from the bottom at the middle of each band.
fn bank(bands: f32) -> Vec<Point<f32>> {
    let bands = bands.round().max(1.);
    let step = (HIGHEST_HZ / LOWEST_HZ).powf(1. / bands);
    (0..bands as i32)
        .map(|band| {
            point(
                across(LOWEST_HZ * step.powf(band as f32 + 0.5)),
                BAND_HEIGHT,
            )
        })
        .collect()
}

/// The numbers under the display: `24 bands · +3 st`.
fn caption(state: &VocoderState) -> String {
    let bands = bands_readout(state.bands);
    match state.formant.abs() < 0.05 {
        true => format!("{bands} bands"),
        false => format!("{bands} bands · {}", formant_readout(state.formant)),
    }
}

pub struct VocoderView {
    device: DeviceEdit<VocoderState>,
    frame: CardFrame,
}

impl VocoderView {
    pub fn new(
        session: Entity<Session>,
        vocoder: Instance<VocoderState>,
        frame: CardFrame,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            device: DeviceEdit::new(session, vocoder, Vocoder::AUTOMATION, cx),
            frame,
        }
    }

    fn display(&self, state: &VocoderState) -> Display {
        let grid = [100., 1_000., 10_000.].map(across).to_vec();
        Display::new("display", DISPLAY_WIDTH)
            .marks(bank(state.bands))
            .grid(grid, Vec::new())
            .caption(caption(state))
    }
}

impl DeviceView for VocoderView {
    type State = VocoderState;

    fn device(&mut self) -> &mut DeviceEdit<VocoderState> {
        &mut self.device
    }
}

impl Render for VocoderView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // `None` once the record is deleted. Whatever hosts the view takes it away then.
        // What plays: the record with the lanes over it.
        let Some(state) = self.device.state(cx) else {
            return div().into_any_element();
        };
        let knob = |control, cx: &mut Context<Self>| self.device.knob(control, &state, cx);
        let columns = [
            Column::new()
                .top(knob(BANDS_KNOB, cx).step(1.))
                .bottom(knob(FORMANT_KNOB, cx)),
            Column::new()
                .top(knob(RELEASE_KNOB, cx))
                .bottom(knob(MIX_KNOB, cx)),
        ];
        let card = self.frame.card().display(self.display(&state));
        let card = columns
            .into_iter()
            .fold(card, |card, column| card.column(column));
        let card = match self.frame.sidechain_column(cx) {
            Some(column) => card.section(Section::new().column(column)),
            None => card,
        };
        card.into_any_element()
    }
}
