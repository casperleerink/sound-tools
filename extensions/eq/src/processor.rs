//! The EQ processor: four bands one after the other, then the output gain.
//!
//! Each band is one state variable filter in its trapezoidal form (Simper, "Solving the
//! continuous SVF equations using trapezoidal integration and equivalent currents", 2013), the
//! same as a section of the Filter. It is linear and stable for every frequency, gain and Q,
//! also while they move: no setting blows up. A band gives its band pass and its low pass from
//! one memory, and every shape is a mix of those two and the input, with its own cutoff factor
//! `g` and damping `k` (Simper, "Linear trapezoidal integrated state variable filter", 2016):
//!
//! | Shape | `g` | `k` | input, band, low |
//! | --- | --- | --- | --- |
//! | Low cut | `t` | `1/q` | `1, -k, -1` |
//! | Low shelf | `t/√a` | `1/q` | `1, k (a - 1), a² - 1` |
//! | Bell | `t` | `1/(q a)` | `1, k (a² - 1), 0` |
//! | Notch | `t` | `1/q` | `1, -k, 0` |
//! | High shelf | `t √a` | `1/q` | `a², k (1 - a) a, 1 - a²` |
//! | High cut | `t` | `1/q` | `0, 0, 1` |
//!
//! with `t = tan(π f / sample rate)` and `a = 10^(gain / 40)`. So a band at 0 dB, or off, is
//! the mix `1, 0, 0`: the input, exactly.
//!
//! A change of shape is a glide too: the weight of each shape glides, and a band that is
//! between shapes takes the mean of their `g` and `k` on a log scale and the mean of their
//! mixes. On and off is a glide of the mix towards `1, 0, 0`. Nothing a composer or an agent
//! changes jumps.

use std::f32::consts::PI;

use sound_core::{
    AudioInput, AudioOutput, CHANNELS, Ports, PrepareConfig, ProcessContext, Processor, Smoothed,
};

use crate::{BANDS, Band, EqState, Shape};

/// How long a change takes to arrive. A jump would click, or step in the sound.
const RAMP_SECONDS: f32 = 0.02;

/// A frequency stays inside the range, and under a part of the sample rate, below the Nyquist
/// frequency where the factor `tan` runs away.
const LOWEST_HZ: f32 = 20.0;
const HIGHEST_HZ: f32 = 20_000.0;
const HIGHEST_PART: f32 = 0.45;

/// While something moves, the factors are worked out again this often, as in the Filter.
const FACTOR_FRAMES: usize = 16;

/// Input louder than this, or not a number, is held to it before anything else, so no sample of
/// anyone else's can make the memory of a band infinite. +36 dBFS: nothing real comes near it.
const INPUT_LIMIT: f32 = 64.0;

/// While the input is silent, a memory smaller than this is let go of: -180 dB. So an EQ after
/// a sound that ended comes to rest and does no work.
const REST: f32 = 1e-9;

/// What a band does to the input, `input`, the band pass `band` and the low pass `low` of its
/// memory, added up.
const THROUGH: [f32; 3] = [1.0, 0.0, 0.0];

/// The frequency the EQ really uses: inside what the sample rate allows.
fn usable_hz(hz: f32, sample_rate: f32) -> f32 {
    // Not `clamp`: it panics when the bounds cross, and nothing may panic on the audio thread.
    hz.max(LOWEST_HZ)
        .min(HIGHEST_HZ.min(HIGHEST_PART * sample_rate))
}

/// One band at one setting: its cutoff factor, its damping, and its mix of input, band pass
/// and low pass.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Shaped {
    g: f32,
    k: f32,
    mix: [f32; 3],
}

/// The table of the module documentation.
fn shaped(shape: Shape, hz: f32, gain_db: f32, q: f32, sample_rate: f32) -> Shaped {
    let g = (PI * usable_hz(hz, sample_rate) / sample_rate).tan();
    let a = 10_f32.powf(gain_db / 40.0);
    let k = 1.0 / q;
    match shape {
        Shape::LowCut => Shaped {
            g,
            k,
            mix: [1.0, -k, -1.0],
        },
        Shape::LowShelf => Shaped {
            g: g / a.sqrt(),
            k,
            mix: [1.0, k * (a - 1.0), a * a - 1.0],
        },
        Shape::Bell => {
            let k = 1.0 / (q * a);
            Shaped {
                g,
                k,
                mix: [1.0, k * (a * a - 1.0), 0.0],
            }
        }
        Shape::Notch => Shaped {
            g,
            k,
            mix: [1.0, -k, 0.0],
        },
        Shape::HighShelf => Shaped {
            g: g * a.sqrt(),
            k,
            mix: [a * a, k * (1.0 - a) * a, 1.0 - a * a],
        },
        Shape::HighCut => Shaped {
            g,
            k,
            mix: [0.0, 0.0, 1.0],
        },
    }
}

