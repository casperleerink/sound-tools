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
//!   "state": {
//!     "mode": "chorus",
//!     "rate_hz": 0.5,
//!     "depth": 0.5,
//!     "feedback": 0.2,
//!     "spread": 0.5,
//!     "mix": 0.5
//!   }
//! }
//! ```
//!
//! [`view`] is the card of the modulation, and the only module here that uses GPUI.

mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{MAX_FEEDBACK, Modulation, response, sweep};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "modulation";

/// What the LFO moves.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModulationState {
    pub mode: Mode,
    /// How fast the LFO goes up and down.
    pub rate_hz: f32,
    /// How far the LFO moves the delay or the notches. 0 holds them still.
    pub depth: f32,
    /// How much of the wet sound goes round again. More is a sharper, ringing sweep.
    pub feedback: f32,
    /// How far apart the LFO of the left and of the right side is. 0 moves both together, 1
    /// moves them half a cycle apart.
    pub spread: f32,
    /// 0 is the sound as it came in, 1 is the wet sound only.
    pub mix: f32,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<ModulationState>;

pub const RATE: Parameter = Parameter {
    field: "rate_hz",
    min: 0.05,
    max: 10.0,
    default: 0.5,
    get: |state| state.rate_hz,
    set: |state, value| state.rate_hz = value,
};
pub const DEPTH: Parameter = Parameter {
    field: "depth",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    get: |state| state.depth,
    set: |state, value| state.depth = value,
};
pub const FEEDBACK: Parameter = Parameter {
    field: "feedback",
    min: 0.0,
    max: 1.0,
    default: 0.2,
    get: |state| state.feedback,
    set: |state, value| state.feedback = value,
};
pub const SPREAD: Parameter = Parameter {
    field: "spread",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    get: |state| state.spread,
    set: |state, value| state.spread = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 0.5,
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
    when: "You put a chorus, a flanger or a phaser on a track, or change one: wider, swirling, a jet sweep",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the modulation tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<ModulationState>(EXTENSION)?
        .behaviour(apply);
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
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(modulation, Modulation::OUTPUT),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent doc gives the ranges and the defaults to agents. They are checked against the one
    /// definition, so they cannot drift from it.
    #[test]
    fn the_docs_give_the_range_and_the_default_of_every_parameter() {
        let docs = [("agent-doc.md", include_str!("../agent-doc.md"))];
        for (name, doc) in docs {
            for Parameter {
                field,
                min,
                max,
                default,
                ..
            } in PARAMETERS
            {
                let row = format!("| `{field}` |");
                let row = doc.lines().find(|line| line.starts_with(&row));
                let row = row.unwrap_or_else(|| panic!("{name} has no row for {field}"));
                assert!(
                    row.contains(&format!("| {min} to {max} |")),
                    "{name}: {row}"
                );
                assert!(row.contains(&format!("| {default} |")), "{name}: {row}");
            }
        }
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        for parameter in PARAMETERS {
            for value in [parameter.min, parameter.default, parameter.max] {
                let mut state = ModulationState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
            let mut state = ModulationState::default();
            (parameter.set)(&mut state, parameter.max * 2.0 + 1.0);
            assert!(
                state
                    .validate()
                    .is_err_and(|error| error.contains(parameter.field))
            );
        }
    }
}
