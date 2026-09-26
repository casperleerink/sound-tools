//! The filter processor: a drive stage, two state variable filter sections, and the mix.
//!
//! Each section is the state variable filter in its trapezoidal form (Simper, "Solving the
//! continuous SVF equations using trapezoidal integration and equivalent currents", 2013). It
//! is linear and stable for every cutoff and every resonance, also while they move, which is
//! what an analog filter of this shape does too: no setting blows up. At 12 dB per octave one
//! section plays. At 24 dB the second follows the first, and the two are the sections of a
//! fourth order Butterworth filter at resonance 0, so the cutoff is at -3 dB for both slopes.
//!
//! Every section gives low, band and high pass at once from one memory. The type is a weight
//! for each of the three, so a change of type is a glide between them and not a switch, and so
//! is a change of slope: both sections always run and the output glides from the first to the
//! second. Nothing that a composer or an agent changes jumps.

use std::f32::consts::{FRAC_1_SQRT_2, PI, TAU};

use sound_core::{
    AudioInput, AudioOutput, CHANNELS, Ports, PrepareConfig, ProcessContext, Processor, Smoothed,
};

use crate::{FilterState, FilterType, Slope};

/// How long a change takes to arrive. A jump would click, or step in the sound.
pub const RAMP_SECONDS: f32 = 0.02;

/// The Q of the section with the peak at resonance 1. At 12 dB per octave that is a peak of
/// +26 dB at the cutoff, at 24 dB +21 dB. It rings, and it never runs away.
pub const MAX_Q: f32 = 20.0;

/// The Q of one section at resonance 0: a second order Butterworth filter.
const BUTTERWORTH_Q: f32 = FRAC_1_SQRT_2;

/// The Qs of the two sections of a fourth order Butterworth filter, `1 / (2 cos(π/8))` and
/// `1 / (2 cos(3π/8))`. Their product is `1/√2`, so the cutoff is at -3 dB here too.
const BUTTERWORTH_4: [f32; 2] = [0.541_196_1, 1.306_563];

/// A cutoff the LFO pushes past the ends stays inside these. The upper one is a part of the
/// sample rate, under the Nyquist frequency where the filter's factors would run away.
const LOWEST_HZ: f32 = 5.0;
const HIGHEST_PART: f32 = 0.45;

/// While something moves, the factors are worked out again this often. Four times per block
/// of the engine: a sweep has no steps anyone can hear, and a `tan` per frame is not needed.
const FACTOR_FRAMES: usize = 16;

/// The saturation after the drive gain is exactly clean up to full scale, then bends softly
/// towards `KNEE + BEND`. So drive 0 leaves every sound up to full scale as it is, and the drive
/// is one gain into one fixed curve: turning it up never makes the sound louder than the curve
/// allows on the way.
const KNEE: f32 = 1.0;
const BEND: f32 = 0.5;

/// Input louder than this, or not a number, is held to it before anything else, so no sample of
/// anyone else's can make the filter's memory infinite. +36 dBFS: nothing real comes near it.
const INPUT_LIMIT: f32 = 64.0;

/// While the input is silent, a memory smaller than this is let go of: -180 dB, far under
/// anything audible. So a filter after a sound that ended comes to rest and does no work, also
/// after the slow ring of a low cutoff at full resonance.
const REST: f32 = 1e-9;

/// The Q of the two sections at a resonance, with `slope` from 0 (12 dB per octave) to 1 (24 dB).
/// Resonance raises the Q of the last section on a ratio, so that equal steps of resonance are
/// equal steps of the peak in dB. Between the two slopes the first section glides from the
/// one-section Q to its place in the fourth order filter.
fn section_q(resonance: f32, slope: f32) -> [f32; 2] {
    let resonant = |base: f32| base * (MAX_Q / base).powf(resonance);
    let alone = resonant(BUTTERWORTH_Q);
    [
        alone + (BUTTERWORTH_4[0] - alone) * slope,
        resonant(BUTTERWORTH_4[1]),
    ]
}

/// How much of the low, band and high pass of a section the type takes. The notch is the low
/// and the high together.
fn taps(kind: FilterType) -> [f32; 3] {
    match kind {
        FilterType::LowPass => [1.0, 0.0, 0.0],
        FilterType::BandPass => [0.0, 1.0, 0.0],
        FilterType::HighPass => [0.0, 0.0, 1.0],
        FilterType::Notch => [1.0, 0.0, 1.0],
    }
}

fn slope_weight(slope: Slope) -> f32 {
    match slope {
        Slope::Twelve => 0.0,
        Slope::TwentyFour => 1.0,
    }
}

/// The cutoff the filter really uses: inside what the sample rate allows.
fn usable_hz(hz: f32, sample_rate: f32) -> f32 {
    // Not `clamp`: it panics when the bounds cross, and nothing may panic on the audio thread.
    hz.max(LOWEST_HZ).min(HIGHEST_PART * sample_rate)
}