/// The gain of one band at `hz`, as a complex number, once every change has arrived. Off, it
/// is 1.
///
/// The trapezoidal form is the analog filter with its frequencies bent by `tan(π f / sample
/// rate)`, so this is the analog response at the bent frequency: `input + (band s + low) / (s²
/// + k s + 1)` with `s = j tan(π f / sample rate) / g`. Exact, not a drawing.
pub fn band_response(band: &Band, hz: f32, sample_rate: f32) -> (f64, f64) {
    if !band.on {
        return (1.0, 0.0);
    }
    let Shaped { g, k, mix } = shaped(
        band.shape,
        band.frequency_hz,
        band.gain_db,
        band.q,
        sample_rate,
    );
    let [input, band, low] = mix.map(f64::from);
    let (g, k) = (f64::from(g), f64::from(k));
    let bent = (std::f64::consts::PI * f64::from(hz.min(0.499 * sample_rate))
        / f64::from(sample_rate))
    .tan();
    let at = bent / g;
    // (low + j band at) / (1 - at² + j k at)
    let (numerator, denominator) = ((low, band * at), (1.0 - at * at, k * at));
    let size = denominator.0 * denominator.0 + denominator.1 * denominator.1;
    (
        input + (numerator.0 * denominator.0 + numerator.1 * denominator.1) / size,
        (numerator.1 * denominator.0 - numerator.0 * denominator.1) / size,
    )
}

/// The gain at `hz` of an EQ with this record, as a factor, once every change has arrived:
/// what a steady sine comes out with. The product of the bands, then the output gain.
///
/// This is the exact response of the processor. The card draws it and the tests hold the
/// measured sound to it.
pub fn response(state: &EqState, hz: f32, sample_rate: f32) -> f32 {
    let (real, imaginary) = state.bands.iter().fold((1.0, 0.0), |sum, band| {
        let one = band_response(band, hz, sample_rate);
        (sum.0 * one.0 - sum.1 * one.1, sum.0 * one.1 + sum.1 * one.0)
    });
    let output = 10_f64.powf(f64::from(state.output_gain_db) / 20.0);
    (output * real.hypot(imaginary)) as f32
}

/// The factors of one band's filter for one `g` and `k`.
#[derive(Copy, Clone, Default)]
struct Factors {
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Factors {
    fn new(g: f32, k: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { a1, a2, a3: g * a2 }
    }
}

/// The memory of one band in one channel: the two integrators.
#[derive(Copy, Clone, Default)]
struct Section {
    ic1: f32,
    ic2: f32,
}

impl Section {
    /// One frame: the band pass and the low pass.
    fn next(&mut self, factors: &Factors, input: f32) -> (f32, f32) {
        let Factors { a1, a2, a3 } = *factors;
        let v3 = input - self.ic2;
        let band = a1 * self.ic1 + a2 * v3;
        let low = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * band - self.ic1;
        self.ic2 = 2.0 * low - self.ic2;
        (band, low)
    }

    fn settle(&mut self) {
        for memory in [&mut self.ic1, &mut self.ic2] {
            if memory.abs() < REST {
                *memory = 0.0;
            }
        }
    }

    fn is_silent(&self) -> bool {
        self.ic1 == 0.0 && self.ic2 == 0.0
    }
}

/// A sample of the input as the EQ takes it: held to [`INPUT_LIMIT`], and silence for anything
/// that is not a number.
fn held(sample: f32) -> f32 {
    if sample.is_nan() {
        return 0.0;
    }
    sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
}

/// One band: where its settings are on their glides, its factors, and its memory.
struct BandGlide {
    /// The frequency as `log2` of hertz, so a glide moves it in octaves.
    octaves: Smoothed,
    gain_db: Smoothed,
    /// The Q as `log2`, so a glide moves it on a ratio.
    q_octaves: Smoothed,
    /// The weight of each shape of [`Shape::ALL`].
    shapes: [Smoothed; 6],
    /// 1 is on, 0 is off.
    on: Smoothed,
    factors: Factors,
    /// The mix at the end of the last run of frames, and where it is going in the run now. It
    /// moves frame by frame inside a run, so a gain glide makes no steps.
    mix: [f32; 3],
    mix_target: [f32; 3],
    sections: [Section; CHANNELS],
}

impl BandGlide {
    fn new() -> Self {
        Self {
            octaves: Smoothed::new(0.0),
            gain_db: Smoothed::new(0.0),
            q_octaves: Smoothed::new(0.0),
            shapes: [0.0; 6].map(Smoothed::new),
            on: Smoothed::new(0.0),
            factors: Factors::default(),
            mix: THROUGH,
            mix_target: THROUGH,
            sections: [Section::default(); CHANNELS],
        }
    }

