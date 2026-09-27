//! Plays a file at any speed: what an instrument needs that plays a sample at the pitch of a
//! key, such as the Sampler and the Drum pad.
//!
//! [`Resampler`](crate::Resampler) is exact for one pair of sample rates, a fraction worked out
//! in integers. A key plays at a ratio that is no fraction, `2^(semitones / 12)` times the ratio
//! of the rates, so this reads the file at any place `position + n * step`, in `f64`. It keeps
//! no state, like the resampler: the caller keeps the place and moves it on by `step` per frame.
//!
//! The filter is one windowed sinc (Kaiser), 32 taps from a table of 256 phases with a straight
//! line between two phases. It is an interpolating filter: its cutoff is the Nyquist frequency
//! of the file, so at a whole place it gives the sample of the file exactly, and a file played
//! at its own rate and speed comes out sample for sample. Level is flat to about 40 % of the
//! rate of the file; what is above that is taken down, most of it above the Nyquist frequency.
//! It makes no room for a step above 1: a key above the root plays what the file holds above
//! the Nyquist frequency of the engine divided by the step, and that folds back. For most
//! samples that is quiet.

use std::sync::LazyLock;

use crate::file::Audio;
use crate::resample::{bessel_i0, sinc};

/// Taps each side of a place.
const HALF: usize = 16;
const WIDTH: usize = 2 * HALF;
/// Rows of the table between two samples.
const PHASES: usize = 256;
/// The Kaiser window, about 80 dB down outside its main lobe.
const BETA: f64 = 8.0;

/// The filter that reads a file at any place. There is one, [`varispeed`].
pub struct Varispeed {
    /// `PHASES + 1` rows of `WIDTH` taps, each row summing to 1. Row 0 is a single 1.
    table: Box<[f32]>,
}

static VARISPEED: LazyLock<Varispeed> = LazyLock::new(Varispeed::new);

/// The one filter. The first call makes its table, so make it on the control side, such as when
/// a processor is made, and never first on the audio thread.
pub fn varispeed() -> &'static Varispeed {
    &VARISPEED
}

impl Varispeed {
    fn new() -> Self {
        let normal = bessel_i0(BETA);
        let mut table = vec![0.0_f32; (PHASES + 1) * WIDTH];
        let mut values = [0.0_f64; WIDTH];
        for (phase, row) in table.chunks_exact_mut(WIDTH).enumerate() {
            let fraction = phase as f64 / PHASES as f64;
            for (tap, value) in values.iter_mut().enumerate() {
                // The distance of this tap's sample from the place, in frames of the file.
                let distance = (tap as f64 - HALF as f64 + 1.0) - fraction;
                let edge = distance / HALF as f64;
                let window = match edge.abs() < 1.0 {
                    true => bessel_i0(BETA * (1.0 - edge * edge).sqrt()) / normal,
                    false => 0.0,
                };
                // At a whole distance the sinc is 0, but `sin` does not say so exactly. A
                // whole place must give its sample and nothing of its neighbours.
                let whole = distance == distance.round() && distance != 0.0;
                *value = match whole {
                    true => 0.0,
                    false => sinc(distance) * window,
                };
            }
            // A steady level comes out at exactly that level.
            let sum: f64 = values.iter().sum();
            for (cell, value) in row.iter_mut().zip(&values) {
                *cell = (value / sum) as f32;
            }
        }
        Self {
            table: table.into_boxed_slice(),
        }
    }

    /// The file at `position`, `position + step`, `position + 2 * step`, ... into `out`, in
    /// frames of the file. `scratch` holds file frames on the way and is at least
    /// [`SCRATCH_FRAMES`](crate::SCRATCH_FRAMES) long. A place outside the file is silence.
    ///
    /// Realtime safe: no allocation, lock or system call.
    pub fn render(
        &self,
        audio: &Audio,
        position: f64,
        step: f64,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        // Output frames per round, so that the file frames they read fit the scratch.
        let room = scratch.len().saturating_sub(WIDTH + 2) as f64;
        let per_round = (room / step.abs().max(f64::MIN_POSITIVE)).floor();
        let per_round = (per_round.min(out.len() as f64) as usize).max(1);
        let mut done = 0;
        for chunk in out.chunks_mut(per_round) {
            let first = position + step * done as f64;
            let last = first + step * (chunk.len() - 1) as f64;
            let window = first.min(last).floor() as i64 - HALF as i64 + 1;
            let length = (first.max(last).floor() as i64 + HALF as i64 + 1 - window)
                .clamp(0, scratch.len() as i64) as usize;
            let Some(input) = scratch.get_mut(..length) else {
                chunk.fill([0.0; 2]);
                return;
            };
            audio.read(window, input);
            let input = &*input;
            for (index, frame) in chunk.iter_mut().enumerate() {
                let at = first + step * index as f64;
                let whole = at.floor();
                let phase = (at - whole) * PHASES as f64;
                let row = (phase as usize).min(PHASES - 1);
                let between = (phase - row as f64) as f32;
                let first_tap = (whole as i64 - HALF as i64 + 1 - window) as usize;
                let lower = self.table.get(row * WIDTH..(row + 1) * WIDTH);
                let upper = self.table.get((row + 1) * WIDTH..(row + 2) * WIDTH);
                let samples = input.get(first_tap..first_tap + WIDTH);
                let (Some(lower), Some(upper), Some(samples)) = (lower, upper, samples) else {
                    *frame = [0.0; 2];
                    continue;
                };
                let mut sum = [0.0_f32; 2];
                for ((sample, lower), upper) in samples.iter().zip(lower).zip(upper) {
                    let weight = lower + between * (upper - lower);
                    sum[0] += sample[0] * weight;
                    sum[1] += sample[1] * weight;
                }
                *frame = sum;
            }
            done += chunk.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_passes_a_steady_level_and_the_first_is_the_sample_itself() {
        let varispeed = Varispeed::new();
        for row in varispeed.table.chunks_exact(WIDTH) {
            let sum: f32 = row.iter().sum();
            assert!((sum - 1.0).abs() < 1e-5, "{sum}");
        }
        let first = &varispeed.table[..WIDTH];
        for (tap, weight) in first.iter().enumerate() {
            let expected = if tap == HALF - 1 { 1.0 } else { 0.0 };
            assert_eq!(*weight, expected, "tap {tap}");
        }
    }
}
