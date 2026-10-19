//! Delay: the built-in delay effect. It goes in an effect slot of a track, like an effect
//! plugin, and repeats the sound: after a note division that follows the tempo, or after a time
//! in ms. Each repeat is quieter by the feedback and passes the low cut and the high cut once
//! more, so the repeats grow thinner and darker. Ping-pong sends them from left to right and
//! back.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "delay",
//!   "state": {
//!     "sync": true,
//!     "division": "1/8",
//!     "feel": "straight",
//!     "time_ms": 250.0,
//!     "feedback": 0.4,
//!     "ping_pong": false,
//!     "low_cut_hz": 100.0,
//!     "high_cut_hz": 8000.0,
//!     "mix": 0.3
//!   }
//! }
//! ```
//!
//! Known gaps:
//!
//! - A time longer than [`LONGEST_SECONDS`] plays at that time: a whole note under 60 bpm, a
//!   dotted one under 90.
//! - The power icon bypasses the slot with a hard switch, the same for every effect. The
//!   repeats stop at once, and on loud repeats it may click.
//!
//! [`view`] is the card of the delay, and the only module here that uses GPUI.

mod processor;
pub mod view;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State, ranges,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{Delay, response};
/// The note of a synced time. They live in `sound-notes`, which every instrument that follows the
/// tempo shares.
pub use sound_notes::{Division, Feel};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "delay";

/// The longest time the delay plays, in seconds. Its lines are this long.
pub const LONGEST_SECONDS: f32 = 4.0;

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// sound in the delay lines is runtime state and is not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default delay.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
#[schemars(transform = ranges(&PARAMETERS))]
pub struct DelayState {
    /// On, the time is `division` and `feel` at the tempo of the project, and follows it. Off,
    /// the time is `time_ms`.
    pub sync: bool,
    /// The note of a synced time. `"1/4"` is one beat of the tempo.
    pub division: Division,
    /// A synced time is the note, one and a half of it (`"dotted"`), or two thirds of it
    /// (`"triplet"`).
    pub feel: Feel,
    /// The time from the sound to its first repeat, when `sync` is off.
    pub time_ms: f32,
    /// How much of each repeat comes back as the next. 0 is one repeat, 0.5 halves each, 0.95
    /// goes on for a long time and always dies away.
    pub feedback: f32,
    /// The repeats go left, right, left, from both sides of the sound as one. Off, each side
    /// repeats on its own side.
    pub ping_pong: bool,
    /// Each repeat is cut below this once more, so later repeats are thinner.
    pub low_cut_hz: f32,
    /// Each repeat is cut above this once more, so later repeats are darker.
    pub high_cut_hz: f32,
    /// 0 is the sound as it came in, 1 is the repeats only.
    pub mix: f32,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<DelayState>;

pub const TIME: Parameter = Parameter {
    field: "time_ms",
    min: 1.0,
    max: 4_000.0,
    default: 250.0,
    scale: Scale::Logarithmic,
    get: |state| state.time_ms,
    set: |state, value| state.time_ms = value,
};
/// Under 1, so every repeat is quieter than the one before, also with no cut at all.
pub const FEEDBACK: Parameter = Parameter {
    field: "feedback",
    min: 0.0,
    max: 0.95,
    default: 0.4,
    scale: Scale::Linear,
    get: |state| state.feedback,
    set: |state, value| state.feedback = value,
};
pub const LOW_CUT: Parameter = Parameter {
    field: "low_cut_hz",
    min: 20.0,
    max: 20_000.0,
    default: 100.0,
    scale: Scale::Logarithmic,
    get: |state| state.low_cut_hz,
    set: |state, value| state.low_cut_hz = value,
};
pub const HIGH_CUT: Parameter = Parameter {
    field: "high_cut_hz",
    min: 20.0,
    max: 20_000.0,
    default: 8_000.0,
    scale: Scale::Logarithmic,
    get: |state| state.high_cut_hz,
    set: |state, value| state.high_cut_hz = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 0.3,
    scale: Scale::Linear,
    get: |state| state.mix,
    set: |state, value| state.mix = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 5] = [&TIME, &FEEDBACK, &LOW_CUT, &HIGH_CUT, &MIX];

impl Default for DelayState {
    fn default() -> Self {
        Self {
            sync: true,
            division: Division::Eighth,
            feel: Feel::Straight,
            time_ms: TIME.default,
            feedback: FEEDBACK.default,
            ping_pong: false,
            low_cut_hz: LOW_CUT.default,
            high_cut_hz: HIGH_CUT.default,
            mix: MIX.default,
        }
    }
}

impl State for DelayState {
    const TOOL: &'static str = "delay";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The time from the sound to its first repeat, in seconds, at a tempo in quarter notes per
/// minute, and at most [`LONGEST_SECONDS`]. The one place a time is worked out: the processor,
/// the card and the tests all use it.
pub fn delay_seconds(state: &DelayState, bpm: f64) -> f32 {
    let seconds = if state.sync {
        let quarters = state.division.quarters_with(state.feel);
        (f64::from(quarters) * 60.0 / bpm) as f32
    } else {
        state.time_ms / 1_000.0
    };
    seconds.min(LONGEST_SECONDS)
}

/// The doc of the delay record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "delay",
    when: "A delay on a track: echoes, a slapback, ping-pong",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the delay tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .tool::<DelayState>(EXTENSION)?
        .behaviour(apply)
        .schema(Delay::AUTOMATION.parameters());
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// repeats go on through an edit and every change glides.
fn apply(state: &DelayState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let delay = context.processor("delay", || Delay::new(*state))?;
    context.update(delay, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(delay, Delay::INPUT));
    context.automation(delay, Delay::AUTOMATION);
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(delay, Delay::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use sound_core::testing::check_parameter_ranges;

    use super::*;

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        let state = DelayState::default();
        assert_eq!(
            check_parameter_ranges(&state, &PARAMETERS, |state| state),
            Ok(())
        );
    }

    /// A synced time is the note at the tempo: an eighth at 120 bpm is a quarter of a second,
    /// dotted three eighths of one, a triplet a sixth.
    #[test]
    fn a_synced_time_is_its_note_at_the_tempo() {
        let at = |division, feel, bpm| {
            let state = DelayState {
                division,
                feel,
                ..DelayState::default()
            };
            delay_seconds(&state, bpm)
        };
        assert_eq!(at(Division::Eighth, Feel::Straight, 120.0), 0.25);
        assert_eq!(at(Division::Eighth, Feel::Dotted, 120.0), 0.375);
        assert!((at(Division::Eighth, Feel::Triplet, 120.0) - 1.0 / 6.0).abs() < 1e-6);
        assert_eq!(at(Division::Quarter, Feel::Straight, 60.0), 1.0);
        assert_eq!(at(Division::ThirtySecond, Feel::Straight, 120.0), 0.0625);
        // Too long for the lines: it plays at the longest.
        assert_eq!(at(Division::Whole, Feel::Dotted, 60.0), LONGEST_SECONDS);
        let free = DelayState {
            sync: false,
            time_ms: 330.0,
            ..DelayState::default()
        };
        assert_eq!(delay_seconds(&free, 120.0), 0.33);
    }
}
