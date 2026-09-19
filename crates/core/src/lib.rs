//! The Sound Tools core: audio engine, musical clock, transport, project storage and editing.
//!
//! Independent of GPUI, and of musical conventions such as tracks, clips and notes.
//! Those belong to extensions. See ARCHITECTURE.md and ENGINEERING.md section 3.
//! `README.md` in this crate explains the engine API for extension authors.

mod control;
mod device;
mod engine;
mod graph;
mod processor;

pub use control::{Edit, EngineConfig, EngineControl, Node};
pub use device::{DeviceError, DeviceStatus, OutputDevice, OutputStream};
pub use engine::{Engine, EngineStatus};
pub use graph::{Connection, Destination, GraphError, NodeId};
pub use processor::{
    AudioInput, AudioInputs, AudioOutput, AudioOutputs, Event, EventInput, EventInputs,
    EventOutput, EventOutputs, InputPort, MAX_BLOCK, OutputPort, Ports, PrepareConfig,
    ProcessContext, Processor, Timed,
};
