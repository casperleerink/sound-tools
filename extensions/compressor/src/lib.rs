//! Compressor: the built-in compressor effect. It goes in an effect slot of a track, like an
//! effect plugin, and turns the sound down when it gets louder than the threshold: by the ratio,
//! over a soft knee, as fast as the attack and back as slowly as the release. Makeup gain, a dry
//! and wet mix and a lookahead complete it.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "compressor",
//!   "state": {
//!     "threshold_db": -18.0,
//!     "ratio": 4.0,
//!     "attack_ms": 10.0,
//!     "release_ms": 120.0,
//!     "knee_db": 6.0,
//!     "makeup_db": 0.0,
//!     "mix": 1.0,
//!     "lookahead_ms": 0
//!   }
//! }
//! ```
//!
//! `README.md` in this crate has the sound, the ranges and the ports. [`view`] is the card of
//! the compressor, and the only module here that uses GPUI.

mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{Compressor, HOLD_SECONDS, reduction_db, static_gain_db};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "compressor";

/// How far ahead the compressor listens, in milliseconds. It delays the sound by as much, and
/// reports that as its latency, so the track stays in time. Saved as the number, `0`, `1` or
/// `10`, and nothing else loads: a lookahead is a latency, and a latency that moves with a knob
/// would make every track before it jump on each step.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Lookahead {
    Off,
    One,
    Ten,
}

impl Lookahead {
    pub const ALL: [Self; 3] = [Self::Off, Self::One, Self::Ten];

    pub const fn milliseconds(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::One => 1,
            Self::Ten => 10,
        }
    }

    /// The delay in frames at a sample rate, which is also the latency.
    pub fn frames(self, sample_rate: f32) -> usize {
        (f32::from(self.milliseconds()) * sample_rate / 1_000.0).round() as usize
    }
}

impl TryFrom<u8> for Lookahead {
    type Error = String;

    fn try_from(milliseconds: u8) -> Result<Self, String> {
        match milliseconds {
            0 => Ok(Self::Off),
            1 => Ok(Self::One),
            10 => Ok(Self::Ten),
            _ => Err(format!(
                "lookahead_ms must be 0, 1 or 10, not {milliseconds}"
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
/// much the compressor turns down right now, and what it heard, are runtime state and are not
/// saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default
/// compressor.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CompressorState {
    /// The peak level in dBFS above which the compressor turns the sound down.
    pub threshold_db: f32,
    /// How much it turns down: 4 means 4 dB over the threshold comes out as 1 dB over it.
    pub ratio: f32,
    /// How fast it turns down when the sound gets louder: the time to 63 % of the way.
    pub attack_ms: f32,
    /// How fast it lets go when the sound gets quieter: the time to 63 % of the way, after the
    /// loud part has left the last 10 ms.
    pub release_ms: f32,
    /// The width of the bend around the threshold, in dB. 0 is a hard corner.
    pub knee_db: f32,
    /// Gain after the compression, to bring the level back up.
    pub makeup_db: f32,
    /// 0 is the sound as it came in, 1 is the compressed sound only.
    pub mix: f32,
    #[serde(rename = "lookahead_ms")]
    pub lookahead: Lookahead,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<CompressorState>;

pub const THRESHOLD: Parameter = Parameter {
    field: "threshold_db",
    min: -60.0,
    max: 0.0,
    default: -18.0,
    get: |state| state.threshold_db,
    set: |state, value| state.threshold_db = value,
};
pub const RATIO: Parameter = Parameter {
    field: "ratio",
    min: 1.0,
    max: 100.0,
    default: 4.0,
    get: |state| state.ratio,
    set: |state, value| state.ratio = value,
};
pub const ATTACK: Parameter = Parameter {
    field: "attack_ms",
    min: 0.1,
    max: 300.0,
    default: 10.0,
    get: |state| state.attack_ms,
    set: |state, value| state.attack_ms = value,
};
pub const RELEASE: Parameter = Parameter {
    field: "release_ms",
    min: 1.0,
    max: 3_000.0,
    default: 120.0,
    get: |state| state.release_ms,
    set: |state, value| state.release_ms = value,
};
pub const KNEE: Parameter = Parameter {
    field: "knee_db",
    min: 0.0,
    max: 18.0,
    default: 6.0,
    get: |state| state.knee_db,
    set: |state, value| state.knee_db = value,
};
pub const MAKEUP: Parameter = Parameter {
    field: "makeup_db",
    min: 0.0,
    max: 24.0,
    default: 0.0,
    get: |state| state.makeup_db,
    set: |state, value| state.makeup_db = value,
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
pub const PARAMETERS: [&Parameter; 7] =
    [&THRESHOLD, &RATIO, &ATTACK, &RELEASE, &KNEE, &MAKEUP, &MIX];

impl Default for CompressorState {
    fn default() -> Self {
        Self {
            threshold_db: THRESHOLD.default,
            ratio: RATIO.default,
            attack_ms: ATTACK.default,
            release_ms: RELEASE.default,
            knee_db: KNEE.default,
            makeup_db: MAKEUP.default,
            mix: MIX.default,
            lookahead: Lookahead::Off,
        }
    }
}

impl State for CompressorState {
    const TOOL: &'static str = "compressor";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the compressor record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "compressor",
    when: "You put a compressor on a track, or change one: evener, punchier, tamer peaks, more sustain",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the compressor tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<CompressorState>(EXTENSION)?
        .behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and every change glides.
fn apply(
    state: &CompressorState,
    context: &mut BehaviourContext<'_>,
) -> Result<(), BehaviourError> {
    let compressor = context.processor("compressor", || Compressor::new(*state))?;
    context.update(compressor, *state)?;
    context.input(
        AUDIO_INPUT,
        InputEndpoint::new(compressor, Compressor::INPUT),
    );
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(compressor, Compressor::OUTPUT),
    );
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
                let mut state = CompressorState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
            let mut state = CompressorState::default();
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
        assert!(Lookahead::try_from(5).is_err());
        assert_eq!(Lookahead::Ten.frames(48_000.0), 480);
        assert_eq!(Lookahead::One.frames(44_100.0), 44);
    }
}
