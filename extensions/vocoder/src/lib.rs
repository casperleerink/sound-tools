//! Vocoder: the built-in vocoder. It goes in an effect slot of a track, such as a synth, and
//! another track, such as a voice, keys it through the `sidechain` input: the sound of the
//! track takes on the shape of the voice's spectrum, band by band, so the synth speaks. With
//! nothing on the sidechain it follows its own sound.
//!
//! It works on the spectrum through [`sound_dsp::Stft`], the transform that `spectral` in a
//! tool of the project uses too.
//!
//! [`view`] is the card of the vocoder, and the only module here that uses GPUI.

mod processor;
pub mod view;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State, ranges,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, SIDECHAIN_INPUT};

pub use processor::{HIGHEST_HZ, LOWEST_HZ, SIZE, Vocoder};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "vocoder";

/// The saved state. It is small and `Copy`, so it is also the update the processor gets.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default vocoder.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
#[schemars(transform = ranges(&PARAMETERS))]
pub struct VocoderState {
    /// How many bands the spectrum is split into, rounded to a whole number. More is clearer
    /// speech, fewer a more robotic sound.
    pub bands: f32,
    /// Moves the shape of the voice up or down, in semitones: up for a smaller voice, down for
    /// a bigger one.
    pub formant: f32,
    /// How long a band takes to fall when the voice gets quieter.
    pub release_ms: f32,
    /// How much of the vocoded sound plays, 0 to 1; the rest is the track as it comes.
    pub mix: f32,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<VocoderState>;

pub const BANDS: Parameter = Parameter {
    field: "bands",
    min: 8.0,
    max: 64.0,
    default: 24.0,
    scale: Scale::Linear,
    get: |state| state.bands,
    set: |state, value| state.bands = value,
};
pub const FORMANT: Parameter = Parameter {
    field: "formant",
    min: -12.0,
    max: 12.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |state| state.formant,
    set: |state, value| state.formant = value,
};
pub const RELEASE: Parameter = Parameter {
    field: "release_ms",
    min: 5.0,
    max: 1_000.0,
    default: 60.0,
    scale: Scale::Logarithmic,
    get: |state| state.release_ms,
    set: |state, value| state.release_ms = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    scale: Scale::Linear,
    get: |state| state.mix,
    set: |state, value| state.mix = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 4] = [&BANDS, &FORMANT, &RELEASE, &MIX];

impl Default for VocoderState {
    fn default() -> Self {
        Self {
            bands: BANDS.default,
            formant: FORMANT.default,
            release_ms: RELEASE.default,
            mix: MIX.default,
        }
    }
}

impl State for VocoderState {
    const TOOL: &'static str = "vocoder";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the vocoder record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "vocoder",
    group: "Devices",
    when: "A vocoder on a track: a talking synth",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the vocoder tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<VocoderState>(EXTENSION)?
        .behaviour(apply)
        .schema(Vocoder::AUTOMATION.parameters());
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit.
fn apply(state: &VocoderState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let vocoder = context.processor("vocoder", || Vocoder::new(*state))?;
    context.update(vocoder, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(vocoder, Vocoder::INPUT));
    context.input(
        SIDECHAIN_INPUT,
        InputEndpoint::new(vocoder, Vocoder::SIDECHAIN),
    );
    context.automation(vocoder, Vocoder::AUTOMATION);
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(vocoder, Vocoder::OUTPUT));
    Ok(())
}
