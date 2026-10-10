//! Hum: the small language a sound is written in, sample by sample, and what runs it on the
//! audio thread. Nobody writes it by hand: the SDK of the tools a project writes in TypeScript
//! turns their sound graph into Hum, which is easy to read when something is wrong.
//!
//! [`compile`] turns lines of Hum into [`Code`], a flat list of operations. A [`Machine`] runs
//! it once per frame and channel with all the memory of one voice, made on the control thread.
//! A [`Hum`] processor plays one machine per voice as an effect, an instrument or a source,
//! and fades to new ones when the code changes.

mod graph;
mod language;
mod machine;
mod processor;

pub use graph::{ControlSpec, Declarations, Graph, GraphError};
pub use language::{
    ArraySpec, Code, CompileError, MAX_LIVES, MAX_PARAMETERS, MAX_WATCHES, ParameterSpec, compile,
};
pub use machine::{Machine, Values};
pub use processor::{Hum, HumUpdate, Kind, MAX_VOICES};

#[cfg(test)]
mod tests;
