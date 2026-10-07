//! Script: an effect whose sound is a small script in its record. It goes in an effect slot of
//! a track, like any effect. An agent or a composer writes the script; when the record changes
//! the new code plays at once, faded in over 10 ms, with no build.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`:
//!
//! ```json
//! {
//!   "tool": "script",
//!   "state": {
//!     "code": [
//!       "param rate = 4 [0.1, 20]",
//!       "param depth = 0.5 [0, 1]",
//!       "wave = 0.5 + 0.5 * sin(phasor(rate) * tau)",
//!       "out = in * (1 - depth * wave)"
//!     ],
//!     "values": { "rate": 6 }
//!   }
//! }
//! ```
//!
//! The language is in [`language`], what runs it in [`Machine`]. Its reference for writers is
//! `agent-doc.md`.
//!
//! Known gaps of this first version: a script is an effect only, the same code runs on each
//! channel apart, nothing knows the tempo, a param cannot be automated, and the card is not
//! made yet.

pub mod language;
mod machine;
mod processor;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use language::{Code, CompileError, MAX_PARAMETERS, ParameterSpec, compile};
pub use machine::{Machine, Values};
pub use processor::{Script, ScriptUpdate};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "script";

/// The saved state. `"state": {}` is a script with no code, which passes the sound through.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScriptState {
    /// The script, one line per string.
    pub code: Vec<String>,
    /// Where each param stands, by name. A param left out is at the default its line gives.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, f32>,
}

impl ScriptState {
    /// The compiled code and where each of its params stands.
    pub fn compile(&self) -> Result<(Code, Values), String> {
        let code = compile(&self.code).map_err(|error| error.to_string())?;
        if let Some(name) = self.values.keys().find(|name| {
            !code
                .parameters
                .iter()
                .any(|parameter| parameter.name == **name)
        }) {
            return Err(format!("values.{name}: the code has no `param {name}`"));
        }
        let mut values = [0.0; MAX_PARAMETERS];
        for (value, parameter) in values.iter_mut().zip(&code.parameters) {
            *value = self
                .values
                .get(&parameter.name)
                .copied()
                .unwrap_or(parameter.default);
            if !(parameter.min..=parameter.max).contains(value) {
                return Err(format!(
                    "values.{}: {value} is outside [{}, {}]",
                    parameter.name, parameter.min, parameter.max
                ));
            }
        }
        Ok((code, values))
    }
}

impl State for ScriptState {
    const TOOL: &'static str = "script";

    fn validate(&self) -> Result<(), String> {
        self.compile().map(|_| ())
    }
}

/// The doc of the script record and its language, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "script",
    when: "You need an effect no built-in effect is: write it as a short script",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the script tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<ScriptState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state. A machine is made every run, and the processor keeps the one it
/// has unless the code is new, so the memory of a delay goes on through a change of a value.
fn apply(state: &ScriptState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let (code, values) = state.compile().map_err(BehaviourError::Other)?;
    let sample_rate = context.prepare_config().sample_rate as f32;
    let script = context.processor("script", || {
        Script::new(Machine::new(code.clone(), &values, sample_rate))
    })?;
    let machine = Some(Box::new(Machine::new(code, &values, sample_rate)));
    context.update(script, ScriptUpdate { machine, values })?;
    context.input(AUDIO_INPUT, InputEndpoint::new(script, Script::INPUT));
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(script, Script::OUTPUT));
    Ok(())
}