    fn aim(&mut self, band: &Band, ramp: f32) {
        self.octaves.set_target(band.frequency_hz.log2(), ramp);
        self.gain_db.set_target(band.gain_db, ramp);
        self.q_octaves.set_target(band.q.log2(), ramp);
        for (shape, weight) in Shape::ALL.iter().zip(&mut self.shapes) {
            weight.set_target(if *shape == band.shape { 1.0 } else { 0.0 }, ramp);
        }
        self.on.set_target(if band.on { 1.0 } else { 0.0 }, ramp);
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        [
            &mut self.octaves,
            &mut self.gain_db,
            &mut self.q_octaves,
            &mut self.on,
        ]
        .into_iter()
        .chain(&mut self.shapes)
    }

    fn is_moving(&self) -> bool {
        [&self.octaves, &self.gain_db, &self.q_octaves, &self.on]
            .into_iter()
            .chain(&self.shapes)
            .any(Smoothed::is_moving)
    }

    /// Moves every glide `frames` along, and works out the factors for where they are when
    /// anything moves or `stale` says so.
    fn move_factors(&mut self, frames: usize, stale: bool, sample_rate: f32) {
        self.mix = self.mix_target;
        let changes = stale || self.is_moving();
        let hz = self.octaves.advance(frames).exp2();
        let gain_db = self.gain_db.advance(frames);
        let q = self.q_octaves.advance(frames).exp2();
        let on = self.on.advance(frames);
        let weights = self.shapes.each_mut().map(|weight| weight.advance(frames));
        if !changes {
            return;
        }
        let one = |shape| shaped(shape, hz, gain_db, q, sample_rate);
        // At rest one shape has all the weight, and the band is exactly that shape.
        let Shaped { g, k, mix } = match weights.iter().position(|weight| *weight == 1.0) {
            Some(index) => one(Shape::ALL[index]),
            None => {
                let (mut log_g, mut log_k, mut mix, mut total) = (0.0, 0.0, [0.0; 3], 0.0);
                for (shape, weight) in Shape::ALL.into_iter().zip(weights) {
                    if weight <= 0.0 {
                        continue;
                    }
                    let shaped = one(shape);
                    log_g += weight * shaped.g.ln();
                    log_k += weight * shaped.k.ln();
                    for (sum, part) in mix.iter_mut().zip(shaped.mix) {
                        *sum += weight * part;
                    }
                    total += weight;
                }
                // The weights glide from one shape to another together, so they add up to 1;
                // this only keeps rounding out.
                let total = total.max(f32::EPSILON);
                Shaped {
                    g: (log_g / total).exp(),
                    k: (log_k / total).exp(),
                    mix: mix.map(|part| part / total),
                }
            }
        };
        self.factors = Factors::new(g, k);
        self.mix_target = if on == 1.0 {
            mix
        } else {
            std::array::from_fn(|index| THROUGH[index] + on * (mix[index] - THROUGH[index]))
        };
        // After a snap there is nothing to glide from.
        if stale {
            self.mix = self.mix_target;
        }
    }
}

pub struct Eq {
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    bands: [BandGlide; BANDS],
    /// The output gain, as a factor.
    output: Smoothed,
    /// Whether the factors have to be worked out again although nothing glides: after a snap,
    /// and before the first block.
    stale: bool,
}

impl Eq {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Starts at these values, so an EQ that is added or opened does not glide in.
    pub fn new(state: EqState) -> Self {
        let mut eq = Self {
            sample_rate: 48_000.0,
            ramp_frames: 1.0,
            bands: std::array::from_fn(|_| BandGlide::new()),
            output: Smoothed::new(1.0),
            stale: true,
        };
        eq.aim(&state);
        eq.snap();
        eq
    }

    /// Sets every target from a record.
    fn aim(&mut self, state: &EqState) {
        let ramp = self.ramp_frames;
        for (glide, band) in self.bands.iter_mut().zip(&state.bands) {
            glide.aim(band, ramp);
        }
        let output = 10_f32.powf(state.output_gain_db / 20.0);
        self.output.set_target(output, ramp);
    }

