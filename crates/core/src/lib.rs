//! The Sound Tools core: audio engine, musical clock, transport, project storage and editing.
//!
//! Independent of GPUI, and of musical conventions such as tracks, clips and notes.
//! Those belong to extensions. See ARCHITECTURE.md and ENGINEERING.md section 3.
//! `README.md` in this crate is the guide for extension authors: processors and tools.

mod clock;
mod control;
mod device;
mod engine;
mod envelope;
mod graph;
mod input;
mod parameter;
mod peaks;
mod processor;
mod project;
mod saturation;
mod transport;

pub use clock::{
    Bar, BarBeat, Clock, ClockError, Frames, MIN_EXACT_SAMPLE_RATE, SignatureRun,
    TICKS_PER_QUARTER, Tempo, TempoChange, TempoMap, Ticks, TimeSignature, TimeSignatures,
};
pub use control::{Edit, EngineConfig, EngineControl, EngineStopped, Node};
pub use device::{
    DeviceError, DeviceStatus, OutputDevice, OutputStream, StreamTiming, monotonic_nanos,
};
pub use engine::{Engine, EngineStatus};
pub use envelope::{ENVELOPE_FLOOR, Envelope, EnvelopeStage, EnvelopeState};
pub use graph::{Connection, Destination, GraphError, NodeId};
pub use input::{
    CAPTURE_SECONDS, CaptureReader, CaptureStatus, CaptureWriter, InputDevice, InputStream, capture,
};
pub use parameter::Parameter;
pub use peaks::Peaks;
pub use processor::{
    AudioInput, AudioInputs, AudioOutput, AudioOutputs, CHANNELS, Event, EventInput, EventInputs,
    EventOutput, EventOutputs, InputPort, MAX_BLOCK, OutputPort, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Timed,
};
pub use project::{
    AGENT_DOC_FILE, AGENT_DOCS_FOLDER, ASSETS_FOLDER, AgentDoc, AssetError, AssetName, Assets,
    BehaviourContext, BehaviourError, Changes, Derived, Edit as ProjectEdit, FORMAT,
    GROUPING_WINDOW, InputEndpoint, Instance, InstanceId, InvalidAssetName, InvalidInstanceId,
    NO_PROBLEMS, OUTSIDE_UNDO_WINDOW, OutputEndpoint, PROBLEMS_FILE, Place, PortReference, Problem,
    Project, ProjectError, ProjectEvent, ProjectFile, Registry, RegistryError, SavedConnection,
    SavedDestination, State, StorageError, ToolRegistration, Was,
};
pub use saturation::soft_clip;
pub use transport::Transport;
