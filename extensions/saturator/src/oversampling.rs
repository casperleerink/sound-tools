//! Oversampling by four, in two stages of two, so that the harmonics the curve makes above the
//! top of the hearing range are taken away before they fold back into it.
//!
//! Each stage is a half band filter: a linear phase FIR filter whose cutoff is a quarter of its
//! own rate, a windowed sinc (Kaiser window). Every other tap of it is zero and the middle one
//! is one half, so a stage of two works out only half of its taps for each sample it makes. A
//! linear phase filter delays every frequency alike, so the saturated sound stays in time with
//! the dry sound it is mixed with, and the delay is reported as the latency of the saturator.
//!
//! The first stage does the hard part: it keeps everything up to 0.45 of the sample rate and
//! takes 88 dB off from 0.55 of it, so what the curve makes above there cannot fold back under
//! 0.45. The second stage, at twice the rate, has a wide band to fall in and is short.

use std::f64::consts::PI;

/// The middle tap of the first stage, from its start. The filter is twice this plus one long,
/// and delays by this many samples of its rate. Odd, so the taps at its ends are not zero.
const FIRST_MIDDLE: usize = 57;
/// The same for the second stage.
const SECOND_MIDDLE: usize = 13;

/// The taps a stage works out per sample it makes, every other tap and the one at the end,
/// with zeros in front up to a multiple of eight: see [`dot`].
const FIRST_TAPS: usize = 64;
const SECOND_TAPS: usize = 16;

/// How many frames the saturated sound comes out after the sound that went in: each stage
/// delays by its middle twice, once on the way up and once on the way down, in samples of its
/// rate. The second stage gets one sample more at twice the rate, so the whole is whole frames.
pub const DELAY_FRAMES: usize = FIRST_MIDDLE + SECOND_MIDDLE.div_ceil(2);

/// The Kaiser window for 88 dB of stop band. With the lengths above, the first stage falls
/// from 0.45 to 0.55 of the rate it goes down to. The second, at four times that rate, keeps
/// what the first lets through, under 0.1375 of its rate, and takes 88 dB off the images
/// of the band under 0.45 of the sample rate, from 0.3875 of its rate up.
const BETA: f64 = 8.96;

/// A half band filter: a windowed sinc whose cutoff is a quarter of its rate. Its middle tap is
/// one half and every other tap from there is zero, so only the taps at odd distances from the
/// middle are kept, at the end of `taps`. `MIDDLE` is the place of the middle tap.
#[derive(Clone, Copy)]
struct HalfBand<const MIDDLE: usize, const TAPS: usize> {
    taps: [f32; TAPS],
}

impl<const MIDDLE: usize, const TAPS: usize> HalfBand<MIDDLE, TAPS> {
    /// The filter is symmetric, so the order of the taps does not matter. Their sum is exactly
    /// one half, so the gain at 0 Hz is exactly 1.
    fn new() -> Self {
        // What the places of the plain and the middle samples in `Up` and `Down` rely on, and
        // `dot`: an odd middle, every tap kept, and whole runs of eight.
        const { assert!(!MIDDLE.is_multiple_of(2) && MIDDLE < TAPS && TAPS.is_multiple_of(8)) };
        let middle = MIDDLE;
        let kept = middle + 1;
        let mut taps = [0.0_f64; TAPS];
        for (index, tap) in taps[TAPS - kept..].iter_mut().enumerate() {
            let from_middle = 2.0 * index as f64 - middle as f64;
            let sinc = (PI * from_middle / 2.0).sin() / (PI * from_middle / 2.0);
            let window = bessel_i0(BETA * (1.0 - (from_middle / middle as f64).powi(2)).sqrt());
            *tap = 0.5 * sinc * window / bessel_i0(BETA);
        }
        let sum: f64 = taps.iter().sum();
        let taps = taps.map(|tap| (tap * 0.5 / sum) as f32);
        Self { taps }
    }
}

/// The modified Bessel function of the first kind and order zero, by its series.
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut index) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * index)).powi(2);
        sum += term;
        index += 1.0;
    }
    sum
}

