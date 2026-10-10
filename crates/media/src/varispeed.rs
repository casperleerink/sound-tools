//! Plays a file at any speed: what an instrument needs that plays a sample at the pitch of a
//! key, such as the Sampler.
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
use crate::resample::{SincTable, bessel_i0, kaiser, sinc};

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
    /// Row 0 is a single 1.
    table: SincTable,
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
        let curve = (0..=WIDTH * PHASES).map(|index| {
            let distance = index as f64 / PHASES as f64 - HALF as f64;
            (sinc(distance) * kaiser(distance / HALF as f64, BETA, normal)) as f32
        });
        Self {
            table: SincTable::new(HALF, PHASES, 1.0, BETA),
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
            let first_tap = |at: f64| (at.floor() as i64 - HALF as i64 + 1 - window) as usize;
            // At a step of 1 from a whole place, such as a key at its root, every frame is on a
            // sample, where the first row of the table is a single 1: the sum is that sample
            // and products with 0. Adding 0 turns -0 into 0 as that sum does. A sample that is
            // not a number spreads to its neighbours there, so then the filter runs.
            if step == 1.0
                && first.fract() == 0.0
                && input.iter().flatten().all(|sample| sample.is_finite())
            {
                for (index, frame) in chunk.iter_mut().enumerate() {
                    let first_tap = first_tap(first + index as f64);
                    let taps = input.get(first_tap..first_tap + WIDTH);
                    *frame = match taps.and_then(|taps| taps.get(HALF - 1)) {
                        Some(sample) => sample.map(|sample| sample + 0.0),
                        None => [0.0; 2],
                    };
                }
                done += chunk.len();
                continue;
            }
            let place = |index: usize| {
                let at = first + step * index as f64;
                let (lower, upper, between) = self.table.rows_at(at - at.floor())?;
                let first_tap = first_tap(at);
                Some(Taps {
                    lower: lower.try_into().ok()?,
                    upper: upper.try_into().ok()?,
                    samples: input.get(first_tap..first_tap + WIDTH)?.try_into().ok()?,
                    between,
                })
            };
            side_by_side(chunk, place, Taps::sum, |taps| Taps::sum([taps])[0]);
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
            // The samples of a frame, when they fit, and how far past a whole place it is.
            let place = |index: usize| {
                let at = first + step * index as f64;
                let whole = at.floor();
                let first_tap = (whole as i64 - reach + 1 - window) as usize;
                let samples = input.get(first_tap..first_tap + width)?;
                Some((samples, (at - whole) as f32))
            };
            let stretched = Stretched { reach, inverse };
            let frames = |places| stretched.frames(self, places);
            side_by_side(chunk, place, frames, |place| {
                stretched.frames(self, [place])[0]
            });
            done += chunk.len();
        }
    }
}

/// Frames worked out side by side. Each sum adds its taps in the same order as alone, so the
/// bits are the same, and the processor adds a tap of one frame while the sum of another waits
/// for its last add.
const SIDE_BY_SIDE: usize = 4;

/// Fills `out` with the frame of each place, `SIDE_BY_SIDE` at a time where all of them fit,
/// and silence where one does not.
#[inline]
fn side_by_side<T: Copy>(
    out: &mut [[f32; 2]],
    place: impl Fn(usize) -> Option<T>,
    frames: impl Fn([T; SIDE_BY_SIDE]) -> [[f32; 2]; SIDE_BY_SIDE],
    frame: impl Fn(T) -> [f32; 2],
) {
    let mut groups = out.chunks_exact_mut(SIDE_BY_SIDE);
    let mut index = 0;
    for group in &mut groups {
        let places: [_; SIDE_BY_SIDE] = std::array::from_fn(|at| place(index + at));
        if let [Some(first), ..] = places
            && places.iter().all(Option::is_some)
        {
            group.copy_from_slice(&frames(places.map(|place| place.unwrap_or(first))));
        } else {
            for (out, place) in group.iter_mut().zip(places) {
                *out = place.map_or([0.0; 2], &frame);
            }
        }
        index += SIDE_BY_SIDE;
    }
    for out in groups.into_remainder() {
        *out = place(index).map_or([0.0; 2], &frame);
        index += 1;
    }
}

/// What one frame at a step up to 1 reads: two rows of the table, how far between them, and
/// its samples.
#[derive(Copy, Clone)]
struct Taps<'a> {
    lower: &'a [f32; WIDTH],
    upper: &'a [f32; WIDTH],
    samples: &'a [[f32; 2]; WIDTH],
    between: f32,
}

