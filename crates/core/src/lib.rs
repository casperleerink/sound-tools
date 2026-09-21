//! The Sound Tools core: audio engine, musical clock, transport, project storage and editing.
//!
//! Independent of GPUI, and of musical conventions such as tracks, clips and notes.
//! Those belong to extensions. See ARCHITECTURE.md and ENGINEERING.md section 3.
//! `README.md` in this crate is the guide for extension authors: processors and tools.

mod clock;
mod control;
mod device;
mod engine;
mod graph;
mod processor;
mod project;
mod transport;

pub use clock::{
    BarBeat, Clock, ClockError, Frames, MIN_EXACT_SAMPLE_RATE, TICKS_PER_QUARTER, Tempo,
    TempoChange, TempoMap, Ticks, TimeSignature,
};
pub use control::{Edit, EngineConfig, EngineControl, EngineStopped, Node};
pub use device::{
    DeviceError, DeviceStatus, OutputDevice, OutputStream, StreamTiming, monotonic_nanos,
};
pub use engine::{Engine, EngineStatus};
pub use graph::{Connection, Destination, GraphError, NodeId};
pub use processor::{
    AudioInput, AudioInputs, AudioOutput, AudioOutputs, CHANNELS, Event, EventInput, EventInputs,
    EventOutput, EventOutputs, InputPort, MAX_BLOCK, OutputPort, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Timed,
};
pub use project::{
    AGENT_DOC_FILE, AGENT_DOCS_FOLDER, AgentDoc, BehaviourContext, BehaviourError, Changes,
    Edit as ProjectEdit, FORMAT, GROUPING_WINDOW, InputEndpoint, Instance, InstanceId,
    InvalidInstanceId, NO_PROBLEMS, OUTSIDE_UNDO_WINDOW, OutputEndpoint, PROBLEMS_FILE, Place,
    PortReference, Problem, Project, ProjectError, ProjectEvent, ProjectFile, Registry,
    RegistryError, SavedConnection, SavedDestination, State, StorageError, ToolRegistration,
};
pub use transport::Transport;
