//! An LFO, for a processor that moves one of its values up and down over time: the cutoff of a
//! filter, the delay of a chorus. Next to [`Smoothed`](crate::Smoothed), a helper a processor
//! uses per block or per frame.

use std::f32::consts::TAU;

use crate::Transport;

/// The wave of an LFO, from -1 to 1. The caller's, like the rate: it may change at any time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LfoShape {
    /// Starts at 0 and rises to 1 a quarter cycle later.
    #[default]
    Sine,
    /// The sine in straight lines: 0, 1, 0, -1 at each quarter cycle.
    Triangle,
    /// Rises from -1 at the start of the cycle to 1 at its end.
    SawUp,
    /// Falls from 1 at the start of the cycle to -1 at its end.
    SawDown,
    /// 1 for the first half of the cycle and -1 for the second.
    Square,
    /// A new random level at the start of every cycle, held until the next. The level follows
    /// from the seed of the LFO and the count of the cycle, so a render is the same every time.
    SampleAndHold,
}

/// Where an LFO is, in cycles from 0 to 1. It starts at 0 when it is made, so a render is the
/// same every time. The rate is the caller's: it may change at any time, and the phase goes on
/// from where it is, so a new rate never jumps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lfo {
    phase: f32,
    /// Whole cycles so far, for the level of a sample and hold. It wraps.
    cycle: u32,
    seed: u32,
}

impl Lfo {
    /// An LFO whose sample and hold levels follow from `seed`. Give LFOs that should not move
    /// together, such as those of two voices, different seeds. `default` has seed 0.
    pub fn seeded(seed: u32) -> Self {
        Self {
            seed,
            ..Self::default()
        }
    }

    /// The value now, from -1 to 1, of `shape` `offset` cycles behind: another channel of the
    /// same LFO, such as the right side of a stereo chorus.
    #[inline]
    pub fn value(&self, shape: LfoShape, offset: f32) -> f32 {
        let at = self.phase - offset;
        let whole = at.floor();
        let phase = at - whole;
        match shape {
            LfoShape::Sine => (TAU * at).sin(),
            LfoShape::Triangle => 1.0 - 4.0 * ((phase + 0.25).fract() - 0.5).abs(),
            LfoShape::SawUp => 2.0 * phase - 1.0,
            LfoShape::SawDown => 1.0 - 2.0 * phase,
            LfoShape::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::SampleAndHold => {
                random_level(self.seed, self.cycle.wrapping_add_signed(whole as i32))
            }
        }
    }

    /// Moves `frames` along at `hz`.
    #[inline]
    pub fn advance(&mut self, frames: usize, hz: f32, sample_rate: f32) {
        let step = frames as f32 * hz / sample_rate;
        let next = self.phase + step;
        self.phase = next.fract();
        self.cycle = self.cycle.wrapping_add(next as u32);
    }

    /// For a cycle `quarters` quarter notes long, more than 0: the rate in Hz at the tempo
    /// where this block starts, for [`advance`](Self::advance). Call it at the start of every
    /// block.
    ///
    /// While the project plays it also puts the phase where the project is: a cycle starts on
    /// every multiple of `quarters` from the project start, the same on every play, and the
    /// sample and hold levels are the same at the same place. While it is stopped the LFO runs
    /// on at the rate.
    pub fn sync(&mut self, transport: &Transport, quarters: f32) -> f32 {
        let quarters = f64::from(quarters);
        if let Some(position) = transport.quarters() {
            let cycles = position / quarters;
            let whole = cycles.floor();
            let phase = (cycles - whole) as f32;
            // Just below a whole cycle can round up to it in `f32`.
            let (whole, phase) = if phase < 1.0 {
                (whole, phase)
            } else {
                (whole + 1.0, 0.0)
            };
            self.phase = phase;
            self.cycle = whole as i64 as u32;
        }
        let bpm = transport.clock.tempo_at(transport.tick_range.start).bpm();
        (bpm / 60.0 / quarters) as f32
    }
}

/// A level from -1 to 1 for each seed and cycle, always the same: the SplitMix64 mix of the
/// two.
fn random_level(seed: u32, cycle: u32) -> f32 {
    let mut mixed = (u64::from(seed) << 32 | u64::from(cycle)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^= mixed >> 31;
    // The top 24 bits, which an `f32` holds exactly, from 0 to 2 and then down by 1.
    (mixed >> 40) as f32 / (1 << 23) as f32 - 1.0
}

#[cfg(test)]
mod tests {
    use super::{Lfo, LfoShape};

    #[test]
    fn it_starts_at_zero_and_rises_to_one_a_quarter_cycle_later() {
        let mut lfo = Lfo::default();
        assert_eq!(lfo.value(LfoShape::Sine, 0.0), 0.0);
        lfo.advance(12_000, 1.0, 48_000.0);
        assert!((lfo.value(LfoShape::Sine, 0.0) - 1.0).abs() < 1e-6);
        // Half a cycle behind is the other side.
        assert!((lfo.value(LfoShape::Sine, 0.5) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_whole_cycle_comes_back_to_the_start() {
        let mut lfo = Lfo::default();
        for _ in 0..100 {
            lfo.advance(480, 1.0, 48_000.0);
        }
        assert!(lfo.value(LfoShape::Sine, 0.0).abs() < 1e-4);
    }
}
