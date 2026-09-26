//! A rig of one source into one reverb into the device, and what the tests measure with.

use std::f64::consts::TAU;

use reverb::{Reverb, ReverbState};
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

/// A sine of this frequency and amplitude in both channels, from phase 0, for `frames` frames
/// and then silence. `usize::MAX` plays for ever.
pub fn sine_for(hz: f64, amplitude: f32, frames: usize, sample_rate: u32) -> Signal {
    let mut phase = 0.0_f64;
    let mut played = 0_usize;
    let step = hz / f64::from(sample_rate);
    Box::new(move || {
        if played >= frames {
            return [0.0; 2];
        }
        played += 1;
        let sample = (TAU * phase).sin() as f32 * amplitude;
        phase = (phase + step).fract();
        [sample; 2]
    })
}

pub fn sine(hz: f64, amplitude: f32) -> Signal {
    sine_for(hz, amplitude, usize::MAX, SAMPLE_RATE)
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

/// The same noise for `frames` frames, then silence.
pub fn burst(amplitude: f32, frames: usize) -> Signal {
    let mut sound = noise(amplitude);
    let mut played = 0_usize;
    Box::new(move || {
        played += 1;
        if played <= frames { sound() } else { [0.0; 2] }
    })
}

/// One frame of full scale in the left channel, then silence.
pub fn impulse() -> Signal {
    let mut first = true;
    Box::new(move || [if std::mem::take(&mut first) { 1.0 } else { 0.0 }, 0.0])
}

/// A source, a reverb and the device, rendering offline.
pub struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub reverb: Node<Reverb>,
}

impl Rig {
    pub fn new(state: ReverbState, signal: Signal) -> Self {
        Self::at_rate(state, signal, SAMPLE_RATE)
    }

    pub fn at_rate(state: ReverbState, signal: Signal, sample_rate: u32) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let reverb = edit.add_processor("reverb", Reverb::new(state)).unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            reverb.id(),
            Reverb::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(reverb.id(), Reverb::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            reverb,
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

    pub fn update(&mut self, state: ReverbState) {
        self.control.update(self.reverb, state).unwrap();
    }
}

/// Only the reverb, with nothing taken off what goes into it and no damping: the plain decay.
pub fn plain(decay_seconds: f32) -> ReverbState {
    ReverbState {
        decay_seconds,
        damping: 0.0,
        low_cut_hz: 20.0,
        high_cut_hz: 20_000.0,
        mix: 1.0,
        ..ReverbState::default()
    }
}

pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

pub fn rms(samples: &[f32]) -> f64 {
    let energy: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    (energy / samples.len().max(1) as f64).sqrt()
}

pub fn db(value: f64) -> f64 {
    20.0 * value.log10()
}

/// The largest step from one sample to the next.
pub fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

/// The slope of the least squares line through `points`, in y per x.
pub fn slope(points: &[(f64, f64)]) -> f64 {
    let count = points.len() as f64;
    let (x_mean, y_mean) = points.iter().fold((0.0, 0.0), |(x, y), point| {
        (x + point.0 / count, y + point.1 / count)
    });
    let (mut over, mut under) = (0.0, 0.0);
    for (x, y) in points {
        over += (x - x_mean) * (y - y_mean);
        under += (x - x_mean).powi(2);
    }
    over / under
}
