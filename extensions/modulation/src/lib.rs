//! Modulation: the built-in chorus, flanger and phaser, one effect with a mode. It goes in an
//! effect slot of a track, like an effect plugin, and moves the sound with an LFO: a chorus
//! thickens it with a copy whose pitch wobbles, a flanger sweeps a comb of notches through it,
//! a phaser sweeps a few wide notches. The rate, depth, feedback, stereo spread and mix mean
//! the same in every mode.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "modulation",
//!   "state": {"mode": "chorus", "rate_hz": 0.5, "depth": 0.5, "feedback": 0.2, "spread": 0.5, "mix": 0.5}
//! }
//! ```
//!
//! [`view`] is the card of the modulation, and the only module here that uses GPUI.

mod processor;
pub mod view;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State, ranges,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{Modulation, response, sweep};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "modulation";

/// What the LFO moves.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// The delay of a copy of the sound, around 12 ms: its pitch wobbles against the dry sound.
    #[default]
    Chorus,
    /// The delay of a copy, around 1.5 ms: with the dry sound it makes a comb of notches that
    /// sweeps up and down.
    Flanger,
    /// Six allpass filters around 1 kHz: with the dry sound they make three wide notches that
    /// sweep up and down.
    Phaser,
}

impl Mode {
    pub const ALL: [Self; 3] = [Self::Chorus, Self::Flanger, Self::Phaser];
}

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// sound in the delay line and the phase of the LFO are runtime state and are not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default chorus.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
#[schemars(transform = ranges(&PARAMETERS))]
pub struct ModulationState {
    pub mode: Mode,
    /// How fast the LFO goes up and down.
    pub rate_hz: f32,
    /// How far the LFO moves the delay or the notches. 1 is 7.4 to 19.5 ms for the chorus, 0.27
    /// to 8.5 ms for the flanger, and 250 Hz to 4 kHz for the phaser. 0 holds them still.
    pub depth: f32,
    /// How much of the wet sound goes round again. More is a sharper, ringing sweep that sounds
    /// metallic.
    pub feedback: f32,
    /// How far apart the LFO of the left and of the right side is. 0 moves both together, 0.5 a
    /// quarter cycle apart, 1 half a cycle: opposite, the widest.
    pub spread: f32,
    /// 0 is the sound as it came in, 1 is the wet sound only. The notches of a flanger and a
    /// phaser are deepest at 0.5.
    pub mix: f32,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<ModulationState>;

pub const RATE: Parameter = Parameter {
    field: "rate_hz",
    min: 0.05,
    max: 10.0,
    default: 0.5,
    scale: Scale::Logarithmic,
    get: |state| state.rate_hz,
    set: |state, value| state.rate_hz = value,
};
pub const DEPTH: Parameter = Parameter {
    field: "depth",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |state| state.depth,
    set: |state, value| state.depth = value,
};
pub const FEEDBACK: Parameter = Parameter {
    field: "feedback",
    min: 0.0,
    max: 1.0,
    default: 0.2,
    scale: Scale::Linear,
    get: |state| state.feedback,
    set: |state, value| state.feedback = value,
};
pub const SPREAD: Parameter = Parameter {
    field: "spread",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |state| state.spread,
    set: |state, value| state.spread = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |state| state.mix,
    set: |state, value| state.mix = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 5] = [&RATE, &DEPTH, &FEEDBACK, &SPREAD, &MIX];

impl Default for ModulationState {
    fn default() -> Self {
        Self {
            mode: Mode::Chorus,
            rate_hz: RATE.default,
            depth: DEPTH.default,
            feedback: FEEDBACK.default,
            spread: SPREAD.default,
            mix: MIX.default,
        }
    }
}

impl State for ModulationState {
    const TOOL: &'static str = "modulation";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the modulation record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "modulation",
    when: "A chorus, flanger or phaser on a track: wider, swirling, a jet sweep",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the modulation tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<ModulationState>(EXTENSION)?
        .behaviour(apply)
        .in_code(Modulation::AUTOMATION.parameters());
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and every change glides.
fn apply(
    state: &ModulationState,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let modulation = context.processor("modulation", || Modulation::new(*state))?;
    context.update(modulation, *state)?;
    context.input(
        AUDIO_INPUT,
        InputEndpoint::new(modulation, Modulation::INPUT),
    );
    context.automation(modulation, Modulation::AUTOMATION);
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(modulation, Modulation::OUTPUT),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use sound_core::testing::check_parameter_ranges;

    use super::*;

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        let state = ModulationState::default();
        assert_eq!(
            check_parameter_ranges(&state, &PARAMETERS, |state| state),
            Ok(())
        );
    }
}
