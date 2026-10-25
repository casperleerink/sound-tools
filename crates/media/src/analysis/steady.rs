//! The pitch of a voice without its vibrato, to choose the note it is moved to.
//!
//! Once a step, the middle of the periods read in the step goes through a cut of 40 dB around
//! 6 Hz, where a vibrato is. The steady pitch is a blend of that and of the reading. It starts
//! at the reading when the pitch is found and slides to the cut one, half way in
//! [`SETTLE_SECONDS`]. A jump, such as a new note, moves it back towards the reading, so the
//! cut does not ring after it.

use std::f64::consts::{PI, TAU};

use super::Biquad;

/// A step is 200 samples at 44.1 kHz, about 4.5 ms, at any rate.
const STEP_SAMPLES: f64 = 200.0;
const STEP_RATE: f64 = 44_100.0;
const CUT_HZ: f64 = 6.0;
const CUT_Q: f64 = 3.0;
/// The amplitude the cut leaves: 40 dB under.
const CUT_GAIN: f64 = 0.1;
const SETTLE_SECONDS: f64 = 0.2;
/// A jump is a change of the low-passed period, per step, of more than this part of it per
/// second. Each jump takes the blend this much back to the reading.
const JUMP_PER_SECOND: f64 = 4.0;
const JUMP_BLEND: f64 = 0.2;

/// Follows the period of a voice, sample by sample, and keeps how far its steady pitch is from
/// it.
pub struct SteadyPitch {
    /// Samples per step, and the samples heard since the last step.
    step: f64,
    count: usize,
    /// The shortest and the longest period read in this step.
    range: Option<(f64, f64)>,
    cut: Biquad,
    /// A low pass at an eighth of the rate of the steps, which finds jumps, and its last
    /// output.
    motion: Biquad,
    moving: f64,
    /// Whether a step has been heard since the pitch was found.
    started: bool,
    /// How much of the reading is in the steady pitch, 1 to 0, and the part of it each step
    /// keeps.
    blend: f64,
    keep: f64,
    jump: f64,
    /// The steady pitch over the pitch read, in semitones, as of the last step.
    offset: f64,
}

impl SteadyPitch {
    pub fn new(rate: f64) -> Self {
        let step = STEP_SAMPLES * rate / STEP_RATE + 0.1;
        let steps_per_second = rate / step;
        let cut = {
            let (sin, cos) = (TAU * CUT_HZ / steps_per_second).sin_cos();
            let alpha = sin / (2.0 * CUT_Q);
            let a0 = 1.0 + alpha / CUT_GAIN;
            Biquad::new(
                [
                    (1.0 + alpha * CUT_GAIN) / a0,
                    -2.0 * cos / a0,
                    (1.0 - alpha * CUT_GAIN) / a0,
                ],
                [-2.0 * cos / a0, (1.0 - alpha / CUT_GAIN) / a0],
            )
        };
        let motion = {
            let (sin, cos) = (PI / 4.0).sin_cos();
            let alpha = sin / std::f64::consts::SQRT_2;
            let a0 = 1.0 + alpha;
            let side = (1.0 - cos) / 2.0 / a0;
            Biquad::new(
                [side, (1.0 - cos) / a0, side],
                [-2.0 * cos / a0, (1.0 - alpha) / a0],
            )
        };
        Self {
            step,
            count: 0,
            range: None,
            cut,
            motion,
            moving: 0.0,
            started: false,
            blend: 1.0,
            keep: 0.5_f64.powf(1.0 / (SETTLE_SECONDS * steps_per_second)),
            jump: JUMP_PER_SECOND / steps_per_second,
            offset: 0.0,
        }
    }

    /// Hears one sample: the period read, in samples, or `None` once the pitch is lost, which
    /// starts again.
    pub fn push(&mut self, period: Option<f64>) {
        match period {
            Some(period) => {
                let (shortest, longest) = self.range.unwrap_or((period, period));
                self.range = Some((shortest.min(period), longest.max(period)));
            }
            None => self.lose(),
        }
        self.count += 1;
        if (self.count as f64) < self.step {
            return;
        }
        self.count = 0;
        match self.range.take() {
            Some((shortest, longest)) => {
                let period = (shortest + longest) / 2.0;
                self.offset = -12.0 * (self.smooth(period) / period).log2();
            }
            None => self.lose(),
        }
    }

    /// How far the steady pitch is from the pitch read, in semitones.
    pub fn offset(&self) -> f64 {
        self.offset
    }

    fn lose(&mut self) {
        (self.range, self.started, self.offset) = (None, false, 0.0);
    }

    fn smooth(&mut self, period: f64) -> f64 {
        if !self.started {
            self.started = true;
            self.cut.settle(period);
            self.motion.settle(period);
            self.moving = period;
            self.blend = 1.0;
        }
        let steady = period + (1.0 - self.blend) * (self.cut.process(period) - period);
        self.blend *= self.keep;
        let moving = self.motion.process(period);
        if (moving - self.moving).abs() > self.jump * moving {
            self.blend = (self.blend + JUMP_BLEND).min(1.0);
        }
        self.moving = moving;
        steady
    }
}
