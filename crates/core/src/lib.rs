//! The Sound Tools core: audio engine, musical clock, transport, project storage and editing.
//!
//! Independent of GPUI, and of musical conventions such as tracks, clips and notes.
//! Those belong to extensions.
//! `README.md` in this crate is the guide for extension authors: processors and tools.

mod apps;
mod automation;
mod clock;
mod compensation;
mod control;
mod delay_line;
mod device;
mod engine;
mod event_tap;
mod graph;
mod input;
mod parameter;
mod peaks;
pub mod process;
mod processor;
mod project;
mod ring;
mod scope;
mod sections;
pub mod testing;
mod thru;
mod transport;
mod watch;

pub use apps::{AppSound, app_processes};
pub use automation::{
    Automated, Automation, AutomationInput, Lane, MAX_AUTOMATED, PlayedLanes, Targets,
};
pub use clock::{
    Bar, BarBeat, Clock, ClockError, Frames, MIN_EXACT_SAMPLE_RATE, SignatureRun,
    TICKS_PER_QUARTER, Tempo, TempoChange, TempoMap, Ticks, TimeSignature, TimeSignatures,
};
pub use control::{Edit, EngineConfig, EngineControl, EngineStopped, Node};
pub use delay_line::DelayLine;
pub use device::{
    DeviceChoice, DeviceError, DeviceStatus, OutputDevice, OutputStream, StreamTiming,
    input_devices, monotonic_nanos, output_devices,
};
pub use engine::{Engine, EngineStatus};
pub use event_tap::{EventTap, TapRead, Tappable};
pub use graph::{Connection, Destination, GraphError, NodeId, Source};
pub use input::{
    CAPTURE_SECONDS, CaptureReader, CaptureStatus, CaptureWriter, InputDevice, InputId,
    InputStream, LiveInput, LiveWriter, capture, live_input,
};
pub use parameter::{
    AutomatedNumber, FADER_TOP_DB, FADER_UNITY, Parameter, ParameterInfo, Scale, ValueRange,
};
pub use peaks::Peaks;
pub use processor::{
    AudioInput, AudioInputs, AudioOutput, AudioOutputs, CHANNELS, Event, EventInput, EventInputs,
    EventOutput, EventOutputs, InputPort, MAX_BLOCK, OutputPort, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Timed,
};
pub use project::{
    AGENT_DOC_FILE, AGENT_DOCS_FOLDER, ASSETS_FOLDER, AgentDoc, AgentDocText, AssetError,
    AssetName, Assets, BehaviourContext, BehaviourError, Changes, Edit as ProjectEdit, FORMAT,
    FieldDoc, FieldRange, GROUPING_WINDOW, INSTRUCTIONS_FILE, InputEndpoint, Instance, InstanceId,
    InvalidAssetName, InvalidInstanceId, JsonTool, JsonToolDoc, NO_PROBLEMS, OUTSIDE_UNDO_WINDOW,
    OutputEndpoint, PROBLEMS_FILE, Place, PortReference, Problem, Project, ProjectError,
    ProjectEvent, ProjectFile, RUN_LONGEST, RUN_QUIET, RecordSchema, RecordStamp, Registry,
    RegistryError, SavedConnection, SavedDestination, SavedSource, State, StorageError,
    ToolRegistration, ToolSchema, parse_state, ranges, type_doc, unit_of,
};
pub use scope::Scope;
pub use sections::{Section, SectionError, SectionName, Sections};
pub use thru::{AudioThru, EventThru};
pub use transport::Transport;
pub use watch::Watch;
