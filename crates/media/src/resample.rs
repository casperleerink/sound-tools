//! Plays a file at another sample rate than the engine, at the right pitch and speed.
//!
//! Stateless: output frame `n` of a stream that starts at file frame `origin` reads the file
//! around `origin + n * file_rate / engine_rate`, worked out exactly in integers. So any frame
//! can be rendered on its own, from any place in the file, and a render gives the same bytes
//! every time. This is why the step does not use a block resampler such as `rubato`: those
//! keep a filter state that must be run in from the start, and a clip is heard from wherever
//! the playhead is.
//!
//! The filter is a windowed sinc (Kaiser window), from a table of phases worked out once on
//! the control side, with a straight line between two phases.

use crate::file::Audio;

/// Taps each side of a position at the rate of the file, when the file is not above the
/// engine's rate. Going down in rate widens the filter by the ratio, to keep out aliasing.
const HALF_TAPS: usize = 16;
/// The pass band, as a part of the lower of the two Nyquist frequencies.
const CUTOFF: f64 = 0.95;
/// Kaiser window shape: about 80 dB of stop band.
const BETA: f64 = 8.0;
/// Rows of the table between two samples.
const PHASES: usize = 256;

/// The file frames one call of [`Resampler::render`] may read at once. Enough for a block of
/// the widest filter this makes, from 384 kHz down to 8 kHz: a filter of about 1600 taps.
pub const SCRATCH_FRAMES: usize = 4096;

/// The filter for one pair of rates.
#[derive(Debug)]
pub struct Resampler {
    /// File frames per engine frame, as a fraction in lowest terms.
    step: (u64, u64),
    /// Taps each side of a position.
    half: usize,
    /// `PHASES + 1` rows of `2 * half` taps, each row summing to 1.
    table: Box<[f32]>,
}

impl Resampler {
    /// The filter that plays a file at `file_rate` in an engine at `engine_rate`.
    pub fn new(file_rate: u32, engine_rate: u32) -> Self {
        let (file_rate, engine_rate) = (u64::from(file_rate.max(1)), u64::from(engine_rate.max(1)));
        let divisor = greatest_common_divisor(file_rate, engine_rate);
        let step = (file_rate / divisor, engine_rate / divisor);
        // Relative to the Nyquist frequency of the file.
        let cutoff = CUTOFF * (engine_rate as f64 / file_rate as f64).min(1.0);
        let half = (HALF_TAPS as f64 * CUTOFF / cutoff).ceil() as usize;
        let taps = 2 * half;
        let mut table = vec![0.0_f32; (PHASES + 1) * taps];
        let normal = bessel_i0(BETA);
        for (phase, row) in table.chunks_exact_mut(taps).enumerate() {
            let fraction = phase as f64 / PHASES as f64;
            let mut values = vec![0.0_f64; taps];
            for (tap, value) in values.iter_mut().enumerate() {
                // The distance of this tap's sample from the position, in file frames.
                let distance = (tap as f64 - half as f64 + 1.0) - fraction;
                let edge = distance / half as f64;
                let window = match edge.abs() < 1.0 {
                    true => bessel_i0(BETA * (1.0 - edge * edge).sqrt()) / normal,
                    false => 0.0,
                };
                *value = cutoff * sinc(cutoff * distance) * window;
            }
            // A steady level comes out at exactly that level.
            let sum: f64 = values.iter().sum();
            for (cell, value) in row.iter_mut().zip(values) {
                *cell = (value / sum) as f32;
            }
        }
        Self {
            step,
            half,
            table: table.into_boxed_slice(),
        }
    }

    /// Whether the two rates are the same, so a file frame is an engine frame.
    pub fn is_identity(&self) -> bool {
        self.step.0 == self.step.1
    }

    /// File frames per engine frame, as a fraction in lowest terms.
    pub fn step(&self) -> (u64, u64) {
        self.step
    }

    /// How many engine frames a stretch of `file_frames` plays: the frames whose position is
    /// still inside it.
    pub fn engine_frames(&self, file_frames: u64) -> u64 {
        let (file, engine) = self.step;
        let frames = u128::from(file_frames) * u128::from(engine);
        u64::try_from(frames.div_ceil(u128::from(file))).unwrap_or(u64::MAX)
    }