impl Taps<'_> {
    /// Frames side by side, each summed in the order of [`SincTable::apply`].
    #[inline]
    fn sum<const N: usize>(taps: [Self; N]) -> [[f32; 2]; N] {
        let mut sums = [[0.0_f32; 2]; N];
        for tap in 0..WIDTH {
            for (sum, taps) in sums.iter_mut().zip(&taps) {
                let (lower, upper) = (taps.lower[tap], taps.upper[tap]);
                let weight = lower + taps.between * (upper - lower);
                sum[0] += taps.samples[tap][0] * weight;
                sum[1] += taps.samples[tap][1] * weight;
            }
        }
        sums
    }
}

/// The kernel stretched for a step above 1.
#[derive(Copy, Clone)]
struct Stretched {
    /// Taps each side of a place, in frames of the file.
    reach: i64,
    /// One over the stretch.
    inverse: f32,
}

impl Stretched {
    /// Frames side by side, each from its samples, all as many, and how far past a whole place
    /// it is. The weights of each frame sum to 1.
    #[inline]
    fn frames<const N: usize>(
        self,
        varispeed: &Varispeed,
        places: [(&[[f32; 2]], f32); N],
    ) -> [[f32; 2]; N] {
        let width = places.first().map_or(0, |(samples, _)| samples.len());
        let mut sums = [[0.0_f32; 3]; N];
        for tap in 0..width {
            let distance = (tap as i64 - self.reach + 1) as f32;
            for (sum, (samples, fraction)) in sums.iter_mut().zip(places) {
                let Some(sample) = samples.get(tap) else {
                    continue;
                };
                let weight = varispeed.kernel((distance - fraction) * self.inverse);
                sum[0] += sample[0] * weight;
                sum[1] += sample[1] * weight;
                sum[2] += weight;
            }
        }
        sums.map(|[left, right, total]| match total > 0.0 {
            true => [left / total, right / total],
            false => [0.0; 2],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_passes_a_steady_level_and_the_first_is_the_sample_itself() {
        let varispeed = Varispeed::new();
        for row in varispeed.table.rows.chunks_exact(WIDTH) {
            let sum: f32 = row.iter().sum();
            assert!((sum - 1.0).abs() < 1e-5, "{sum}");
        }
        let first = &varispeed.table.rows[..WIDTH];
        for (tap, weight) in first.iter().enumerate() {
            let expected = if tap == HALF - 1 { 1.0 } else { 0.0 };
            assert_eq!(*weight, expected, "tap {tap}");
        }
    }

    /// Frames side by side, and a step of 1 copied from the file, give the bits of the filter
    /// on each frame alone, also around -0, a number under the smallest normal one and one that
    /// is not a number.
    #[test]
    fn every_frame_is_the_filter_of_its_own_taps_alone() {
        let mut samples: Vec<f32> = (0..400).map(|index| (index as f32 * 0.37).sin()).collect();
        samples[60] = -0.0;
        samples[61] = 1e-40;
        samples[230] = f32::NAN;
        let mut bytes = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
        for sample in samples {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
        let audio = Audio::parse(bytes.into_inner()).unwrap();
        let varispeed = Varispeed::new();
        let mut scratch = vec![[0.0; 2]; crate::SCRATCH_FRAMES];
        for step in [0.7, 1.0, 1.6] {
            for position in [-20.0, 50.0, 50.25, 200.0] {
                let mut out = [[0.0_f32; 2]; 64];
                varispeed.render(&audio, position, step, &mut out, &mut scratch);
                for (index, frame) in out.iter().enumerate() {
                    let at = position + step * index as f64;
                    let whole = at.floor();
                    let alone = if step > 1.0 {
                        let stretch = step / CUTOFF;
                        let reach = (HALF as f64 * stretch).ceil() as i64;
                        let mut taps = vec![[0.0; 2]; 2 * reach as usize];
                        audio.read(whole as i64 - reach + 1, &mut taps);
                        let inverse = (1.0 / stretch) as f32;
                        let fraction = (at - whole) as f32;
                        Stretched { reach, inverse }.frames(&varispeed, [(&taps, fraction)])[0]
                    } else {
                        let mut taps = [[0.0; 2]; WIDTH];
                        audio.read(whole as i64 - HALF as i64 + 1, &mut taps);
                        varispeed.table.apply(&taps, 0, at - whole)
                    };
                    assert_eq!(
                        frame.map(f32::to_bits),
                        alone.map(f32::to_bits),
                        "step {step} from {position}, frame {index}"
                    );
                }
            }
        }
    }
}
