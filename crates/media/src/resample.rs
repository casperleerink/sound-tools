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
//! the control side, with a straight line between two phases. It is flat up to 20 kHz, or to
//! 95 % of the lower Nyquist frequency of the two rates when that is lower, and it stops from
//! that Nyquist frequency on, so nothing above it folds back.

use crate::file::Audio;

/// Where the pass band ends at most: 20 kHz and a little room, so 20 kHz itself is flat.
const PASS_HZ: f64 = 20_500.0;
/// The pass band as a part of the lower Nyquist frequency, for rates too low for `PASS_HZ`.
const PASS_PART: f64 = 0.95;
/// How far the stop band is down.
const STOP_DB: f64 = 100.0;
/// Rows of the table between two samples when the file is not above the engine's rate. Going
/// down in rate stretches the filter, which then needs fewer.
const PHASES: usize = 1024;

/// The file frames one call of [`Resampler::render`] may read at once. Enough for a block of
/// the widest filter this makes, from 384 kHz down to 8 kHz.
pub const SCRATCH_FRAMES: usize = 16_384;

/// The filter for one pair of rates.
#[derive(Debug)]
pub struct Resampler {
    /// File frames per engine frame, as a fraction in lowest terms.
    step: (u64, u64),
    table: SincTable,
}

impl Resampler {
    /// The filter that plays a file at `file_rate` in an engine at `engine_rate`.
    pub fn new(file_rate: u32, engine_rate: u32) -> Self {
        let (file, engine) = (u64::from(file_rate.max(1)), u64::from(engine_rate.max(1)));
        let divisor = greatest_common_divisor(file, engine);
        let step = (file / divisor, engine / divisor);
        if step.0 == step.1 {
            return Self {
                step,
                table: SincTable::default(),
            };
        }
        // Kaiser's design, at the lower of the two rates.
        let lower = file.min(engine) as f64;
        let nyquist = lower / 2.0;
        let pass = PASS_HZ.min(PASS_PART * nyquist);
        let transition = (nyquist - pass) / lower;
        let taps = (STOP_DB - 7.95) / (2.285 * std::f64::consts::TAU * transition);
        // In frames of the file: going down in rate, the filter is as many times wider.
        let stretch = file as f64 / lower;
        let half = (taps / 2.0 * stretch).ceil() as usize;
        let phases = ((PHASES as f64 / stretch).ceil() as usize).max(16);
        // The middle of the transition, relative to the Nyquist frequency of the file.
        let cutoff = (pass + nyquist) / 2.0 / (file as f64 / 2.0);
        let beta = 0.1102 * (STOP_DB - 8.7);
        Self {
            step,
            table: SincTable::new(half, phases, cutoff, beta),
        }
    }

    fn is_identity(&self) -> bool {
        self.step.0 == self.step.1
    }

    /// Where engine frame `frame` of a stream from `origin` is in the file: the file frame
    /// before it, and how far past that frame, from 0 to 1.
    fn position(&self, origin: u64, frame: u64) -> (u64, f64) {
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
        if self.is_identity() {
            let start = origin.saturating_add(first);
            audio.read(i64::try_from(start).unwrap_or(i64::MAX), out);
            return;
        }
        let (file, engine) = self.step;
        let half = self.table.half;
        // Output frames per round, so their input fits the scratch.
        let room = scratch.len().saturating_sub(2 * half + 1) as u128;
        let per_round = (room * u128::from(engine) / u128::from(file)).max(1);
        let per_round = usize::try_from(per_round).unwrap_or(usize::MAX);
        let mut done = 0_u64;
        for chunk in out.chunks_mut(per_round) {
            let start = first + done;
            let end = start + chunk.len() as u64 - 1;
            let window = self.position(origin, start).0 as i64 - half as i64 + 1;
            let length = (self.position(origin, end).0 as i64 + half as i64 + 1 - window)
                .clamp(0, scratch.len() as i64) as usize;
            let Some(input) = scratch.get_mut(..length) else {
                chunk.fill([0.0; 2]);
                return;
            };
            audio.read(window, input);
            let input = &*input;
            for (index, frame) in chunk.iter_mut().enumerate() {
                let (sample, fraction) = self.position(origin, start + index as u64);
                let first_tap = (sample as i64 - half as i64 + 1 - window) as usize;
                *frame = self.table.apply(input, first_tap, fraction);
            }
            done += chunk.len() as u64;
        }
    }
}

/// A windowed sinc (Kaiser window) from a table of phases, with a straight line between two
/// phases. The filter of [`Resampler`] and of [`Varispeed`](crate::Varispeed).
#[derive(Debug, Default)]
pub(crate) struct SincTable {
    /// Taps each side of a place.
    pub(crate) half: usize,
    pub(crate) phases: usize,
    /// `phases + 1` rows of `2 * half` taps, each row summing to 1.
    pub(crate) rows: Box<[f32]>,
}

