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
//! Known gaps:
//!
//! - A freeze over a short decay in a large room rises by up to 1.5 dB, although the frozen
//!   loop keeps its energy exactly. The cause is not proven.
//! - With damping, the 20 ms glide into freeze can write up to about 1 dB more into the lines
//!   than they held, as the damping filter lets go.
//! - The power icon bypasses the slot with a hard switch, the same for every effect. The tail
//!   stops at once, and on a loud tail it may click.
//!
//! [`view`] is the card of the reverb, and the only module here that uses GPUI.

mod processor;
pub mod view;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State, ranges,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::{DAMPED_HZ, Reverb, high_decay_seconds, reflections};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "reverb";

/// The saved state. It is small and `Copy`, so it is also the update the processor gets. The
/// sound in the delay lines is runtime state and is not saved.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default reverb.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
#[schemars(transform = ranges(&PARAMETERS))]
pub struct ReverbState {
    /// The time from the sound to the first of its reflections. Longer keeps a voice or a drum
    /// clear of its room.
    pub pre_delay_ms: f32,
    /// The time the tail takes to fall by 60 dB.
    pub decay_seconds: f32,
    /// The size of the room: how far apart the reflections are. 0 is a small box and sounds
    /// metallic, 1 a hall.
    pub size: f32,
    /// How much sooner the highs die than the rest. 0 is as long, 0.5 is 55 % of the decay
    /// time, 1 is a tenth of it.
    pub damping: f32,
    /// How quickly the reflections blur into a smooth tail. 0 is separate echoes, 1 is smooth
    /// at once.
    pub diffusion: f32,
    /// What goes into the reverb is cut below this. Raise it to keep the bass out of the room.
    pub low_cut_hz: f32,
    /// What goes into the reverb is cut above this. Lower it for a darker room.
    pub high_cut_hz: f32,
    /// 0 is a mono tail, 1 is as wide as it gets. The dry sound keeps its own width.
    pub width: f32,
    /// 0 is the sound as it came in, 1 is the reverb only.
    pub mix: f32,
    /// Holds the tail as it is, for as long as it is on, lets no new sound into it, and passes
    /// the dry sound as before.
    pub freeze: bool,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<ReverbState>;

pub const PRE_DELAY: Parameter = Parameter {
    field: "pre_delay_ms",
    min: 0.5,
    max: 250.0,
    default: 20.0,
    scale: Scale::Logarithmic,
    get: |state| state.pre_delay_ms,
    set: |state, value| state.pre_delay_ms = value,
};
pub const DECAY: Parameter = Parameter {
    field: "decay_seconds",
    min: 0.2,
    max: 60.0,
    default: 2.0,
    scale: Scale::Logarithmic,
    get: |state| state.decay_seconds,
    set: |state, value| state.decay_seconds = value,
};
pub const SIZE: Parameter = Parameter {
    field: "size",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |state| state.size,
    set: |state, value| state.size = value,
};
pub const DAMPING: Parameter = Parameter {
    field: "damping",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    scale: Scale::Linear,
    get: |state| state.damping,
    set: |state, value| state.damping = value,
};
pub const DIFFUSION: Parameter = Parameter {
    field: "diffusion",
    min: 0.0,
    max: 1.0,
    default: 0.7,
    scale: Scale::Linear,
    get: |state| state.diffusion,
    set: |state, value| state.diffusion = value,
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
pub const WIDTH: Parameter = Parameter {
    field: "width",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    scale: Scale::Linear,
    get: |state| state.width,
    set: |state, value| state.width = value,
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
    registry
        .tool::<ReverbState>(EXTENSION)?
        .behaviour(apply)
        .schema(Reverb::AUTOMATION.parameters());
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// tail goes on through an edit and every change glides.
fn apply(state: &ReverbState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let reverb = context.processor("reverb", || Reverb::new(*state))?;
    context.update(reverb, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(reverb, Reverb::INPUT));
    context.automation(reverb, Reverb::AUTOMATION);
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(reverb, Reverb::OUTPUT));
    Ok(())
}

#[cfg(test)]
mod tests {
    use sound_core::testing::check_parameter_ranges;

    use super::*;

    #[test]
    fn the_default_of_every_parameter_is_valid_and_the_ends_of_its_range_are_too() {
        let state = ReverbState::default();
        assert_eq!(
            check_parameter_ranges(&state, &PARAMETERS, |state| state),
            Ok(())
        );
    }
}
