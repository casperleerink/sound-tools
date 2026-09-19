//! Tone: a sine oscillator. The smallest real extension processor.
//!
//! Saved state and tool registration arrive with the live project folder.

use sound_core::{AudioOutput, Ports, PrepareConfig, ProcessContext, Processor};

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ToneParameters {
    pub frequency_hz: f32,
    /// Linear gain. 1.0 is full scale.
    pub gain: f32,
}

pub struct Tone {
    parameters: ToneParameters,
    /// In cycles, from 0 to 1. Runtime state: it survives parameter and routing changes.
    phase: f32,
    /// Zero until `prepare` runs, so an unprepared Tone holds its phase.
    seconds_per_frame: f32,
}

impl Tone {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub fn new(parameters: ToneParameters) -> Self {
        Self {
            parameters,
            phase: 0.0,
            seconds_per_frame: 0.0,
        }
    }
}

impl Processor for Tone {
    type Update = ToneParameters;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.seconds_per_frame = 1.0 / config.sample_rate as f32;
    }

    fn update(&mut self, update: &mut ToneParameters) {
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
