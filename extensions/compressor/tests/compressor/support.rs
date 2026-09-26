//! A rig of one source into one compressor into the device, and what the tests measure with.

use std::f64::consts::TAU;

use compressor::{Compressor, CompressorState};
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, Node, Ports, PrepareConfig,
    ProcessContext, Processor,
};

pub const SAMPLE_RATE: u32 = 48_000;

/// Makes the next frame of a signal, left and right. Called on the audio thread.
pub type Signal = Box<dyn FnMut() -> [f32; 2] + Send>;

/// A processor that plays a signal. The closure is made on the control thread; calling it
/// allocates nothing.
pub struct Source {
    signal: Signal,
}

impl Source {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub fn new(signal: Signal) -> Self {
        Self { signal }
    }
}

impl Processor for Source {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            [*left, *right] = (self.signal)();
        }
    }
}

/// A sine of this frequency and amplitude in both channels, from phase 0.
pub fn sine(hz: f64, amplitude: f32) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(SAMPLE_RATE);
    Box::new(move || {
        let sample = (TAU * phase).sin() as f32 * amplitude;
        phase = (phase + step).fract();
        [sample; 2]
    })
}

/// A steady value in both channels that changes to another at a frame: the cleanest level
/// step, because the level of a constant is the constant and the gain is the output over it.
pub fn step(before: f32, at: usize, after: f32) -> Signal {
    let mut frame = 0_usize;
    Box::new(move || {
        let value = if frame < at { before } else { after };
        frame += 1;
        [value; 2]
    })
}

/// White noise from -amplitude to amplitude, the same every run, other in each channel.
pub fn noise(amplitude: f32) -> Signal {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    Box::new(move || [next() * amplitude, next() * amplitude])
}

/// A source, a compressor and the device, rendering offline.
pub struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub compressor: Node<Compressor>,
}

impl Rig {
    pub fn new(state: CompressorState, signal: Signal) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(SAMPLE_RATE, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let compressor = edit
            .add_processor("compressor", Compressor::new(state))
            .unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            compressor.id(),
            Compressor::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(
            compressor.id(),
            Compressor::OUTPUT,
            0,
        ))
        .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            compressor,
        }
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(480 * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    pub fn update(&mut self, state: CompressorState) {
        self.control.update(self.compressor, state).unwrap();
    }
}

/// The amplitude of the part of `samples` at `hz`, by correlation with a sine and a cosine of
/// that frequency. `samples` start at frame `start` of a signal whose phase was 0 at frame 0.
pub fn amplitude_at(samples: &[f32], start: usize, hz: f64) -> f64 {
    let (mut sine, mut cosine) = (0.0, 0.0);
    for (offset, sample) in samples.iter().enumerate() {
        let angle = TAU * hz * (start + offset) as f64 / f64::from(SAMPLE_RATE);
        sine += f64::from(*sample) * angle.sin();
        cosine += f64::from(*sample) * angle.cos();
    }
    2.0 * sine.hypot(cosine) / samples.len() as f64
}

/// The gain of a compressor with this record on a steady sine of this peak level, measured once
/// it has settled, in dB. The settling is two seconds: every release of these tests is
/// shorter than a tenth of that.
pub fn measured_gain_db(state: CompressorState, hz: f64, level_db: f32) -> f64 {
    let amplitude = 10_f32.powf(level_db / 20.0);
    let mut rig = Rig::new(state, sine(hz, amplitude));
    let settle = 2 * SAMPLE_RATE as usize;
    rig.render(settle);
    // Whole cycles over a quarter of a second, so the correlation sees no part of a cycle.
    let window = ((hz * 0.25).ceil() * f64::from(SAMPLE_RATE) / hz).round() as usize;
    let [left, right] = rig.render(window);
    assert_eq!(left, right);
    let latency = state.lookahead.frames(SAMPLE_RATE as f32);
    let measured = amplitude_at(&left, settle - latency, hz);
    20.0 * (measured / f64::from(amplitude)).log10()
}

pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// The largest step from one sample to the next.
pub fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}