    /// Takes every target at once. For an EQ nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        for band in &mut self.bands {
            band.smoothers().for_each(Smoothed::snap);
        }
        self.output.snap();
        self.stale = true;
    }

    fn is_resting(&self) -> bool {
        self.bands
            .iter()
            .flat_map(|band| &band.sections)
            .all(Section::is_silent)
    }
}

impl Processor for Eq {
    type Update = EqState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.ramp_frames = (RAMP_SECONDS * self.sample_rate).max(1.0);
        self.stale = true;
    }

    fn update(&mut self, update: &mut EqState) {
        self.aim(update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = left_in.iter().chain(right_in).all(|sample| *sample == 0.0);
        if silent_input && self.is_resting() {
            // Nothing sounds and nothing rings: no glide can be heard, and the output is
            // already silent.
            self.snap();
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let chunks = left_in
            .chunks(FACTOR_FRAMES)
            .zip(right_in.chunks(FACTOR_FRAMES))
            .zip(left_out.chunks_mut(FACTOR_FRAMES))
            .zip(right_out.chunks_mut(FACTOR_FRAMES));
        for (((left_in, right_in), left_out), right_out) in chunks {
            let length = left_in.len();
            let stale = std::mem::take(&mut self.stale);
            for band in &mut self.bands {
                band.move_factors(length, stale, self.sample_rate);
            }
            let frames = left_in
                .iter()
                .zip(right_in)
                .zip(left_out.iter_mut())
                .zip(right_out.iter_mut());
            for (index, (((left_in, right_in), left_out), right_out)) in frames.enumerate() {
                let along = (index + 1) as f32 / length as f32;
                let output = self.output.advance(1);
                let mut sound = [held(*left_in), held(*right_in)];
                for band in &mut self.bands {
                    let [input, band_pass, low_pass] = std::array::from_fn(|part| {
                        band.mix[part] + (band.mix_target[part] - band.mix[part]) * along
                    });
                    for (section, sample) in band.sections.iter_mut().zip(&mut sound) {
                        let (band_out, low_out) = section.next(&band.factors, *sample);
                        *sample = input * *sample + band_pass * band_out + low_pass * low_out;
                    }
                }
                *left_out = sound[0] * output;
                *right_out = sound[1] * output;
            }
        }
        if silent_input {
            let sections = self.bands.iter_mut().flat_map(|band| &mut band.sections);
            sections.for_each(Section::settle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BANDS;

    const RATE: f32 = 48_000.0;

    fn db(state: &EqState, hz: f32) -> f32 {
        20.0 * response(state, hz, RATE).log10()
    }

    fn one_band(shape: Shape, gain_db: f32, q: f32) -> EqState {
        let mut state = EqState::default();
        for band in &mut state.bands {
            band.on = false;
        }
        state.bands[1] = Band {
            on: true,
            shape,
            frequency_hz: 1_000.0,
            gain_db,
            q,
        };
        state
    }

    #[test]
    fn the_default_eq_is_flat_everywhere() {
        let state = EqState::default();
        for hz in [20.0, 100.0, 1_000.0, 10_000.0, 20_000.0] {
            assert_eq!(response(&state, hz, RATE), 1.0, "{hz}");
        }
        assert_eq!(state.bands.len(), BANDS);
    }

    /// A bell is its gain at its frequency; a shelf is half its gain there and the whole gain
    /// far on its side; a cut is 3 dB down at Q 0.71 and a notch takes its frequency out.
    #[test]
    fn each_shape_has_its_gain_at_the_frequency() {
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(near(db(&one_band(Shape::Bell, 9.0, 2.0), 1_000.0), 9.0));
        assert!(near(db(&one_band(Shape::Bell, -12.0, 0.5), 1_000.0), -12.0));
        let low_shelf = one_band(Shape::LowShelf, 10.0, 0.71);
        assert!(near(db(&low_shelf, 1_000.0), 5.0));
        assert!(near(db(&low_shelf, 20.0), 10.0));
        let high_shelf = one_band(Shape::HighShelf, -6.0, 0.71);
        assert!(near(db(&high_shelf, 1_000.0), -3.0));
        assert!(near(db(&high_shelf, 20.0), 0.0));
        let low_cut = one_band(Shape::LowCut, 0.0, std::f32::consts::FRAC_1_SQRT_2);
        assert!(near(db(&low_cut, 1_000.0), -3.0103));
        let high_cut = one_band(Shape::HighCut, 0.0, std::f32::consts::FRAC_1_SQRT_2);
        assert!(near(db(&high_cut, 1_000.0), -3.0103));
        assert!(db(&one_band(Shape::Notch, 0.0, 1.0), 1_000.0) < -100.0);
    }
}
