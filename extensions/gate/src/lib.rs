//! Gate: the built-in gate and transient shaper. It goes in an effect slot of a track and turns
//! the sound down by the range while it is quieter than the threshold: it opens as fast as the
//! attack, stays open for the hold and closes as slowly as the release. Another track can key
//! it through the `sidechain` input. Transient and sustain make the start of each hit louder or
//! quieter, and its tail.
//!
//! One device and not two: the shaper is two numbers on the level the gate hears already, so
//! a device of its own would be a second crate, record, card and doc around them.
//!
//! [`view`] is the card of the gate, and the only module here that uses GPUI.

mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, Scale, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT, SIDECHAIN_INPUT};

pub use processor::{Gate, Meters, static_gain_db};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "gate";

/// The saved state. It is small and `Copy`, so it is also the update the processor gets.
///
/// A field that a record leaves out takes its default, so `"state": {}` is the default gate.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GateState {
    /// The peak level in dBFS under which the gate closes.
    pub threshold_db: f32,
    /// How fast it opens: the time to 63 % of the way.
    pub attack_ms: f32,
    /// How long it stays open after the level fell under the threshold.
    pub hold_ms: f32,
    /// How fast it closes after the hold: the time to 63 % of the way.
    pub release_ms: f32,
    /// How far it turns the sound down when closed. 0 does not gate.
    pub range_db: f32,
    /// Gain on the start of each hit. Below 0 softens it.
    pub transient_db: f32,
    /// Gain on the tail of each hit. Below 0 makes it shorter.
    pub sustain_db: f32,
}

/// One number of the saved state, with its range and its default.
pub type Parameter = sound_core::Parameter<GateState>;

pub const THRESHOLD: Parameter = Parameter {
    field: "threshold_db",
    min: -80.0,
    max: 0.0,
    default: -40.0,
    scale: Scale::Linear,
    get: |state| state.threshold_db,
    set: |state, value| state.threshold_db = value,
};
pub const ATTACK: Parameter = Parameter {
    field: "attack_ms",
    min: 0.1,
    max: 100.0,
    default: 0.5,
    scale: Scale::Logarithmic,
    get: |state| state.attack_ms,
    set: |state, value| state.attack_ms = value,
};
pub const HOLD: Parameter = Parameter {
    field: "hold_ms",
    min: 0.0,
    max: 500.0,
    default: 20.0,
    scale: Scale::Linear,
    get: |state| state.hold_ms,
    set: |state, value| state.hold_ms = value,
};
pub const RELEASE: Parameter = Parameter {
    field: "release_ms",
    min: 1.0,
    max: 3_000.0,
    default: 100.0,
    scale: Scale::Logarithmic,
    get: |state| state.release_ms,
    set: |state, value| state.release_ms = value,
};
pub const RANGE: Parameter = Parameter {
    field: "range_db",
    min: 0.0,
    max: 80.0,
    default: 80.0,
    scale: Scale::Linear,
    get: |state| state.range_db,
    set: |state, value| state.range_db = value,
};
pub const TRANSIENT: Parameter = Parameter {
    field: "transient_db",
    min: -18.0,
    max: 18.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |state| state.transient_db,
    set: |state, value| state.transient_db = value,
};
pub const SUSTAIN: Parameter = Parameter {
    field: "sustain_db",
    min: -18.0,
    max: 18.0,
    default: 0.0,
    scale: Scale::Linear,
    get: |state| state.sustain_db,
    set: |state, value| state.sustain_db = value,
};

/// Every number of the state, in the order of its fields.
pub const PARAMETERS: [&Parameter; 7] = [
    &THRESHOLD, &ATTACK, &HOLD, &RELEASE, &RANGE, &TRANSIENT, &SUSTAIN,
];

impl Default for GateState {
    fn default() -> Self {
        Self {
            threshold_db: THRESHOLD.default,
            attack_ms: ATTACK.default,
            hold_ms: HOLD.default,
            release_ms: RELEASE.default,
            range_db: RANGE.default,
            transient_db: TRANSIENT.default,
            sustain_db: SUSTAIN.default,
        }
    }
}

impl State for GateState {
    const TOOL: &'static str = "gate";

    fn validate(&self) -> Result<(), String> {
        PARAMETERS
            .iter()
            .try_for_each(|parameter| parameter.check(self))
    }
}

/// The doc of the gate record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "gate",
    when: "A gate or transient shaper on a track: tighter drums, less bleed, more punch",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the gate tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<GateState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so the
/// sound goes on through an edit and every change glides.
fn apply(state: &GateState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let meters = Meters {
        level: context.peaks(Meters::LEVEL),
        reduction: context.peaks(Meters::REDUCTION),
    };
    let gate = context.processor("gate", || Gate::new(*state, meters))?;
    context.update(gate, *state)?;
    context.input(AUDIO_INPUT, InputEndpoint::new(gate, Gate::INPUT));
    context.input(SIDECHAIN_INPUT, InputEndpoint::new(gate, Gate::SIDECHAIN));
    context.automation(gate, Gate::AUTOMATION);
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(gate, Gate::OUTPUT));
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
        for parameter in PARAMETERS {
            let field = parameter.field;
            let row = format!("| `{field}` |");
            let row = doc.lines().find(|line| line.starts_with(&row));
            let row = row.unwrap_or_else(|| panic!("the doc has no row for {field}"));
            let (min, max) = (parameter.min, parameter.max);
            assert!(row.contains(&format!("| {min} to {max} |")), "{row}");
            assert!(row.contains(&format!("| {} |", parameter.default)), "{row}");
        }
    }
}
