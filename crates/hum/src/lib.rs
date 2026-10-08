//! Hum: the small language a sound is written in, sample by sample, and what runs it on the
//! audio thread. A tool a project writes in TypeScript plays the Hum its code generates.
//!
//! [`compile`] turns lines of Hum into [`Code`], a flat list of operations. A [`Machine`] runs
//! it once per frame and channel with all the memory of one voice, made on the control thread.
//! A [`Hum`] processor plays one machine per voice as an effect, an instrument or a source,
//! and fades to new ones when the code changes.
//!
//! The reference for writers is `agent-doc.md`.

mod language;
mod machine;
mod processor;

pub use language::{
    ArraySpec, Code, CompileError, MAX_LIVES, MAX_PARAMETERS, MAX_WATCHES, ParameterSpec, compile,
};
pub use machine::{Machine, Values};
pub use processor::{Hum, HumUpdate, Kind, MAX_VOICES};

/// The reference of the language, for an agent that writes the sound of a tool.
pub const AGENT_DOC: sound_core::AgentDoc = sound_core::AgentDoc {
    name: "hum",
    when: "You write or fix the sound of a tool of this project: the Hum language",
    markdown: include_str!("../agent-doc.md"),
};

#[cfg(test)]
mod tests;
