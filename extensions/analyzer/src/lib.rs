//! Analyzer: a built-in effect that shows the sound and leaves it as it is. It goes in an effect
//! slot of a track, and its card shows the spectrum, the level (peak and loudness) and the note
//! the sound plays, with how far off it is in cents.
//!
//! A record on disk, `<name>.json` in a track folder, named in the track's `effects`. It has
//! nothing to set:
//!
//! ```json
//! {
//!   "tool": "analyzer",
//!   "state": {}
//! }
//! ```
//!
//! The processor only copies its input to its output and writes it into a [`Scope`]. The card
//! works out what it shows from the scope, off the audio thread, with the measuring code of
//! `--analyze` ([`sound_media::analysis`]).
//!
//! [`view`] is the card of the analyzer, and the only module here that uses GPUI.
//!
//! [`Scope`]: sound_core::Scope

mod analysis;
mod processor;
pub mod view;

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, Registry,
    RegistryError, State,
};
use sound_notes::{AUDIO_INPUT, AUDIO_OUTPUT};

pub use processor::Analyzer;

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "analyzer";

/// The saved state: nothing. What the card shows is runtime state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerState {}

impl State for AnalyzerState {
    const TOOL: &'static str = "analyzer";
}

/// The doc of the analyzer record, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "analyzer",
    when: "An analyzer on a track: the composer sees its spectrum, level and pitch",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the analyzer tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<AnalyzerState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source.
fn apply(_: &AnalyzerState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let scope = context.scope(Analyzer::SCOPE);
    let analyzer = context.processor("analyzer", || Analyzer::new(scope))?;
    context.input(AUDIO_INPUT, InputEndpoint::new(analyzer, Analyzer::INPUT));
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(analyzer, Analyzer::OUTPUT),
    );
    Ok(())
}
