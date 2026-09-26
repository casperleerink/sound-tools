//! Filter: the built-in filter effect. It goes in an effect slot of a track, like an effect
//! plugin, and bends the sound that passes through it: low pass, band pass, high pass or notch,
//! with resonance, a slope of 12 or 24 dB per octave, drive, a dry and wet mix and an LFO on the
//! cutoff.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "filter",
//!   "state": {
//!     "type": "low_pass",
//!     "cutoff_hz": 1000.0,
//!     "resonance": 0.2,
//!     "slope": 12,
//!     "drive_db": 0.0,
//!     "mix": 1.0,
//!     "lfo_rate_hz": 1.0,
//!     "lfo_depth_octaves": 0.0
//!   }
//! }
//! ```
//!
//! `README.md` in this crate has the sound, the ranges and the ports. [`view`] is the card of
//! the filter, and the only module here that uses GPUI.

mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{Filter, MAX_Q, response};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "filter";

/// Which part of the sound the filter lets through.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterType {
    /// What is below the cutoff.
    LowPass,
    /// A band around the cutoff. Resonance makes it narrower.
    BandPass,
    /// What is above the cutoff.
    HighPass,
    /// Everything but a band around the cutoff. Resonance makes the gap narrower.
    Notch,
}

impl FilterType {
    pub const ALL: [Self; 4] = [Self::LowPass, Self::BandPass, Self::HighPass, Self::Notch];
}

/// How steeply the filter cuts past the cutoff, in dB per octave. Saved as the number, `12` or
/// `24`, and nothing else loads.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Slope {
    Twelve,
    TwentyFour,
}

impl Slope {
    pub const ALL: [Self; 2] = [Self::Twelve, Self::TwentyFour];

    pub const fn db_per_octave(self) -> u8 {
        match self {
            Self::Twelve => 12,
            Self::TwentyFour => 24,
        }
    }
}

impl TryFrom<u8> for Slope {
    type Error = String;

    fn try_from(db: u8) -> Result<Self, String> {
        match db {
            12 => Ok(Self::Twelve),
            24 => Ok(Self::TwentyFour),
            _ => Err(format!("slope must be 12 or 24, not {db}")),
        }
    }
}

impl From<Slope> for u8 {
    fn from(slope: Slope) -> Self {
        slope.db_per_octave()
    }
}

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// filter's memory and the phase of its LFO are runtime state and are not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default filter.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FilterState {
    #[serde(rename = "type")]
    pub kind: FilterType,
    /// Where the filter starts to cut, or the middle of its band or its gap.
    pub cutoff_hz: f32,
    /// 0 is no peak. 1 is a strong ringing peak at the cutoff, and a narrow band or gap.
    pub resonance: f32,
    pub slope: Slope,
    /// Gain into a soft saturation before the filter. 0 is clean.
    pub drive_db: f32,
    /// 0 is the sound as it came in, 1 is the filtered sound only.
    pub mix: f32,
    /// How fast the LFO moves the cutoff up and down.
    pub lfo_rate_hz: f32,
    /// How far the LFO moves the cutoff each way. 0 is no LFO.
    pub lfo_depth_octaves: f32,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<FilterState>;

pub const CUTOFF: Parameter = Parameter {
    field: "cutoff_hz",
    min: 20.0,
    max: 20_000.0,
    default: 1_000.0,
    get: |state| state.cutoff_hz,
    set: |state, value| state.cutoff_hz = value,
};
pub const RESONANCE: Parameter = Parameter {
    field: "resonance",
    min: 0.0,
    max: 1.0,
    default: 0.2,
    get: |state| state.resonance,
    set: |state, value| state.resonance = value,
};
pub const DRIVE: Parameter = Parameter {
    field: "drive_db",
    min: 0.0,
    max: 24.0,
    default: 0.0,
    get: |state| state.drive_db,
    set: |state, value| state.drive_db = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    get: |state| state.mix,
    set: |state, value| state.mix = value,
};
pub const LFO_RATE: Parameter = Parameter {
    field: "lfo_rate_hz",
    min: 0.05,
    max: 20.0,
    default: 1.0,
    get: |state| state.lfo_rate_hz,
    set: |state, value| state.lfo_rate_hz = value,
};
pub const LFO_DEPTH: Parameter = Parameter {
    field: "lfo_depth_octaves",
    min: 0.0,
    max: 4.0,
    default: 0.0,
    get: |state| state.lfo_depth_octaves,
    set: |state, value| state.lfo_depth_octaves = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 6] = [&CUTOFF, &RESONANCE, &DRIVE, &MIX, &LFO_RATE, &LFO_DEPTH];

impl Default for FilterState {
    fn default() -> Self {
        Self {
            kind: FilterType::LowPass,
            cutoff_hz: CUTOFF.default,
            resonance: RESONANCE.default,
            slope: Slope::Twelve,
            drive_db: DRIVE.default,
            mix: MIX.default,
            lfo_rate_hz: LFO_RATE.default,
            lfo_depth_octaves: LFO_DEPTH.default,
        }
    }
}

impl State for FilterState {
    const TOOL: &'static str = "filter";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the filter record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "filter",
    when: "You put a filter on a track, or change one: darker, brighter, a sweep, a wah",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the filter tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<FilterState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and every change glides.
fn apply(state: &FilterState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let filter = context.processor("filter", || Filter::new(*state))?;
    context.update(filter, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(filter, Filter::INPUT));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(filter, Filter::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The docs give the ranges and the defaults to agents and to people. They are checked
    /// against the one definition, so they cannot drift from it.
    #[test]
    fn the_docs_give_the_range_and_the_default_of_every_parameter() {
        let docs = [
            ("agent-doc.md", include_str!("../agent-doc.md")),
            ("README.md", include_str!("../README.md")),
        ];
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
                let mut state = FilterState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
            let mut state = FilterState::default();
            (parameter.set)(&mut state, parameter.max * 2.0 + 1.0);
            assert!(
                state
                    .validate()
                    .is_err_and(|error| error.contains(parameter.field))
            );
        }
    }
}