/// The sum of the products of two runs of numbers in eight lanes, which the compiler does
/// eight at a time: a plain sum in one order cannot be. The runs are a multiple of eight long.
fn dot<const TAPS: usize>(taps: &[f32; TAPS], samples: &[f32]) -> f32 {
    let mut lanes = [0.0_f32; 8];
    let chunks = taps.chunks_exact(8).zip(samples.chunks_exact(8));
    for (taps, samples) in chunks {
        for ((lane, tap), sample) in lanes.iter_mut().zip(taps).zip(samples) {
            *lane += tap * sample;
        }
    }
    lanes.iter().sum()
}

/// The last `TAPS` samples of one stream, oldest first, as one run: each is written twice, one
/// length apart, so the run never wraps.
#[derive(Clone, Copy)]
struct History<const TAPS: usize, const TWICE: usize> {
    samples: [f32; TWICE],
    next: usize,
}

impl<const TAPS: usize, const TWICE: usize> History<TAPS, TWICE> {
    const fn new() -> Self {
        const { assert!(TWICE == 2 * TAPS) };
        Self {
            samples: [0.0; TWICE],
            next: 0,
        }
    }

    /// Takes a sample and gives the last `TAPS`, this one last.
    fn push(&mut self, sample: f32) -> &[f32] {
        self.samples[self.next] = sample;
        self.samples[self.next + TAPS] = sample;
        self.next = (self.next + 1) % TAPS;
        &self.samples[self.next..self.next + TAPS]
    }
}

/// One stage up: two samples at twice the rate for each sample that comes in.
#[derive(Clone, Copy)]
struct Up<const TAPS: usize, const TWICE: usize> {
    history: History<TAPS, TWICE>,
}

impl<const TAPS: usize, const TWICE: usize> Up<TAPS, TWICE> {
    const fn new() -> Self {
        Self {
            history: History::new(),
        }
    }

    /// The input with a zero after each sample, filtered, times 2 for the zeros. At the even
    /// places only the taps at odd distances meet a sample; at the odd places only the middle
    /// one does, which is one half: the input as it was, `(middle - 1) / 2` samples ago.
    fn next<const MIDDLE: usize>(
        &mut self,
        filter: &HalfBand<MIDDLE, TAPS>,
        sample: f32,
    ) -> [f32; 2] {
        let recent = self.history.push(sample);
        let filtered = 2.0 * dot(&filter.taps, recent);
        let plain = recent[TAPS - 1 - (MIDDLE - 1) / 2];
        [filtered, plain]
    }
}

/// One stage down: one sample for each two at twice the rate.
#[derive(Clone, Copy)]
struct Down<const TAPS: usize, const TWICE: usize> {
    even: History<TAPS, TWICE>,
    odd: History<TAPS, TWICE>,
}

impl<const TAPS: usize, const TWICE: usize> Down<TAPS, TWICE> {
    const fn new() -> Self {
        Self {
            even: History::new(),
            odd: History::new(),
        }
    }

    /// The filter at every even place of the input: the taps at odd distances meet the even
    /// samples, and the middle one meets an odd sample, `(middle + 1) / 2` pairs ago.
    fn next<const MIDDLE: usize>(
        &mut self,
        filter: &HalfBand<MIDDLE, TAPS>,
        [even, odd]: [f32; 2],
    ) -> f32 {
        let filtered = dot(&filter.taps, self.even.push(even));
        let middle = self.odd.push(odd)[TAPS - 1 - MIDDLE.div_ceil(2)];
        filtered + 0.5 * middle
    }
}

/// The taps of both stages, the same for every channel.
pub struct Kernels {
    first: HalfBand<FIRST_MIDDLE, FIRST_TAPS>,
    second: HalfBand<SECOND_MIDDLE, SECOND_TAPS>,
}

impl Kernels {
    pub fn new() -> Self {
        Self {
            first: HalfBand::new(),
            second: HalfBand::new(),
        }
    }
}

/// The memory of one channel on its way up to four times the rate and down again.
#[derive(Clone, Copy)]
pub struct Oversampler {
    first_up: Up<FIRST_TAPS, { 2 * FIRST_TAPS }>,
    second_up: Up<SECOND_TAPS, { 2 * SECOND_TAPS }>,
    second_down: Down<SECOND_TAPS, { 2 * SECOND_TAPS }>,
    first_down: Down<FIRST_TAPS, { 2 * FIRST_TAPS }>,
    /// The last sample at twice the rate: the one sample of delay that makes the whole delay
    /// whole frames.
    held: f32,
}

