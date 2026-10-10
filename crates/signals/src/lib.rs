//! Signals: what plays the sound of a tool of the project, sample by sample, on the audio thread.
//!
//! The SDK of the tools a project writes in TypeScript sends a sound as a [`Graph`] of typed
//! JSON nodes, which [`Graph::compile`] checks and lowers to [`Code`], a flat list of
//! operations. A [`Machine`] runs it over a span of frames, an operation at a time, with all
//! the memory of one voice, made on the control thread; only operations around a feedback or a
//! buffer run frame by frame. A [`Signals`] processor plays one machine per voice as an effect,
//! an instrument or a source, and fades to new ones when the code changes.

mod code;
mod graph;
mod machine;
mod processor;
mod program;

pub use code::{ArraySpec, Code, ParameterSpec};
pub use graph::{ControlSpec, Declarations, Graph, GraphError};
pub use machine::{Machine, Values};
pub use processor::{Kind, MAX_VOICES, Signals, SignalsUpdate};

// The tests build sounds with the helpers of `tests/sound`, which name this crate.
#[cfg(test)]
extern crate self as sound_signals;
#[cfg(test)]
mod tests;
