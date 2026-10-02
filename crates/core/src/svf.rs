//! The state variable filter of the built-in Filter effect and the Wavetable synth: low, band,
//! high pass and notch, at 12 or 24 dB per octave, with resonance. The EQ and the Utility mix
//! its raw outputs in their own shapes ([`SvfSection::band_and_low`]). Next to
//! [`Smoothed`](crate::Smoothed), a helper a processor uses per frame. The core itself filters
//! nothing.
//!
//! Each section is the state variable filter in its trapezoidal form (Simper, "Solving the
//! continuous SVF equations using trapezoidal integration and equivalent currents", 2013). It
//! is linear and stable for every cutoff and every resonance, also while they move, which is
//! what an analog filter of this shape does too: no setting blows up. At 12 dB per octave one
//! section plays. At 24 dB the second follows the first, and the two are the sections of a
//! fourth order Butterworth filter at resonance 0, so the cutoff is at -3 dB for both slopes.
//!
//! The resonance is in the first section only. The second has a fixed Q that never boosts, so a
//! glide between the slopes never rings at a Q that neither end has. As resonance rises, the low
//! and high pass get quieter by the square root of how far their Q rose, as an analog ladder
//! loses its bass: the peak of full resonance is about +12 dB and not +26.
//!
//! Every section gives low, band, high pass and notch at once from one memory. The type is a
//! weight for each ([`FilterType::taps`]), so a processor can glide from one type to another,
//! and from one slope to the other ([`FilterSlope::weight`]) by running both sections and
//! gliding from the output of the first to the output of the second.

use std::f32::consts::{FRAC_1_SQRT_2, PI};

use serde::{Deserialize, Serialize};

/// Which part of the sound the filter lets through.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterType {
    /// What is below the cutoff.
    LowPass,
    /// A band around the cutoff. Resonance makes it narrower.
    BandPass,
    /// What is above the cutoff.
    HighPass,
    /// Everything but a band around the cutoff. Resonance makes the gap narrower.
    Notch,
}

impl FilterType {
    pub const ALL: [Self; 4] = [Self::LowPass, Self::BandPass, Self::HighPass, Self::Notch];