impl Oversampler {
    pub const fn new() -> Self {
        Self {
            first_up: Up::new(),
            second_up: Up::new(),
            second_down: Down::new(),
            first_down: Down::new(),
            held: 0.0,
        }
    }

    /// Frames up to four times the rate: four samples for each, in order.
    pub fn up(&mut self, kernels: &Kernels, frames: &[f32], four: &mut [f32]) {
        for (sample, four) in frames.iter().zip(four.chunks_exact_mut(4)) {
            let [early, late] = self.first_up.next(&kernels.first, *sample);
            let twice = [std::mem::replace(&mut self.held, late), early];
            for (sample, two) in twice.into_iter().zip(four.chunks_exact_mut(2)) {
                two.copy_from_slice(&self.second_up.next(&kernels.second, sample));
            }
        }
    }

    /// Four samples for each frame down to the rate of the frames again.
    pub fn down(&mut self, kernels: &Kernels, four: &[f32], frames: &mut [f32]) {
        for (four, frame) in four.chunks_exact(4).zip(frames) {
            let mut twice = [0.0; 2];
            for (two, sample) in four.chunks_exact(2).zip(&mut twice) {
                *sample = self.second_down.next(&kernels.second, [two[0], two[1]]);
            }
            *frame = self.first_down.next(&kernels.first, twice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gain of a half band filter at a part of its rate, from all its taps.
    fn gain<const MIDDLE: usize, const TAPS: usize>(
        filter: &HalfBand<MIDDLE, TAPS>,
        at: f64,
    ) -> f64 {
        let (mut real, mut imaginary) = (0.5, 0.0);
        let kept = &filter.taps[TAPS - MIDDLE - 1..];
        for (index, tap) in kept.iter().enumerate() {
            let angle = 2.0 * PI * at * (2.0 * index as f64 - MIDDLE as f64);
            real += f64::from(*tap) * angle.cos();
            imaginary += f64::from(*tap) * angle.sin();
        }
        real.hypot(imaginary)
    }

    #[test]
    fn both_stages_keep_the_pass_band_and_take_88_db_off_the_stop_band() {
        let Kernels { first, second } = Kernels::new();
        let first = |at| gain(&first, at);
        let second = |at| gain(&second, at);
        let stages: [(&dyn Fn(f64) -> f64, f64, f64); 2] =
            [(&first, 0.225, 0.275), (&second, 0.1375, 0.3875)];
        for (gain, pass, stop) in stages {
            for step in 0..=100 {
                let at = pass * f64::from(step) / 100.0;
                let db = 20.0 * gain(at).log10();
                assert!(db.abs() < 0.001, "{at}: {db}");
                let at = stop + (0.5 - stop) * f64::from(step) / 100.0;
                let db = 20.0 * gain(at).log10();
                assert!(db < -88.0, "{at}: {db}");
            }
        }
    }

    /// A frame through the way up and down comes out the delay later, as it was, and blocks of
    /// any length make the same sound.
    #[test]
    fn a_frame_comes_out_the_delay_later() {
        let kernels = Kernels::new();
        let mut oversampler = Oversampler::new();
        let mut input = [0.0_f32; 200];
        input[0] = 1.0;
        let mut output = [0.0_f32; 200];
        let mut four = [0.0_f32; 4 * 64];
        let mut start = 0;
        for length in [1, 64, 7, 64, 64].into_iter().cycle() {
            let end = (start + length).min(input.len());
            let four = &mut four[..4 * (end - start)];
            oversampler.up(&kernels, &input[start..end], four);
            oversampler.down(&kernels, four, &mut output[start..end]);
            start = end;
            if start == input.len() {
                break;
            }
        }
        let loudest = (0..output.len())
            .max_by(|a, b| output[*a].abs().total_cmp(&output[*b].abs()))
            .unwrap();
        assert_eq!(loudest, DELAY_FRAMES);
        let sum: f32 = output.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "{sum}");
        // Symmetric around it: linear phase.
        for offset in 1..DELAY_FRAMES {
            let (before, after) = (output[DELAY_FRAMES - offset], output[DELAY_FRAMES + offset]);
            assert!((before - after).abs() < 1e-6, "{offset}: {before} {after}");
        }
    }
}
