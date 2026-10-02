//! A rig of one source into one modulation into the device, and what the tests measure with.

use std::f64::consts::TAU;

use modulation::{Modulation, ModulationState};
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, Node, Ports, PrepareConfig,
    ProcessContext, Processor,
};

pub(crate) const SAMPLE_RATE: u32 = 48_000;
pub(crate) const SECOND: usize = SAMPLE_RATE as usize;

/// Makes the next frame of a signal, left and right. Called on the audio thread.
pub(crate) type Signal = Box<dyn FnMut() -> [f32; 2] + Send>;

/// A processor that plays a signal. The closure is made on the control thread; calling it
/// allocates nothing.
pub(crate) struct Source {
    signal: Signal,
}

impl Source {
    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub(crate) fn new(signal: Signal) -> Self {
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
pub(crate) fn sine(hz: f64, amplitude: f32) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(SAMPLE_RATE);
    Box::new(move || {
        let sample = (TAU * phase).sin() as f32 * amplitude;
        phase = (phase + step).fract();
        [sample; 2]
    })
}

/// White noise from -amplitude to amplitude, the same every run, other in each channel.
pub(crate) fn noise(amplitude: f32) -> Signal {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    Box::new(move || [next() * amplitude, next() * amplitude])
}

/// A signal for `frames` frames, then silence.
pub(crate) fn for_frames(mut signal: Signal, frames: usize) -> Signal {
    let mut played = 0_usize;
    Box::new(move || {
        played += 1;
        if played <= frames { signal() } else { [0.0; 2] }
    })
}

/// A source, a modulation and the device, rendering offline.
pub(crate) struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub modulation: Node<Modulation>,
}

impl Rig {
    pub(crate) fn new(state: ModulationState, signal: Signal) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(SAMPLE_RATE, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let modulation = edit
            .add_processor("modulation", Modulation::new(state))
            .unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            modulation.id(),
            Modulation::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(
            modulation.id(),
            Modulation::OUTPUT,
            0,
        ))
        .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            modulation,
        }
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub(crate) fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(480 * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    pub(crate) fn update(&mut self, state: ModulationState) {
        self.control.update(self.modulation, state).unwrap();
    }
}

/// The amplitude of the part of `samples` at `hz`, by correlation with a sine and a cosine of
/// that frequency. `samples` start at frame `start` of a signal whose phase was 0 at frame 0.
pub(crate) fn amplitude_at(samples: &[f32], start: usize, hz: f64) -> f64 {
    let (mut sine, mut cosine) = (0.0, 0.0);
    for (offset, sample) in samples.iter().enumerate() {
        let angle = TAU * hz * (start + offset) as f64 / f64::from(SAMPLE_RATE);
        sine += f64::from(*sample) * angle.sin();
        cosine += f64::from(*sample) * angle.cos();
    }
    2.0 * sine.hypot(cosine) / samples.len() as f64
}

/// The frequency of a tone over time: the time of each rising zero crossing, placed between
/// its two frames, and the frequency from the crossing before to it. In frames and Hz.
pub(crate) fn frequencies(samples: &[f32]) -> Vec<(f64, f64)> {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(index, pair)| {
            let (before, after) = (f64::from(pair[0]), f64::from(pair[1]));
            index as f64 + before / (before - after)
        })
        .collect();
    crossings
        .windows(2)
        .map(|pair| (pair[1], f64::from(SAMPLE_RATE) / (pair[1] - pair[0])))
        .collect()
}

/// The pitch of a tone in bins of `frames`: the mean of the frequencies of its crossings in each.
pub(crate) fn pitch_in_bins(samples: &[f32], frames: usize) -> Vec<f64> {
    let mut bins = vec![(0.0, 0_usize); samples.len() / frames];
    for (time, hz) in frequencies(samples) {
        if let Some((sum, count)) = bins.get_mut(time as usize / frames) {
            *sum += hz;
            *count += 1;
        }
    }
    bins.into_iter()
        .map(|(sum, count)| sum / count.max(1) as f64)
        .collect()
}

/// The correlation of two lists of numbers, from -1 to 1.
pub(crate) fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    let (a_mean, b_mean) = (mean(a), mean(b));
    let (mut both, mut a_square, mut b_square) = (0.0, 0.0, 0.0);
    for (a, b) in a.iter().zip(b) {
        both += (a - a_mean) * (b - b_mean);
        a_square += (a - a_mean).powi(2);
        b_square += (b - b_mean).powi(2);
    }
    both / (a_square * b_square).sqrt()
}

pub(crate) fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

pub(crate) fn rms(samples: &[f32]) -> f64 {
    let energy: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    (energy / samples.len().max(1) as f64).sqrt()
}

pub(crate) fn db(value: f64) -> f64 {
    20.0 * value.log10()
}

/// The largest sixth difference of the samples: what is left of the sound above a few kHz. It
/// takes a tone of `ω` radians per frame down to `ω⁶` of its level, a few millionths for a tone
/// of 440 Hz at full scale, and keeps a step or a corner at about its own size: a click stands
/// out by thousands of times, however the level of the tone swells and falls.
pub(crate) fn crackle(samples: &[f32]) -> f32 {
    samples
        .windows(7)
        .map(|seven| {
            let [a, b, c, d, e, f, g] = [0, 1, 2, 3, 4, 5, 6].map(|index| f64::from(seven[index]));
            (a - 6.0 * b + 15.0 * c - 20.0 * d + 15.0 * e - 6.0 * f + g).abs() as f32
        })
        .fold(0.0, f32::max)
}