/// The gain at `hz` of a filter with this record, as a factor, once every change has arrived:
/// what a quiet steady sine comes out with, at the cutoff the record says and with its mix.
/// Drive is left out, because what it does depends on the level; a quiet sound gets its gain.
///
/// This is the exact response of the processor, not a drawing of one: the trapezoidal form is
/// the analog filter with its frequencies bent by `tan(π f / sample rate)`, so the analog
/// response at the bent frequency is the answer. The card draws it and the tests hold the
/// measured sound to it.
pub fn response(state: &FilterState, hz: f32, sample_rate: f32) -> f32 {
    let bend = |hz: f32| (f64::from(PI) * f64::from(hz) / f64::from(sample_rate)).tan();
    let cutoff = usable_hz(state.cutoff_hz, sample_rate);
    let at = bend(hz.min(0.499 * sample_rate)) / bend(cutoff);
    let taps = taps(state.kind).map(f64::from);
    let slope = slope_weight(state.slope);
    let [first, second] = section_q(state.resonance, slope).map(f64::from);
    let one = section_response(taps, first, at);
    let filtered = match state.slope {
        Slope::Twelve => one,
        Slope::TwentyFour => multiply(one, section_response(taps, second, at)),
    };
    let mix = f64::from(state.mix);
    let (real, imaginary) = (mix * filtered.0 + (1.0 - mix), mix * filtered.1);
    real.hypot(imaginary) as f32
}

