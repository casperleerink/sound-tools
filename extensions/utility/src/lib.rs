//! Utility: the built-in utility effect. It goes in an effect slot of a track, like an effect
//! plugin, and does the plain jobs of a mix: gain, pan, stereo width, the bass in mono, which
//! channels play, a channel turned upside down, and mute. At its defaults it leaves every
//! sample as it came.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "utility",
//!   "state": {
//!     "gain_db": 0.0,
//!     "pan": 0.0,
//!     "width": 1.0,
//!     "bass_mono": false,
//!     "bass_mono_hz": 120.0,
//!     "channels": "stereo",
//!     "invert_left": false,
//!     "invert_right": false,
//!     "mute": false
//!   }
//! }
//! ```
//!
//! Known gap: turning bass mono on or off fades from the sound as it came to the sound through
//! the crossover over 20 ms. The crossover turns the phase near its frequency, so for those
//! 20 ms the bass there dips. It does not click.
//!
//! [`view`] is the card of the utility, and the only module here that uses GPUI.

mod processor;
pub mod view;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State, ranges,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{Utility, matrix};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "utility";

/// Which input channels play, before anything else.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Channels {
    /// Left plays left and right plays right.
    Stereo,
    /// The left channel in both.
    Left,
    /// The right channel in both.
    Right,
    /// Left plays right and right plays left.
    Swap,
}

impl Channels {
    pub const ALL: [Self; 4] = [Self::Left, Self::Stereo, Self::Right, Self::Swap];
}

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// memory of the bass mono crossover is runtime state and is not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default utility,
/// which changes nothing.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
#[schemars(transform = ranges(&PARAMETERS))]
pub struct UtilityState {
    /// Turns the sound up or down.
    pub gain_db: f32,
    /// -1 is left, 1 is right, with the pan law of a track: a sound keeps its loudness wherever
    /// it is. Hard left, the left channel is 3 dB up and the right one is silent.
    pub pan: f32,
    /// 0 is mono, 1 is the sound as it came, 2 is twice as wide. It scales the difference of the
    /// two channels and keeps their sum.
    pub width: f32,
    /// The sound below [`Self::bass_mono_hz`] in mono, and the rest as wide as it was. The two
    /// bands add up flat.
    pub bass_mono: bool,
    /// Where the mono bass ends.
    pub bass_mono_hz: f32,
    pub channels: Channels,
    /// Turns the left channel upside down: its polarity, which a phase button flips.
    pub invert_left: bool,
    /// Turns the right channel upside down.
    pub invert_right: bool,
    /// Silence out. To mute the whole track, use `mute` of the track.
    pub mute: bool,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<UtilityState>;

pub const GAIN: Parameter = Parameter {
    field: "gain_db",
    min: -36.0,
    max: 36.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |state| state.gain_db,
    set: |state, value| state.gain_db = value,
};
pub const PAN: Parameter = Parameter {
    field: "pan",
    min: -1.0,
    max: 1.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |state| state.pan,
    set: |state, value| state.pan = value,
};
pub const WIDTH: Parameter = Parameter {
    field: "width",
    min: 0.0,
    max: 2.0,
    default: 1.0,
    scale: Scale::Linear,
    get: |state| state.width,
    set: |state, value| state.width = value,
};
pub const BASS_MONO_HZ: Parameter = Parameter {
    field: "bass_mono_hz",
    min: 50.0,
    max: 500.0,
    default: 120.0,
    scale: Scale::Logarithmic,
    get: |state| state.bass_mono_hz,
    set: |state, value| state.bass_mono_hz = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 4] = [&GAIN, &PAN, &WIDTH, &BASS_MONO_HZ];

impl Default for UtilityState {
    fn default() -> Self {
        Self {
            gain_db: GAIN.default,
            pan: PAN.default,
            width: WIDTH.default,
            bass_mono: false,
            bass_mono_hz: BASS_MONO_HZ.default,
            channels: Channels::Stereo,
            invert_left: false,
            invert_right: false,
            mute: false,
        }
    }
}

impl State for UtilityState {
    const TOOL: &'static str = "utility";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the utility record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "utility",
    when: "You change gain, pan or width inside a chain, make the bass mono, or flip a channel",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the utility tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<UtilityState>(EXTENSION)?
        .behaviour(apply)
        .in_code(Utility::AUTOMATION.parameters());
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and every change glides.
fn apply(state: &UtilityState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let utility = context.processor("utility", || Utility::new(*state))?;
    context.update(utility, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(utility, Utility::INPUT));
    context.automation(utility, Utility::AUTOMATION);
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(utility, Utility::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use sound_core::testing::check_parameter_ranges;

    use super::*;

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        let state = UtilityState::default();
        assert_eq!(
            check_parameter_ranges(&state, &PARAMETERS, |state| state),
            Ok(())
        );
    }
}