impl SincTable {
    /// `cutoff` is a part of the Nyquist frequency of the file, and `beta` the shape of the
    /// window.
    pub(crate) fn new(half: usize, phases: usize, cutoff: f64, beta: f64) -> Self {
        let normal = bessel_i0(beta);
        let width = 2 * half;
        let mut rows = vec![0.0_f32; (phases + 1) * width];
        let mut values = vec![0.0_f64; width];
        for (phase, row) in rows.chunks_exact_mut(width).enumerate() {
            let fraction = phase as f64 / phases as f64;
            for (tap, value) in values.iter_mut().enumerate() {
                // The distance of this tap's sample from the place, in frames of the file.
                let distance = (tap as f64 - half as f64 + 1.0) - fraction;
                let window = kaiser(distance / half as f64, beta, normal);
                *value = cutoff * sinc(cutoff * distance) * window;
            }
            // A steady level comes out at exactly that level.
            let sum: f64 = values.iter().sum();
            for (cell, value) in row.iter_mut().zip(&values) {
                *cell = (value / sum) as f32;
            }
        }
        Self {
            half,
            phases,
            rows: rows.into_boxed_slice(),
        }
    }

    /// The two rows around `fraction` (0 to 1), and how far it is from the first to the second.
    #[inline]
    pub(crate) fn rows_at(&self, fraction: f64) -> Option<(&[f32], &[f32], f32)> {
        let width = 2 * self.half;
        let phase = fraction * self.phases as f64;
        let row = (phase as usize).min(self.phases.saturating_sub(1));
        let between = (phase - row as f64) as f32;
        let lower = self.rows.get(row * width..(row + 1) * width)?;
        let upper = self.rows.get((row + 1) * width..(row + 2) * width)?;
        Some((lower, upper, between))
    }

    /// The filtered frame `fraction` (0 to 1) past the frame `input[first_tap + half - 1]`.
    /// Silence when its taps do not fit in `input`.
    #[inline]
    pub(crate) fn apply(&self, input: &[[f32; 2]], first_tap: usize, fraction: f64) -> [f32; 2] {
        let width = 2 * self.half;
        let samples = input.get(first_tap..first_tap + width);
        let (Some((lower, upper, between)), Some(samples)) = (self.rows_at(fraction), samples)
        else {
            return [0.0; 2];
        };
        let mut sum = [0.0_f32; 2];
        for ((sample, lower), upper) in samples.iter().zip(lower).zip(upper) {
            let weight = lower + between * (upper - lower);
            sum[0] += sample[0] * weight;
            sum[1] += sample[1] * weight;
        }
        sum
    }
}

/// `sin(πx) / (πx)`: exactly 1 at 0, and exactly 0 at every other whole `x`, where `sin` alone
/// is a little off. So a filter with its cutoff at the Nyquist frequency gives the sample itself
/// at a whole place, and nothing of its neighbours.
pub(crate) fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        return 1.0;
    }
    if x == x.round() {
        return 0.0;
    }
    let angle = std::f64::consts::PI * x;
    angle.sin() / angle
}

/// The Kaiser window at `edge`, from -1 to 1 across it. `normal` is `bessel_i0(beta)`.
pub(crate) fn kaiser(edge: f64, beta: f64, normal: f64) -> f64 {
    match edge.abs() < 1.0 {
        true => bessel_i0(beta * (1.0 - edge * edge).sqrt()) / normal,
        false => 0.0,
    }
}

/// The modified Bessel function of the first kind, order 0, for the Kaiser window.
pub(crate) fn bessel_i0(x: f64) -> f64 {
    let quarter = x * x / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..100 {
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
    fn the_same_rate_is_the_identity_and_positions_are_exact() {
        assert!(Resampler::new(48_000, 48_000).is_identity());
        let up = Resampler::new(44_100, 48_000);
        assert_eq!(up.step, (147, 160));
        assert_eq!(up.position(10, 160), (157, 0.0));
        assert_eq!(up.position(0, 1), (0, 147.0 / 160.0));
    }

    #[test]
    fn every_row_of_the_filter_passes_a_steady_level_unchanged_and_fits_the_scratch() {
        for (file, engine) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 48_000),
            (384_000, 8_000),
        ] {
            let resampler = Resampler::new(file, engine);
            let width = 2 * resampler.table.half;
            let block = sound_core::MAX_BLOCK * (file / engine + 1) as usize;
            assert!(
                width + block < SCRATCH_FRAMES,
                "{file} to {engine}: {width} taps"
            );
            println!(
                "{file} Hz to {engine} Hz: {width} taps, {} phases",
                resampler.table.phases
            );
            for row in resampler.table.rows.chunks_exact(width) {
                let sum: f32 = row.iter().sum();
                assert!((sum - 1.0).abs() < 1e-5, "{sum}");
            }
        }
    }
}
