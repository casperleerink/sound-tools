//! Signals: what plays the sound of a tool of the project, sample by sample, on the audio thread.
//!
//! The TypeScript SDK sends a sound as a [`Graph`] of JSON nodes, which [`Graph::compile`]
//! checks and lowers to [`Code`]. A [`Machine`] runs that code with the memory of one voice, and
//! a [`Signals`] processor plays one machine per voice.

mod cable;
mod code;
mod graph;
mod machine;
mod processor;
mod program;

pub use cable::{Bang, Cable, Control, Number, Placed, Shown};
pub use code::{ArraySpec, Code, MAX_ARRAY_LENGTH, ParameterSpec};
pub use graph::{ControlSpec, Declarations, Graph, GraphError};
pub use machine::{List, Machine, Values};
pub use processor::{Kind, MAX_VOICES, Signals, SignalsUpdate};

// The tests build sounds with the helpers of `tests/sound`, which name this crate.
#[cfg(test)]
extern crate self as sound_signals;
#[cfg(test)]
mod tests;
