//! Plays a file at any speed: what an instrument needs that plays a sample at the pitch of a
//! key, such as the Sampler and the Drum pad.
//!
//! [`Resampler`](crate::Resampler) is exact for one pair of sample rates, a fraction worked out
//! in integers. A key plays at a ratio that is no fraction, `2^(semitones / 12)` times the ratio
//! of the rates, so this reads the file at any place `position + n * step`, in `f64`. It keeps
//! no state, like the resampler: the caller keeps the place and moves it on by `step` per frame.
//!
//! The filter is one windowed sinc (Kaiser, beta 8) of 32 taps at a step up to 1, from a table
//! of 256 phases with a straight line between two. There it is an interpolating filter: its
//! cutoff is the Nyquist frequency of the file, so at a whole place it gives the sample of the
//! file exactly, and a file played at its own rate and speed comes out sample for sample.
//!
//! Above a step of 1 the file goes by faster than the engine plays it, and what the file holds
//! above the Nyquist frequency of the engine would fold back. So the same kernel is stretched by
//! `step / 0.85`, read from a fine table of it at `distance / stretch`, with as many more taps
//! and the weights of each frame summed to 1: its cutoff follows the output, flat to about
//! 17 kHz at 48 kHz and down by the stop band of the window from the output's Nyquist frequency
//! on. The stretch stops at a step of 8 (`MAX_STEP`), 302 taps; above it the part of the file
//! above `8 / step` of the output's Nyquist frequency folds back.

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
/// Above a step of 1, where the stretched kernel puts its cutoff, as a part of the output's
/// Nyquist frequency: the transition of the window then ends under the Nyquist frequency, so
/// nothing that would fold back gets through.
const CUTOFF: f64 = 0.85;
/// The largest step the kernel stretches for. Its cost grows with the step: 302 taps here.
pub const MAX_STEP: f64 = 8.0;

/// The filter that reads a file at any place. There is one, [`varispeed`].
pub struct Varispeed {
    /// `PHASES + 1` rows of `WIDTH` taps, each row summing to 1. Row 0 is a single 1.
    table: Box<[f32]>,
    /// The kernel itself at every `1 / PHASES` from `-HALF` to `HALF`, for the stretched one.
    curve: Box<[f32]>,
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
        let curve = (0..=WIDTH * PHASES).map(|index| {
            let distance = index as f64 / PHASES as f64 - HALF as f64;
            let edge = distance / HALF as f64;
            let window = match edge.abs() < 1.0 {
                true => bessel_i0(BETA * (1.0 - edge * edge).sqrt()) / normal,
                false => 0.0,
            };
            (sinc(distance) * window) as f32
        });
        Self {
            table: table.into_boxed_slice(),
            curve: curve.collect(),
        }
    }

    /// The kernel at `distance` in its own units, between two places of the fine table.
    fn kernel(&self, distance: f32) -> f32 {
        let place = (distance + HALF as f32) * PHASES as f32;
        // Before the kernel, and a place that is not a number, weigh nothing.
        if place.is_nan() || place <= 0.0 {
            return 0.0;
        }
        let index = place as usize;
        let between = place - index as f32;
        match (self.curve.get(index), self.curve.get(index + 1)) {
            (Some(lower), Some(upper)) => lower + between * (upper - lower),
            _ => 0.0,
        }
    }

    /// The file at `position`, `position + step`, `position + 2 * step`, ... into `out`, in
    /// frames of the file. `scratch` holds file frames on the way and is at least
    /// [`SCRATCH_FRAMES`](crate::SCRATCH_FRAMES) long. A place outside the file is silence.
    ///
    /// Realtime safe: no allocation, lock or system call. Above a step of 1 a frame costs
    /// about `2 * ceil(16 * step / 0.85)` taps, up to 302 at a step of 8.
    pub fn render(
        &self,
        audio: &Audio,
        position: f64,
        step: f64,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        if step > 1.0 {
            return self.render_stretched(audio, position, step, out, scratch);
        }
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

    /// A step above 1: the kernel stretched so its cutoff follows the output, with the weights
    /// of each frame summed to 1.
    fn render_stretched(
        &self,
        audio: &Audio,
        position: f64,
        step: f64,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        let stretch = step.min(MAX_STEP) / CUTOFF;
        let inverse = (1.0 / stretch) as f32;
        // Taps each side of a place, in frames of the file.
        let reach = (HALF as f64 * stretch).ceil() as i64;
        let width = 2 * reach as usize;
        let room = scratch.len().saturating_sub(width + 2) as f64;
        let per_round = (room / step).floor();
        let per_round = (per_round.min(out.len() as f64) as usize).max(1);
        let mut done = 0;
        for chunk in out.chunks_mut(per_round) {
            let first = position + step * done as f64;
            let last = first + step * (chunk.len() - 1) as f64;
            let window = first.floor() as i64 - reach + 1;
            let length = (last.floor() as i64 + reach + 1 - window).clamp(0, scratch.len() as i64);
            let Some(input) = scratch.get_mut(..length as usize) else {
                chunk.fill([0.0; 2]);
                return;
            };
            audio.read(window, input);
            let input = &*input;
            for (index, frame) in chunk.iter_mut().enumerate() {
                let at = first + step * index as f64;
                let whole = at.floor();
                let fraction = (at - whole) as f32;
                let first_tap = (whole as i64 - reach + 1 - window) as usize;
                let Some(samples) = input.get(first_tap..first_tap + width) else {
                    *frame = [0.0; 2];
                    continue;
                };
                let (mut sum, mut total) = ([0.0_f32; 2], 0.0_f32);
                for (tap, sample) in samples.iter().enumerate() {
                    let distance = (tap as i64 - reach + 1) as f32 - fraction;
                    let weight = self.kernel(distance * inverse);
                    sum[0] += sample[0] * weight;
                    sum[1] += sample[1] * weight;
                    total += weight;
                }
                *frame = match total > 0.0 {
                    true => [sum[0] / total, sum[1] / total],
                    false => [0.0; 2],
                };
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
