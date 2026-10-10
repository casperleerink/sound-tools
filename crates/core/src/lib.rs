//! The Sound Tools core: audio engine, musical clock, transport, project storage and editing.
//!
//! Independent of GPUI, and of musical conventions such as tracks, clips and notes.
//! Those belong to extensions. See ARCHITECTURE.md and ENGINEERING.md section 3.
//! `README.md` in this crate is the guide for extension authors: processors and tools.

mod apps;
mod automation;
mod clock;
mod control;
mod delay_line;
mod detector;
mod device;
mod dsp;
mod engine;
mod envelope;
mod gain;
mod graph;
mod input;
mod lfo;
mod limiter;
mod oversampling;
mod parameter;
mod peaks;
pub mod process;
mod processor;
mod project;
mod saturation;
mod svf;
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
pub use detector::{PeakDetector, pole};
pub use device::{
    DeviceChoice, DeviceError, DeviceStatus, OutputDevice, OutputStream, StreamTiming,
    input_devices, monotonic_nanos, output_devices,
};
pub use dsp::{
    HIGHEST_PHASE_STEP, OnePole, Taps, all_held_silent, all_positive_zero, all_zero, held,
    poly_blep,
};
pub use engine::{Engine, EngineStatus};
pub use envelope::{ENVELOPE_FLOOR, Envelope, EnvelopeCurves, EnvelopeStage, EnvelopeState};
pub use gain::{amplitude, pan_gains};
pub use graph::{Connection, Destination, GraphError, NodeId, Source};
pub use input::{
    CAPTURE_SECONDS, CaptureReader, CaptureStatus, CaptureWriter, InputDevice, InputId,
    InputStream, LiveInput, LiveWriter, capture, live_input,
};
pub use lfo::{Lfo, LfoShape};
pub use limiter::PeakLimiter;
pub use oversampling::{Oversampler, OversamplingFilters};
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
    AssetName, Assets, BehaviourContext, BehaviourError, Changes, Derived, Edit as ProjectEdit,
    FORMAT, GROUPING_WINDOW, INSTRUCTIONS_FILE, InputEndpoint, Instance, InstanceId,
    InvalidAssetName, InvalidInstanceId, JsonTool, JsonToolDoc, NO_PROBLEMS, OUTSIDE_UNDO_WINDOW,
    OutputEndpoint, PROBLEMS_FILE, Place, PortReference, Problem, Project, ProjectError,
    ProjectEvent, ProjectFile, Registry, RegistryError, SavedConnection, SavedDestination,
    SavedSource, State, StorageError, ToolRegistration, Was,
};
pub use saturation::soft_clip;
pub use svf::{FilterSlope, FilterType, SVF_MAX_Q, SvfFactors, SvfSection, svf_response};
pub use transport::Transport;
pub use watch::Watch;
