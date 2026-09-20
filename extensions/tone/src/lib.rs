//! Tone: a sine oscillator. The smallest real tool: one record, one processor, one output.
//!
//! A record on disk, `state/<name>.json`:
//!
//! ```json
//! {
//!   "tool": "tone",
//!   "state": {"frequency_hz": 220.0, "gain": 0.2}
//! }
//! ```

use serde::{Deserialize, Serialize};
use sound_core::{
    AgentDoc, AudioOutput, BehaviourContext, BehaviourError, OutputEndpoint, Ports, PrepareConfig,
    ProcessContext, Processor, Registry, RegistryError, State,
};

/// The name to enable in `project.json`.
pub const EXTENSION: &str = "tone";

/// The name of the mono audio output, for connections in `project.json`.
pub const AUDIO_OUTPUT: &str = "audio";

/// The saved state. It is small and `Copy`, so it is also the update the processor gets.
/// Oscillator phase is runtime state and is not saved.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToneState {
    pub frequency_hz: f32,
    /// Linear gain. 1.0 is full scale.
    pub gain: f32,
}

impl Default for ToneState {
    fn default() -> Self {
        Self {
            frequency_hz: 220.0,
            gain: 0.2,
        }
    }
}

impl State for ToneState {
    const TOOL: &'static str = "tone";

    fn validate(&self) -> Result<(), String> {
        if !(1.0..=20_000.0).contains(&self.frequency_hz) {
            return Err(format!(
                "frequency_hz must be from 1 to 20000, not {}",
                self.frequency_hz
            ));
        }
        if !(0.0..=1.0).contains(&self.gain) {
            return Err(format!("gain must be from 0 to 1, not {}", self.gain));
        }
        Ok(())
    }
}

/// The doc of the tool, for an agent with only file access.
pub const AGENT_DOC: AgentDoc = AgentDoc {
    name: "tone",
    when: "You work on a `tone` record: a steady sine outside the arrangement",
    markdown: include_str!("../agent-doc.md"),
};

/// Registers the Tone tool. Call it before the project opens.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.tool::<ToneState>(EXTENSION)?.behaviour(apply);
    registry.agent_doc(EXTENSION, AGENT_DOC)?;
    Ok(())
}

/// Runs for every valid state, from every source. The processor is kept between runs, so its
/// phase goes on through parameter edits.
fn apply(state: &ToneState, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let tone = context.processor("oscillator", || Tone::new(*state))?;
    context.update(tone, *state)?;
    context.output(AUDIO_OUTPUT, OutputEndpoint::new(tone, Tone::OUTPUT));
    Ok(())
}

pub struct Tone {
    parameters: ToneState,
    /// In cycles, from 0 to 1. Runtime state: it survives parameter and routing changes.
    phase: f32,
    /// Zero until `prepare` runs, so an unprepared Tone holds its phase.
    seconds_per_frame: f32,
}

impl Tone {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub fn new(parameters: ToneState) -> Self {
        Self {
            parameters,
            phase: 0.0,
            seconds_per_frame: 0.0,
        }
    }
}

impl Processor for Tone {
    type Update = ToneState;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.seconds_per_frame = 1.0 / config.sample_rate as f32;
    }

    fn update(&mut self, update: &mut ToneState) {
        self.parameters = *update;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let step = self.parameters.frequency_hz * self.seconds_per_frame;
        for sample in context.audio_outputs.get(Self::OUTPUT) {
            *sample = (self.phase * std::f32::consts::TAU).sin() * self.parameters.gain;
            self.phase = (self.phase + step).fract();
        }
    }
}
