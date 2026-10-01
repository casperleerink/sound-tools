//! Limiter: the built-in limiter effect. It goes in an effect slot of a track, like an effect
//! plugin, and keeps the sound under its ceiling: a peak that would go over is turned down at
//! once, just enough, and the gain comes back with the release. Gain pushes the sound into it,
//! for a louder track with the same peaks. A lookahead lets it turn down before a peak arrives,
//! so the top of the wave keeps its shape.
//!
//! It is the limiter at the end of the master as a device: both are [`sound_core::PeakLimiter`],
//! so they sound the same for the same settings. Under the ceiling it leaves every sample as it
//! was, one lookahead later.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "limiter",
//!   "state": {
//!     "gain_db": 0.0,
//!     "ceiling_db": -1.0,
//!     "release_ms": 100.0,
//!     "lookahead_ms": 1
//!   }
//! }
//! ```
//!
//! Known gaps:
//!
//! - It holds the samples under the ceiling, not the wave between them, which can go over it
//!   once the sound is played or made into a lossy file: most for high tones, up to 3 dB at a
//!   quarter of the sample rate. The default ceiling of -1 dB leaves some room for it.
//! - A change of the lookahead starts the delay again from silence, as a change of latency
//!   moves the track in time anyway.
//!
//! [`view`] is the card of the limiter, and the only module here that uses GPUI.

mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{Limiter, Meters};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "limiter";

/// How far ahead the limiter looks, in milliseconds. It delays the sound by as much, and reports
/// that as its latency, so the track stays in time. Saved as the number, `0`, `1` or `5`, and
/// nothing else loads: a lookahead is a latency, and a latency that moves with a knob would make
/// every track before it jump on each step.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Lookahead {
    Off,
    One,
    Five,
}

impl Lookahead {
    pub const ALL: [Self; 3] = [Self::Off, Self::One, Self::Five];

    pub const fn milliseconds(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::One => 1,
            Self::Five => 5,
        }
    }

    pub fn seconds(self) -> f32 {
        f32::from(self.milliseconds()) / 1_000.0
    }

    /// The delay in frames at a sample rate, which is also the latency.
    pub fn frames(self, sample_rate: f32) -> usize {
        (self.seconds() * sample_rate).round() as usize
    }
}

impl TryFrom<u8> for Lookahead {
    type Error = String;

    fn try_from(milliseconds: u8) -> Result<Self, String> {
        match milliseconds {
            0 => Ok(Self::Off),
            1 => Ok(Self::One),
            5 => Ok(Self::Five),
            _ => Err(format!(
                "lookahead_ms must be 0, 1 or 5, not {milliseconds}"
            )),
        }
    }
}

impl From<Lookahead> for u8 {
    fn from(lookahead: Lookahead) -> Self {
        lookahead.milliseconds()
    }
}

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. How
/// much the limiter turns down right now is runtime state and is not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default limiter.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LimiterState {
    /// How much louder the sound goes into the limiter.
    pub gain_db: f32,
    /// The highest a sample may reach, in dBFS.
    pub ceiling_db: f32,
    /// How long the gain takes to come back after a peak: the time to 63 % of the way.
    pub release_ms: f32,
    #[serde(rename = "lookahead_ms")]
    pub lookahead: Lookahead,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<LimiterState>;

pub const GAIN: Parameter = Parameter {
    field: "gain_db",
    min: 0.0,
    max: 24.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |state| state.gain_db,
    set: |state, value| state.gain_db = value,
};
pub const CEILING: Parameter = Parameter {
    field: "ceiling_db",
    min: -24.0,
    max: 0.0,
    // Under full scale, for the peaks between samples, which the limiter does not see.
    default: -1.0,
    scale: Scale::Linear,
    get: |state| state.ceiling_db,
    set: |state, value| state.ceiling_db = value,
};
pub const RELEASE: Parameter = Parameter {
    field: "release_ms",
    min: 10.0,
    max: 1_000.0,
    default: 100.0,
    scale: Scale::Logarithmic,
    get: |state| state.release_ms,
    set: |state, value| state.release_ms = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 3] = [&GAIN, &CEILING, &RELEASE];

impl Default for LimiterState {
    fn default() -> Self {
        Self {
            gain_db: GAIN.default,
            ceiling_db: CEILING.default,
            release_ms: RELEASE.default,
            // A millisecond keeps the top of a peak round, and a keyboard played into the track
            // waits only that long.
            lookahead: Lookahead::One,
        }
    }
}

impl State for LimiterState {
    const TOOL: &'static str = "limiter";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the limiter record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "limiter",
    when: "A limiter on a track: louder, with no peak over a ceiling",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the limiter tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<LimiterState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and the gain glides.
fn apply(state: &LimiterState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let meters = Meters {
        output: context.peaks(Meters::OUTPUT),
        reduction: context.peaks(Meters::REDUCTION),
    };
    let limiter = context.processor("limiter", || Limiter::new(*state, meters))?;
    context.update(limiter, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(limiter, Limiter::INPUT));
    context.automation(limiter, Limiter::AUTOMATION);
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(limiter, Limiter::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The agent doc gives the ranges and the defaults to agents. They are checked against the
    /// one definition, so they cannot drift from it.
    #[test]
    fn the_doc_gives_the_range_and_the_default_of_every_parameter() {
        let doc = include_str!("../agent-doc.md");
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
            let row = row.unwrap_or_else(|| panic!("the doc has no row for {field}"));
            assert!(row.contains(&format!("| {min} to {max} |")), "{row}");
            assert!(row.contains(&format!("| {default} |")), "{row}");
        }
    }

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        for parameter in PARAMETERS {
            for value in [parameter.min, parameter.default, parameter.max] {
                let mut state = LimiterState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
            let mut state = LimiterState::default();
            (parameter.set)(&mut state, parameter.max.abs() * 2.0 + 1.0);
            assert!(
                state
                    .validate()
                    .is_err_and(|error| error.contains(parameter.field))
            );
        }
    }

    #[test]
    fn the_lookahead_is_saved_as_its_milliseconds_and_nothing_else_loads() {
        for lookahead in Lookahead::ALL {
            let number = u8::from(lookahead);
            assert_eq!(Lookahead::try_from(number), Ok(lookahead));
        }
        assert!(Lookahead::try_from(10).is_err());
        assert_eq!(Lookahead::Five.frames(48_000.0), 240);
        assert_eq!(Lookahead::One.frames(44_100.0), 44);
    }
}
