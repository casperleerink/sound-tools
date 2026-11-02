//! Checks for the tests of a processor: the level of one frequency, and what its sound folds
//! back.

use std::f64::consts::TAU;

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
