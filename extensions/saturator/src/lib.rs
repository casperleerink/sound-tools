//! Saturator: the built-in saturation effect. It goes in an effect slot of a track, like an
//! effect plugin, and drives the sound into a curve that rounds its peaks off: warmer and denser
//! at a little drive, distorted at a lot. Four curves, drive, a tone tilt after the curve, the
//! output level and a dry and wet mix.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "saturator",
//!   "state": {"curve": "soft", "drive_db": 6.0, "tone_db": 0.0, "output_db": 0.0, "mix": 1.0}
//! }
//! ```
//!
//! The level stays where it was as the drive goes up: the saturator turns its output down by
//! what the drive adds to a sine at -12 dBFS. So the drive changes the colour of the sound and
//! not how loud it is.
//!
//! [`view`] is the card of the saturator, and the only module here that uses GPUI.

mod oversampling;
mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{LATENCY, Saturator, auto_gain, response, transfer};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "saturator";

/// The shape the sound is driven into. Every curve has a slope of 1 at silence, so a quiet
/// sound goes through each at the same level.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    /// `tanh`: rounds off early and evenly. Odd harmonics.
    Soft,
    /// Rounds off earlier and more gently than soft, and never flattens: the softest.
    Tape,
    /// Leans to one side, so it adds even harmonics too, as a tube stage does.
    Tube,
    /// Clean up to a ceiling, then a short round corner: a clipper.
    Clip,
}

impl Curve {
    pub const ALL: [Self; 4] = [Self::Soft, Self::Tape, Self::Tube, Self::Clip];
}

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// memory of the oversampling and of the tone are runtime state and are not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default
/// saturator.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SaturatorState {
    pub curve: Curve,
    /// Gain into the curve. The output is turned down by what it adds to a sine at -12 dBFS.
    pub drive_db: f32,
    /// A tilt after the curve: above 0 the highs go up and the lows down by half of it each,
    /// around 1 kHz. Below 0 the other way.
    pub tone_db: f32,
    /// Gain on the saturated sound.
    pub output_db: f32,
    /// 0 is the sound as it came in, 1 is the saturated sound only.
    pub mix: f32,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<SaturatorState>;

pub const DRIVE: Parameter = Parameter {
    field: "drive_db",
    min: 0.0,
    max: 36.0,
    default: 6.0,
    get: |state| state.drive_db,
    set: |state, value| state.drive_db = value,
};
pub const TONE: Parameter = Parameter {
    field: "tone_db",
    min: -12.0,
    max: 12.0,
    default: 0.0,
    get: |state| state.tone_db,
    set: |state, value| state.tone_db = value,
};
pub const OUTPUT: Parameter = Parameter {
    field: "output_db",
    min: -12.0,
    max: 12.0,
    default: 0.0,
    get: |state| state.output_db,
    set: |state, value| state.output_db = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    get: |state| state.mix,
    set: |state, value| state.mix = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 4] = [&DRIVE, &TONE, &OUTPUT, &MIX];

impl Default for SaturatorState {
    fn default() -> Self {
        Self {
            curve: Curve::Soft,
            drive_db: DRIVE.default,
            tone_db: TONE.default,
            output_db: OUTPUT.default,
            mix: MIX.default,
        }
    }
}

impl State for SaturatorState {
    const TOOL: &'static str = "saturator";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the saturator record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "saturator",
    when: "A saturator on a track: warmer, denser, grit or distortion",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the saturator tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<SaturatorState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and every change glides.
fn apply(state: &SaturatorState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let saturator = context.processor("saturator", || Saturator::new(*state))?;
    context.update(saturator, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(saturator, Saturator::INPUT));
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(saturator, Saturator::OUTPUT),
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
                let mut state = SaturatorState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
            let mut state = SaturatorState::default();
            (parameter.set)(&mut state, parameter.max.abs() * 2.0 + 1.0);
            assert!(
                state
                    .validate()
                    .is_err_and(|error| error.contains(parameter.field))
            );
        }
    }

    #[test]
    fn a_curve_is_saved_as_its_name_and_nothing_else_loads() {
        let state: SaturatorState = serde_json::from_str(r#"{"curve": "tube"}"#).unwrap();
        assert_eq!(state.curve, Curve::Tube);
        assert!(serde_json::from_str::<SaturatorState>(r#"{"curve": "fuzz"}"#).is_err());
    }
}
