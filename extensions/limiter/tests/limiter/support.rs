//! A rig of one source into one limiter into the device, and what the tests measure with.

use std::f64::consts::TAU;

use limiter::{Limiter, LimiterState, Meters};
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

/// A steady value in both channels.
pub fn steady(value: f32) -> Signal {
    Box::new(move || [value; 2])
}

/// A steady value in both channels, and another from `from` for `frames`: the cleanest peak,
/// because the level of a constant is the constant and the gain is the output over it.
pub fn burst(value: f32, from: usize, frames: usize, loud: f32) -> Signal {
    let mut frame = 0_usize;
    Box::new(move || {
        let sample = match (from..from + frames).contains(&frame) {
            true => loud,
            false => value,
        };
        frame += 1;
        [sample; 2]
    })
}

/// White noise from -amplitude to amplitude, the same every run for a seed, other in each
/// channel.
pub fn noise(amplitude: f32, seed: u64) -> Signal {
    let mut state = 0x2545_f491_4f6c_dd1d_u64 ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    Box::new(move || [next() * amplitude, next() * amplitude])
}

/// The first `frames` frames of a signal, left channel.
pub fn take(mut signal: Signal, frames: usize) -> Vec<f32> {
    (0..frames).map(|_| signal()[0]).collect()
}

/// The factor of a level in dB, worked out as the limiter does.
pub fn amplitude(db: f32) -> f32 {
    10_f64.powf(f64::from(db) / 20.0) as f32
}

/// A source, a limiter and the device, rendering offline.
pub struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub limiter: Node<Limiter>,
    /// What the card of this limiter would read.
    pub meters: Meters,
}

impl Rig {
    pub fn new(state: LimiterState, signal: Signal) -> Self {
        Self::at(SAMPLE_RATE, state, signal)
    }

    pub fn at(sample_rate: u32, state: LimiterState, signal: Signal) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let meters = Meters::default();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let limiter = edit
            .add_processor("limiter", Limiter::new(state, meters.clone()))
            .unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            limiter.id(),
            Limiter::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(limiter.id(), Limiter::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            limiter,
            meters,
        }
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        self.render_in_blocks(frames, 480)
    }

    /// The same in device buffers of `block` frames.
    pub fn render_in_blocks(&mut self, frames: usize, block: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(block * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    pub fn update(&mut self, state: LimiterState) {
        self.control.update(self.limiter, state).unwrap();
    }

    /// The latency the engine has from the limiter after one more frame.
    pub fn latency(&mut self) -> u64 {
        self.render(1);
        self.control.poll().unwrap().latency
    }
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