    /// Where engine frame `frame` of a stream from `origin` is in the file: the file frame
    /// before it, and how far past that frame, from 0 to 1.
    pub fn position(&self, origin: u64, frame: u64) -> (u64, f64) {
        let (file, engine) = self.step;
        let travelled = u128::from(frame) * u128::from(file);
        let whole = u64::try_from(travelled / u128::from(engine)).unwrap_or(u64::MAX);
        let rest = (travelled % u128::from(engine)) as f64 / engine as f64;
        (origin.saturating_add(whole), rest)
    }

    /// Engine frames `first..first + out.len()` of a stream that starts at file frame
    /// `origin`, into `out`. `scratch` holds file frames on the way and is at least
    /// [`SCRATCH_FRAMES`] long.
    ///
    /// Realtime safe: no allocation, lock or system call.
    pub fn render(
        &self,
        audio: &Audio,
        origin: u64,
        first: u64,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        let (file, engine) = self.step;
        if file == engine {
            let start = origin.saturating_add(first);
            audio.read(i64::try_from(start).unwrap_or(i64::MAX), out);
            return;
        }
        let taps = 2 * self.half;
        // Output frames per round, so their input fits the scratch.
        let room = scratch.len().saturating_sub(taps + 1) as u128;
        let per_round = (room * u128::from(engine) / u128::from(file)).max(1);
        let per_round = usize::try_from(per_round).unwrap_or(usize::MAX);
        let mut done = 0_u64;
        for chunk in out.chunks_mut(per_round) {
            let start = first + done;
            let end = start + chunk.len() as u64 - 1;
            let window = self.position(origin, start).0 as i64 - self.half as i64 + 1;
            let length = (self.position(origin, end).0 as i64 + self.half as i64 + 1 - window)
                .clamp(0, scratch.len() as i64) as usize;
            let Some(input) = scratch.get_mut(..length) else {
                chunk.fill([0.0; 2]);
                return;
            };
            audio.read(window, input);
            let input = &*input;
            for (index, frame) in chunk.iter_mut().enumerate() {
                let (sample, fraction) = self.position(origin, start + index as u64);
                let phase = fraction * PHASES as f64;
                let row = (phase as usize).min(PHASES - 1);
                let between = (phase - row as f64) as f32;
                let first_tap = (sample as i64 - self.half as i64 + 1 - window) as usize;
                let lower = self.table.get(row * taps..(row + 1) * taps);
                let upper = self.table.get((row + 1) * taps..(row + 2) * taps);
                let samples = input.get(first_tap..first_tap + taps);
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
            done += chunk.len() as u64;
        }
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        return 1.0;
    }
    let angle = std::f64::consts::PI * x;
    angle.sin() / angle
}

/// The modified Bessel function of the first kind, order 0, for the Kaiser window.
fn bessel_i0(x: f64) -> f64 {
    let quarter = x * x / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..64 {
        term *= quarter / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

fn greatest_common_divisor(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_rate_is_the_identity_and_lengths_follow_the_ratio() {
        assert!(Resampler::new(48_000, 48_000).is_identity());
        let up = Resampler::new(44_100, 48_000);
        assert_eq!(up.step(), (147, 160));
        assert_eq!(up.engine_frames(44_100), 48_000);
        assert_eq!(up.engine_frames(1), 2);
        let down = Resampler::new(96_000, 48_000);
        assert_eq!(down.engine_frames(96_000), 48_000);
        assert_eq!(down.engine_frames(3), 2);
        assert_eq!(up.position(10, 160), (157, 0.0));
    }

    #[test]
    fn every_row_of_the_filter_passes_a_steady_level_unchanged() {
        for (file, engine) in [(44_100, 48_000), (96_000, 48_000), (384_000, 8_000)] {
            let resampler = Resampler::new(file, engine);
            let taps = 2 * resampler.half;
            assert!(taps + 1 < SCRATCH_FRAMES, "{file} to {engine}: {taps} taps");
            for row in resampler.table.chunks_exact(taps) {
                let sum: f32 = row.iter().sum();
                assert!((sum - 1.0).abs() < 1e-5, "{sum}");
            }
        }
    }
}
