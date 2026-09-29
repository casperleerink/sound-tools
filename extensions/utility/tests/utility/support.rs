//! A rig of one source into one utility into the device, and what the tests measure with.

use std::f64::consts::TAU;

use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, Node, Ports, PrepareConfig,
    ProcessContext, Processor,
};
use utility::{Utility, UtilityState};

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

/// A sine of this frequency, from phase 0, with its amplitude on the left and on the right. A
/// negative amplitude is the sine upside down.
pub fn sine(hz: f64, amplitude: [f32; 2]) -> Signal {
    sine_at(hz, amplitude, SAMPLE_RATE)
}

/// The same at another sample rate.
pub fn sine_at(hz: f64, [left, right]: [f32; 2], sample_rate: u32) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(sample_rate);
    Box::new(move || {
        let sample = (TAU * phase).sin() as f32;
        phase = (phase + step).fract();
        [sample * left, sample * right]
    })
}

/// A sine of one frequency on the left and one of another on the right, so every channel can be
/// told apart.
pub fn two_sines(left_hz: f64, right_hz: f64, amplitude: f32) -> Signal {
    let [mut left, mut right] = [
        sine(left_hz, [amplitude; 2]),
        sine(right_hz, [amplitude; 2]),
    ];
    Box::new(move || [left()[0], right()[0]])
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

/// The first `frames` of a signal, left and right: what the utility hears.
pub fn frames_of(mut signal: Signal, frames: usize) -> [Vec<f32>; 2] {
    let frames: Vec<[f32; 2]> = (0..frames).map(|_| signal()).collect();
    [0, 1].map(|channel| frames.iter().map(|frame| frame[channel]).collect())
}

/// A source, a utility and the device, rendering offline.
pub struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub utility: Node<Utility>,
}

impl Rig {
    pub fn new(state: UtilityState, signal: Signal) -> Self {
        Self::at_rate(state, signal, SAMPLE_RATE)
    }

    pub fn at_rate(state: UtilityState, signal: Signal, sample_rate: u32) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let utility = edit.add_processor("utility", Utility::new(state)).unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            utility.id(),
            Utility::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(utility.id(), Utility::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            utility,
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

    pub fn update(&mut self, state: UtilityState) {
        self.control.update(self.utility, state).unwrap();
    }
}

/// The amplitude of the part of `samples` at `hz`, by correlation with a sine and a cosine of
/// that frequency. `samples` start at frame `start` of a signal whose phase was 0 at frame 0.
pub fn amplitude_at(samples: &[f32], start: usize, hz: f64, sample_rate: u32) -> f64 {
    let (mut sine, mut cosine) = (0.0, 0.0);
    for (offset, sample) in samples.iter().enumerate() {
        let angle = TAU * hz * (start + offset) as f64 / f64::from(sample_rate);
        sine += f64::from(*sample) * angle.sin();
        cosine += f64::from(*sample) * angle.cos();
    }
    2.0 * sine.hypot(cosine) / samples.len() as f64
}

/// The amplitude of a sine through a utility on the left and on the right, once it has settled.
pub fn measured(state: UtilityState, hz: f64, input: [f32; 2]) -> [f64; 2] {
    measured_at(state, hz, input, SAMPLE_RATE)
}

/// The same at another sample rate.
pub fn measured_at(state: UtilityState, hz: f64, input: [f32; 2], sample_rate: u32) -> [f64; 2] {
    // Long enough for the slowest crossover to settle, then whole cycles over about a quarter
    // of a second, so the correlation sees no part of a cycle.
    let settle = sample_rate as usize;
    let cycles = (hz * 0.25).ceil();
    let window = (cycles * f64::from(sample_rate) / hz).round() as usize;
    let mut rig = Rig::at_rate(state, sine_at(hz, input, sample_rate), sample_rate);
    rig.render(settle);
    rig.render(window)
        .map(|channel| amplitude_at(&channel, settle, hz, sample_rate))
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