/// One section at `at` times its cutoff, as a complex number: `(low + band k s + high s²) /
/// (s² + k s + 1)` with `s = j at` and `k = 1 / q`. The band is scaled by `k`, so its peak is 1.
fn section_response([low, band, high]: [f64; 3], q: f64, at: f64) -> (f64, f64) {
    let k = 1.0 / q;
    let numerator = (low - high * at * at, band * k * at);
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
#[derive(Copy, Clone, Default)]
struct Factors {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Factors {
    /// `g` is `tan(π cutoff / sample rate)`.
    fn new(g: f32, q: f32) -> Self {
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self {
            k,
            a1,
            a2,
            a3: g * a2,
        }
    }
}

/// The memory of one section: the two integrators.
#[derive(Copy, Clone, Default)]
struct Section {
    ic1: f32,
    ic2: f32,
}

impl Section {
    /// One frame. Returns the mix of low, band and high pass that `taps` asks for.
    fn next(&mut self, factors: &Factors, [low, band, high]: [f32; 3], input: f32) -> f32 {
        let Factors { k, a1, a2, a3 } = *factors;
        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        let high_pass = input - k * v1 - v2;
        low * v2 + band * k * v1 + high * high_pass
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

/// The saturation: clean up to [`KNEE`], then a soft bend that never passes `KNEE + BEND`.
/// Its slope is 1 at the knee on both sides, so the bend starts without a corner.
fn saturate(sample: f32) -> f32 {
    let size = sample.abs();
    if size <= KNEE {
        return sample;
    }
    (KNEE + BEND * ((size - KNEE) / BEND).tanh()).copysign(sample)
}

/// A sample of the input as the filter takes it: held to [`INPUT_LIMIT`], and silence for
/// anything that is not a number.
fn held(sample: f32) -> f32 {
    if sample.is_nan() {
        return 0.0;
    }
    sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
}

pub struct Filter {
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    /// The cutoff as `log2` of hertz, so a glide and the LFO move it in octaves.
    octaves: Smoothed,
    resonance: Smoothed,
    /// 0 is 12 dB per octave, 1 is 24 dB.
    slope: Smoothed,
    /// The weights of low, band and high pass.
    taps: [Smoothed; 3],
    /// The gain into the saturation, as a factor.
    drive: Smoothed,
    mix: Smoothed,
    lfo_depth: Smoothed,
    lfo_rate_hz: f32,
    /// In cycles, from 0 to 1. It starts at 0 when the filter is made, so a render is the
    /// same every time.
    lfo_phase: f32,
    /// Whether the factors have to be worked out again although nothing glides: after an
    /// update that snapped, and before the first block.
    stale: bool,
    factors: [Factors; 2],
    /// The two sections of each channel, left first.
    sections: [[Section; 2]; CHANNELS],
}

impl Filter {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Starts at these values, so a filter that is added or opened does not glide in.
    pub fn new(state: FilterState) -> Self {
        let mut filter = Self {
            sample_rate: 48_000.0,
            ramp_frames: 1.0,
            octaves: Smoothed::new(0.0),
            resonance: Smoothed::new(0.0),
            slope: Smoothed::new(0.0),
            taps: [0.0; 3].map(Smoothed::new),
            drive: Smoothed::new(1.0),
            mix: Smoothed::new(1.0),
            lfo_depth: Smoothed::new(0.0),
            lfo_rate_hz: state.lfo_rate_hz,
            lfo_phase: 0.0,
            stale: true,
            factors: [Factors::default(); 2],
            sections: [[Section::default(); 2]; CHANNELS],
        };
        filter.aim(&state);
        filter.snap();
        filter
    }

    /// Sets every target from a record.
    fn aim(&mut self, state: &FilterState) {
        let ramp = self.ramp_frames;
        self.octaves.set_target(state.cutoff_hz.log2(), ramp);
        self.resonance.set_target(state.resonance, ramp);
        self.slope.set_target(slope_weight(state.slope), ramp);
        for (tap, target) in self.taps.iter_mut().zip(taps(state.kind)) {
            tap.set_target(target, ramp);
        }
        self.drive
            .set_target(10_f32.powf(state.drive_db / 20.0), ramp);
        self.mix.set_target(state.mix, ramp);
        self.lfo_depth.set_target(state.lfo_depth_octaves, ramp);
        self.lfo_rate_hz = state.lfo_rate_hz;
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        let [low, band, high] = &mut self.taps;
        [
            &mut self.octaves,
            &mut self.resonance,
            &mut self.slope,
            low,
            band,
            high,
            &mut self.drive,
            &mut self.mix,
            &mut self.lfo_depth,
        ]
        .into_iter()
    }

    /// Takes every target at once. For a filter nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        self.smoothers().for_each(Smoothed::snap);
        self.stale = true;
    }

    /// Moves the cutoff, the resonance, the slope and the LFO `frames` along, and works out the
    /// factors for where they are when anything of them moves.
    fn move_factors(&mut self, frames: usize) {
        let changes = self.stale
            || self.octaves.is_moving()
            || self.resonance.is_moving()
            || self.slope.is_moving()
            || self.lfo_depth.is_moving()
            || self.lfo_depth.current() != 0.0;
        let cutoff = self.octaves.advance(frames);
        let resonance = self.resonance.advance(frames);
        let slope = self.slope.advance(frames);
        let depth = self.lfo_depth.advance(frames);
        let lfo = (TAU * self.lfo_phase).sin();
        let step = frames as f32 * self.lfo_rate_hz / self.sample_rate;
        self.lfo_phase = (self.lfo_phase + step).fract();
        if !changes {
            return;
        }
        self.stale = false;
        let hz = usable_hz((cutoff + depth * lfo).exp2(), self.sample_rate);
        let g = (PI * hz / self.sample_rate).tan();
        let [first, second] = section_q(resonance, slope);
        self.factors = [Factors::new(g, first), Factors::new(g, second)];
    }

    fn is_resting(&self) -> bool {
        self.sections.iter().flatten().all(Section::is_silent)
    }
}

impl Processor for Filter {
    type Update = FilterState;

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

    fn update(&mut self, update: &mut FilterState) {
        self.aim(update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let frames = context.frames;
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = left_in.iter().chain(right_in).all(|sample| *sample == 0.0);
        if silent_input && self.is_resting() {
            // Nothing sounds and nothing rings: no glide can be heard, and the output is
            // already silent. The LFO goes on, so where it is does not depend on the silence.
            self.snap();
            let step = frames as f32 * self.lfo_rate_hz / self.sample_rate;
            self.lfo_phase = (self.lfo_phase + step).fract();
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let chunks = left_in
            .chunks(FACTOR_FRAMES)
            .zip(right_in.chunks(FACTOR_FRAMES))
            .zip(left_out.chunks_mut(FACTOR_FRAMES))
            .zip(right_out.chunks_mut(FACTOR_FRAMES));
        for (((left_in, right_in), left_out), right_out) in chunks {
            self.move_factors(left_in.len());
            let (factors, slope) = (self.factors, self.slope.current());
            let frames = left_in
                .iter()
                .zip(right_in)
                .zip(left_out.iter_mut())
                .zip(right_out.iter_mut());
            for (((left_in, right_in), left_out), right_out) in frames {
                let taps = self.taps.each_mut().map(|tap| tap.advance(1));
                let drive = self.drive.advance(1);
                let mix = self.mix.advance(1);
                let [left, right] = &mut self.sections;
                for (sections, input, output) in
                    [(left, left_in, left_out), (right, right_in, right_out)]
                {
                    let dry = held(*input);
                    let driven = saturate(dry * drive);
                    let [first, second] = sections;
                    let one = first.next(&factors[0], taps, driven);
                    let two = second.next(&factors[1], taps, one);
                    let wet = one + slope * (two - one);
                    *output = dry + mix * (wet - dry);
                }
            }
        }
        if silent_input {
            self.sections.iter_mut().flatten().for_each(Section::settle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cutoff_is_at_minus_three_db_for_both_slopes_at_resonance_zero() {
        for slope in Slope::ALL {
            let state = FilterState {
                resonance: 0.0,
                slope,
                ..FilterState::default()
            };
            let db = 20.0 * response(&state, state.cutoff_hz, 48_000.0).log10();
            assert!((db + 3.0103).abs() < 0.001, "{slope:?}: {db}");
        }
    }

    #[test]
    fn full_resonance_peaks_at_max_q_on_one_section() {
        let state = FilterState {
            resonance: 1.0,
            ..FilterState::default()
        };
        let peak = response(&state, state.cutoff_hz, 48_000.0);
        assert!((peak - MAX_Q).abs() < 0.001, "{peak}");
    }
}