    /// How much of the low, band and high pass and of the notch of a section the type takes,
    /// for [`SvfSection::next`].
    pub fn taps(self) -> [f32; 4] {
        match self {
            Self::LowPass => [1.0, 0.0, 0.0, 0.0],
            Self::BandPass => [0.0, 1.0, 0.0, 0.0],
            Self::HighPass => [0.0, 0.0, 1.0, 0.0],
            Self::Notch => [0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// How steeply the filter cuts past the cutoff, in dB per octave. Saved as the number, `12` or
/// `24`, and nothing else loads.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum FilterSlope {
    Twelve,
    TwentyFour,
}

impl FilterSlope {
    pub const ALL: [Self; 2] = [Self::Twelve, Self::TwentyFour];

    pub const fn db_per_octave(self) -> u8 {
        match self {
            Self::Twelve => 12,
            Self::TwentyFour => 24,
        }
    }

    /// 0 for one section, 1 for two: how much of the output of the second section a processor
    /// takes. A value between is a glide from one slope to the other.
    pub fn weight(self) -> f32 {
        match self {
            Self::Twelve => 0.0,
            Self::TwentyFour => 1.0,
        }
    }
}

impl TryFrom<u8> for FilterSlope {
    type Error = String;

    fn try_from(db: u8) -> Result<Self, String> {
        match db {
            12 => Ok(Self::Twelve),
            24 => Ok(Self::TwentyFour),
            _ => Err(format!("slope must be 12 or 24, not {db}")),
        }
    }
}

impl From<FilterSlope> for u8 {
    fn from(slope: FilterSlope) -> Self {
        slope.db_per_octave()
    }
}

/// The Q of the first section at resonance 1. With the level that the resonance takes from the
/// low and high pass, the peak at the cutoff is +11.5 dB at 12 dB per octave and +8.8 dB at 24.
/// It rings, and it never runs away.
pub const SVF_MAX_Q: f32 = 20.0;

/// The Q of one section at resonance 0: a second order Butterworth filter.
const BUTTERWORTH_Q: f32 = FRAC_1_SQRT_2;

/// The Qs of the two sections of a fourth order Butterworth filter, `1 / (2 cos(π/8))` and
/// `1 / (2 cos(3π/8))`. Their product is `1/√2`, so the cutoff is at -3 dB here too.
const BUTTERWORTH_4: [f32; 2] = [0.541_196_1, 1.306_563];

/// A cutoff pushed past the ends of the range stays inside it, and under a part of the sample
/// rate, below the Nyquist frequency where the filter's factors would run away.
const LOWEST_HZ: f32 = 20.0;
const HIGHEST_HZ: f32 = 20_000.0;
const HIGHEST_PART: f32 = 0.45;

/// While the input is silent, a memory smaller than this is let go of: -180 dB, far under
/// anything audible. So a filter after a sound that ended comes to rest and does no work, also
/// after the slow ring of a low cutoff at full resonance.
const REST: f32 = 1e-9;

/// The first section at a resonance, with `slope` from 0 (12 dB per octave) to 1 (24 dB): its
/// Q, and the level of its low and high pass. Resonance raises the Q from its Butterworth value
/// on a ratio, so equal steps of resonance are equal steps of the peak in dB. The Butterworth
/// value glides between the one of a single section and its place in the fourth order filter.
/// The level is `sqrt(butterworth / q)`, so the peak rises by half as many dB as the Q.
fn first_section(resonance: f32, slope: f32) -> (f32, f32) {
    let butterworth = BUTTERWORTH_Q + (BUTTERWORTH_4[1] - BUTTERWORTH_Q) * slope;
    let rise = (SVF_MAX_Q / butterworth).powf(resonance);
    (butterworth * rise, 1.0 / rise.sqrt())
}

/// The Q of the second section: fixed, and below the one that would boost at the cutoff.
const SECOND_Q: f32 = BUTTERWORTH_4[0];

/// The cutoff the filter really uses: inside what the sample rate allows.
fn usable_hz(hz: f32, sample_rate: f32) -> f32 {
    // Not `clamp`: it panics when the bounds cross, and nothing may panic on the audio thread.
    hz.max(LOWEST_HZ)
        .min(HIGHEST_HZ.min(HIGHEST_PART * sample_rate))
}

/// The gain at `hz` of a filter, as a complex number, once every change has arrived: what a
/// quiet steady sine comes out with. The cutoff is held inside what the sample rate allows, as
/// [`SvfFactors::sections`] holds it.
///
/// This is the exact response of the filter, not a drawing of one: the trapezoidal form is the
/// analog filter with its frequencies bent by `tan(π f / sample rate)`, so the analog response
/// at the bent frequency is the answer.
pub fn svf_response(
    kind: FilterType,
    slope: FilterSlope,
    cutoff_hz: f32,
    resonance: f32,
    hz: f32,
    sample_rate: f32,
) -> (f64, f64) {
    let bend = |hz: f32| (f64::from(PI) * f64::from(hz) / f64::from(sample_rate)).tan();
    let cutoff = usable_hz(cutoff_hz, sample_rate);
    let at = bend(hz.min(0.499 * sample_rate)) / bend(cutoff);
    let taps = kind.taps().map(f64::from);
    let (q, level) = first_section(resonance, slope.weight());
    let one = section_response(taps, f64::from(q), f64::from(level), at);
    match slope {
        FilterSlope::Twelve => one,
        FilterSlope::TwentyFour => {
            let second = section_response(taps, f64::from(SECOND_Q), 1.0, at);
            multiply(one, second)
        }
    }
}

/// One section at `at` times its cutoff, as a complex number: `(level (low + high s²) + band k s
/// + notch (s² + 1)) / (s² + k s + 1)` with `s = j at` and `k = 1 / q`. The band is scaled by
/// `k`, so its peak is 1.
fn section_response([low, band, high, notch]: [f64; 4], q: f64, level: f64, at: f64) -> (f64, f64) {
    let k = 1.0 / q;
    let square = at * at;
    let numerator = (
        level * (low - high * square) + notch * (1.0 - square),
        band * k * at,
    );
    let denominator = (1.0 - at * at, k * at);
    let size = denominator.0 * denominator.0 + denominator.1 * denominator.1;
    (
        (numerator.0 * denominator.0 + numerator.1 * denominator.1) / size,
        (numerator.1 * denominator.0 - numerator.0 * denominator.1) / size,
    )
}

fn multiply(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

/// The factors of one section for one cutoff and Q.
#[derive(Copy, Clone, Debug, Default)]
pub struct SvfFactors {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl SvfFactors {
    /// `g` is [`Self::cutoff_factor`] and `k` the damping, `1 / Q`. For a filter of its own
    /// shape; [`Self::sections`] gives the ones of this filter.
    pub fn new(g: f32, k: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self {
            k,
            a1,
            a2,
            a3: g * a2,
        }
    }

    /// `g` of a cutoff, `tan(π cutoff / sample rate)`, with the cutoff held inside what the
    /// sample rate allows. A `tan`: work it out when the cutoff moves, not per frame.
    pub fn cutoff_factor(cutoff_hz: f32, sample_rate: f32) -> f32 {
        (PI * usable_hz(cutoff_hz, sample_rate) / sample_rate).tan()
    }

    /// Both sections of a filter at a cutoff, a resonance from 0 to 1 and a slope weight from 0
    /// (12 dB per octave) to 1 (24 dB, see [`FilterSlope::weight`]), and the level of the low
    /// and high pass of the first section. The cutoff is held inside what the sample rate
    /// allows. A `tan` and a `powf`: work it out when something moves, not per frame.
    pub fn sections(
        cutoff_hz: f32,
        resonance: f32,
        slope: f32,
        sample_rate: f32,
    ) -> ([Self; 2], f32) {
        let g = Self::cutoff_factor(cutoff_hz, sample_rate);
        let (q, level) = first_section(resonance, slope);
        ([Self::new(g, 1.0 / q), Self::new(g, 1.0 / SECOND_Q)], level)
    }
}

/// The memory of one section: the two integrators.
#[derive(Copy, Clone, Debug, Default)]
pub struct SvfSection {
    ic1: f32,
    ic2: f32,
}

impl SvfSection {
    /// One frame. Returns the mix of low, band and high pass and notch that `taps` asks for,
    /// with the low and the high pass at `level`.
    #[inline]
    pub fn next(
        &mut self,
        factors: &SvfFactors,
        [low, band, high, notch]: [f32; 4],
        level: f32,
        input: f32,
    ) -> f32 {
        let (v1, v2) = self.band_and_low(factors, input);
        let high_pass = input - factors.k * v1 - v2;
        let band_pass = factors.k * v1;
        level * (low * v2 + high * high_pass) + band * band_pass + notch * (input - band_pass)
    }

    /// One frame, before any mix: the band pass and the low pass, from which every other output
    /// is made. This band pass is not scaled by `k`, so its peak is Q and not 1.
    #[inline]
    pub fn band_and_low(&mut self, factors: &SvfFactors, input: f32) -> (f32, f32) {
        let SvfFactors { a1, a2, a3, .. } = *factors;
        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v1, v2)
    }

    /// Lets go of a memory too small to hear, so a filter whose input went silent comes to
    /// rest. Call it after a block of silent input.
    pub fn settle(&mut self) {
        for memory in [&mut self.ic1, &mut self.ic2] {
            if memory.abs() < REST {
                *memory = 0.0;
            }
        }
    }

    pub fn is_silent(&self) -> bool {
        self.ic1 == 0.0 && self.ic2 == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Full resonance: the Q of the first section is `SVF_MAX_Q` and its level takes half of
    /// that rise in dB back, so the peak of one section is `sqrt(q0 SVF_MAX_Q)`, +11.5 dB.
    #[test]
    fn full_resonance_peaks_at_the_root_of_its_rise() {
        let (real, imaginary) = svf_response(
            FilterType::LowPass,
            FilterSlope::Twelve,
            1_000.0,
            1.0,
            1_000.0,
            48_000.0,
        );
        let peak = real.hypot(imaginary) as f32;
        let expected = (BUTTERWORTH_Q * SVF_MAX_Q).sqrt();
        assert!((peak - expected).abs() < 0.001, "{peak}");
    }
}
