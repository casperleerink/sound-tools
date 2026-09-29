//! A rig of one source into one delay into the device, and what the tests measure with.

use std::f64::consts::TAU;

use delay::{Delay, DelayState};
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, Node, Ports, PrepareConfig,
    ProcessContext, Processor, Tempo, TempoMap, TimeSignature,
};

pub const SAMPLE_RATE: u32 = 48_000;
pub const SECOND: usize = SAMPLE_RATE as usize;

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

/// One frame of full scale at each of these frames, in the left channel, and silence between.
pub fn clicks(at: Vec<usize>) -> Signal {
    let mut frame = 0_usize;
    Box::new(move || {
        let click = at.contains(&frame);
        frame += 1;
        [if click { 1.0 } else { 0.0 }, 0.0]
    })
}

/// One click at the first frame.
pub fn impulse() -> Signal {
    clicks(vec![0])
}

/// A tempo map of one tempo in 4/4.
pub fn tempo(bpm: f64) -> TempoMap {
    let four_four = TimeSignature::new(4, 4).unwrap();
    TempoMap::constant(four_four, Tempo::from_bpm(bpm).unwrap())
}

/// A source, a delay and the device, rendering offline. The transport stands at the start, at
/// 120 bpm, until a test says otherwise.
pub struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub delay: Node<Delay>,
}

impl Rig {
    pub fn new(state: DelayState, signal: Signal) -> Self {
        Self::at_rate(state, signal, SAMPLE_RATE)
    }

    pub fn at_rate(state: DelayState, signal: Signal, sample_rate: u32) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let delay = edit.add_processor("delay", Delay::new(state)).unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            delay.id(),
            Delay::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(delay.id(), Delay::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            delay,
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

    pub fn update(&mut self, state: DelayState) {
        self.control.update(self.delay, state).unwrap();
    }
}

/// Only the repeats, with nothing cut: what a test of time and level measures.
pub fn wet() -> DelayState {
    DelayState {
        low_cut_hz: 20.0,
        high_cut_hz: 20_000.0,
        mix: 1.0,
        ..DelayState::default()
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

/// The frame of the loudest sample in `from..to`.
pub fn loudest_frame(samples: &[f32], from: usize, to: usize) -> usize {
    let window = &samples[from..to];
    let (index, _) =
        window
            .iter()
            .enumerate()
            .fold((0, 0.0_f32), |(best, level), (index, sample)| {
                if sample.abs() > level {
                    (index, sample.abs())
                } else {
                    (best, level)
                }
            });
    from + index
}
