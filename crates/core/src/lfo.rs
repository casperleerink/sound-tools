//! A sine LFO, for a processor that moves one of its values up and down over time: the cutoff
//! of a filter, the delay of a chorus. Next to [`Smoothed`](crate::Smoothed), a helper a
//! processor uses per block or per frame.

use std::f32::consts::TAU;

/// Where a sine LFO is, in cycles from 0 to 1. It starts at 0 when it is made, so a render is
/// the same every time. The rate is the caller's: it may change at any time, and the phase goes
/// on from where it is, so a new rate never jumps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lfo {
    phase: f32,
}

impl Lfo {
    /// The value now, from -1 to 1, of the sine `offset` cycles behind: another channel of the
    /// same LFO, such as the right side of a stereo chorus.
    #[inline]
    pub fn value(&self, offset: f32) -> f32 {
        (TAU * (self.phase - offset)).sin()
    }

    /// Moves `frames` along at `hz`.
    #[inline]
    pub fn advance(&mut self, frames: usize, hz: f32, sample_rate: f32) {
        let step = frames as f32 * hz / sample_rate;
        self.phase = (self.phase + step).fract();
    }
}

#[cfg(test)]
mod tests {
    use super::Lfo;

    #[test]
    fn it_starts_at_zero_and_rises_to_one_a_quarter_cycle_later() {
        let mut lfo = Lfo::default();
        assert_eq!(lfo.value(0.0), 0.0);
        lfo.advance(12_000, 1.0, 48_000.0);
        assert!((lfo.value(0.0) - 1.0).abs() < 1e-6);
        // Half a cycle behind is the other side.
        assert!((lfo.value(0.5) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_whole_cycle_comes_back_to_the_start() {
        let mut lfo = Lfo::default();
        for _ in 0..100 {
            lfo.advance(480, 1.0, 48_000.0);
        }
        assert!(lfo.value(0.0).abs() < 1e-4);
    }
}
