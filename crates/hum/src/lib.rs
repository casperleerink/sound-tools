//! Hum: the small language a sound is written in, sample by sample, and what runs it on the
//! audio thread. The Script effect plays Hum from its record; a tool a project writes in
//! TypeScript plays Hum that its code generates.
//!
//! [`compile`] turns lines of Hum into [`Code`], a flat list of operations. A [`Machine`] runs
//! it once per frame and channel with all the memory it needs, made on the control thread. A
//! [`Hum`] processor plays a machine, and fades to a new one when the code changes.
//!
//! The reference for writers is `extensions/script/agent-doc.md`.

mod language;
mod machine;
mod processor;

pub use language::{Code, CompileError, MAX_PARAMETERS, ParameterSpec, compile};
pub use machine::{Machine, Values};
pub use processor::{Hum, HumUpdate};
