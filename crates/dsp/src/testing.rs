//! For the tests of a processor: signals to play into it, and what to measure of its sound.

use std::f64::consts::TAU;

use sound_core::{AudioOutput, Ports, PrepareConfig, ProcessContext, Processor};

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
pub fn sine_at(hz: f64, amplitude: f32, sample_rate: u32) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(sample_rate);
    Box::new(move || {
        let sample = (TAU * phase).sin() as f32 * amplitude;
        phase = (phase + step).fract();
        [sample; 2]
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

/// The loudest sample.
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

pub fn rms(samples: &[f32]) -> f64 {
    let energy: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    (energy / samples.len().max(1) as f64).sqrt()
}

/// An amplitude in dB.
pub fn db(value: f64) -> f64 {
    20.0 * value.log10()
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

/// The sound of a steady tone at `bin` times `sample_rate / window`, split into what is in tune
/// with it and what is not: the amplitude of each harmonic under 20 kHz, the fundamental first,
/// and the level of everything else under 20 kHz but DC against the fundamental, in dB. What is
/// not in tune is what folded back from above the Nyquist frequency. `samples` is one window
/// of the settled sound, starting at a whole number of cycles.
///
/// Make `bin` a prime number, so nothing that folds back lands on a harmonic.
pub struct Spectrum {
    pub harmonics: Vec<f64>,
    pub out_of_tune_db: f64,
}

impl Spectrum {
    pub fn of(samples: &[f32], bin: usize, sample_rate: u32) -> Self {
        let window = samples.len();
        let bin_hz = f64::from(sample_rate) / window as f64;
        let top = (20_000.0 / bin_hz) as usize;
        let (mut harmonics, mut out_of_tune) = (Vec::new(), 0.0);
        for each in 1..=top {
            let amplitude = amplitude_at(samples, 0, each as f64 * bin_hz, sample_rate);
            match each % bin == 0 {
                true => harmonics.push(amplitude),
                false => out_of_tune += amplitude * amplitude,
            }
        }
        let fundamental = harmonics.first().copied().unwrap_or_default();
        let out_of_tune_db = 10.0 * (out_of_tune / (fundamental * fundamental)).log10();
        Self {
            harmonics,
            out_of_tune_db,
        }
    }

    /// A harmonic against the fundamental, in dB: 2 is the second.
    pub fn harmonic_db(&self, number: usize) -> f64 {
        let level = |index: usize| self.harmonics.get(index).copied().unwrap_or_default();
        20.0 * (level(number - 1) / level(0)).log10()
    }
}
