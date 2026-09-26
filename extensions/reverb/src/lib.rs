//! Reverb: the built-in reverb effect. It goes in an effect slot of a track, like an effect
//! plugin, and puts the sound in a room: a pre-delay, early reflections and a tail that dies
//! away in the decay time, with the highs dying sooner as the damping rises. Freeze holds the
//! tail.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "reverb",
//!   "state": {
//!     "pre_delay_ms": 20.0,
//!     "decay_seconds": 2.0,
//!     "size": 0.5,
//!     "damping": 0.5,
//!     "diffusion": 0.7,
//!     "low_cut_hz": 100.0,
//!     "high_cut_hz": 8000.0,
//!     "width": 1.0,
//!     "mix": 0.3,
//!     "freeze": false
//!   }
//! }
//! ```
//!
//! `README.md` in this crate has the sound, the ranges and the ports. [`view`] is the card of
//! the reverb, and the only module here that uses GPUI.

mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{DAMPED_HZ, Reverb, high_decay_seconds, line_seconds};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "reverb";

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// sound in the delay lines is runtime state and is not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default reverb.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ReverbState {
    /// The time from the sound to the first of its reflections.
    pub pre_delay_ms: f32,
    /// The time the tail takes to fall by 60 dB.
    pub decay_seconds: f32,
    /// The size of the room: how far apart the reflections are. 0 is a small box, 1 a hall.
    pub size: f32,
    /// How much sooner the highs die than the rest. 0 is as long, 1 is a tenth of the time.
    pub damping: f32,
    /// How quickly the reflections blur into a smooth tail. 0 is separate echoes.
    pub diffusion: f32,
    /// What goes into the reverb is cut below this.
    pub low_cut_hz: f32,
    /// What goes into the reverb is cut above this.
    pub high_cut_hz: f32,
    /// 0 is a mono tail, 1 is as wide as it gets.
    pub width: f32,
    /// 0 is the sound as it came in, 1 is the reverb only.
    pub mix: f32,
    /// Holds the tail as it is, for as long as it is on, and lets no new sound in.
    pub freeze: bool,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<ReverbState>;

pub const PRE_DELAY: Parameter = Parameter {
    field: "pre_delay_ms",
    min: 0.5,
    max: 250.0,
    default: 20.0,
    get: |state| state.pre_delay_ms,
    set: |state, value| state.pre_delay_ms = value,
};
pub const DECAY: Parameter = Parameter {
    field: "decay_seconds",
    min: 0.2,
    max: 60.0,
    default: 2.0,
    get: |state| state.decay_seconds,
    set: |state, value| state.decay_seconds = value,
};
pub const SIZE: Parameter = Parameter {
    field: "size",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    get: |state| state.size,
    set: |state, value| state.size = value,
};
pub const DAMPING: Parameter = Parameter {
    field: "damping",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    get: |state| state.damping,
    set: |state, value| state.damping = value,
};
pub const DIFFUSION: Parameter = Parameter {
    field: "diffusion",
    min: 0.0,
    max: 1.0,
    default: 0.7,
    get: |state| state.diffusion,
    set: |state, value| state.diffusion = value,
};
pub const LOW_CUT: Parameter = Parameter {
    field: "low_cut_hz",
    min: 20.0,
    max: 20_000.0,
    default: 100.0,
    get: |state| state.low_cut_hz,
    set: |state, value| state.low_cut_hz = value,
};
pub const HIGH_CUT: Parameter = Parameter {
    field: "high_cut_hz",
    min: 20.0,
    max: 20_000.0,
    default: 8_000.0,
    get: |state| state.high_cut_hz,
    set: |state, value| state.high_cut_hz = value,
};
pub const WIDTH: Parameter = Parameter {
    field: "width",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    get: |state| state.width,
    set: |state, value| state.width = value,
};
pub const MIX: Parameter = Parameter {
    field: "mix",
    min: 0.0,
    max: 1.0,
    default: 0.3,
    get: |state| state.mix,
    set: |state, value| state.mix = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 9] = [
    &PRE_DELAY, &DECAY, &SIZE, &DAMPING, &DIFFUSION, &LOW_CUT, &HIGH_CUT, &WIDTH, &MIX,
];

impl Default for ReverbState {
    fn default() -> Self {
        Self {
            pre_delay_ms: PRE_DELAY.default,
            decay_seconds: DECAY.default,
            size: SIZE.default,
            damping: DAMPING.default,
            diffusion: DIFFUSION.default,
            low_cut_hz: LOW_CUT.default,
            high_cut_hz: HIGH_CUT.default,
            width: WIDTH.default,
            mix: MIX.default,
            freeze: false,
        }
    }
}

impl State for ReverbState {
    const TOOL: &'static str = "reverb";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the reverb record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "reverb",
    when: "You put a reverb on a track, or change one: a room, a hall, more space, a frozen pad",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the reverb tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<ReverbState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// tail goes on through an edit and every change glides.
fn apply(state: &ReverbState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let reverb = context.processor("reverb", || Reverb::new(*state))?;
    context.update(reverb, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(reverb, Reverb::INPUT));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(reverb, Reverb::OUTPUT));
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
                let mut state = ReverbState::default();
                (parameter.set)(&mut state, value);
                assert_eq!((parameter.get)(&state), value);
                assert_eq!(state.validate(), Ok(()));
            }
            let mut state = ReverbState::default();
            (parameter.set)(&mut state, parameter.max * 2.0 + 1.0);
            assert!(
                state
                    .validate()
                    .is_err_and(|error| error.contains(parameter.field))
            );
        }
    }
}
