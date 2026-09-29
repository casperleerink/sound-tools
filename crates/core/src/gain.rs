//! Gain in decibels and the pan law, the two sums every volume and pan of the SDK shares: the
//! track mixer, a pad of the Drum pad and the Utility. One place, so a pan of 0.3 means the same
//! wherever a composer turns it.

use std::f64::consts::{FRAC_PI_2, SQRT_2};

use crate::processor::CHANNELS;

/// The factor a gain in decibels multiplies the samples by: 0 dB is exactly 1 and `-inf`
/// exactly 0.
pub fn amplitude(db: f32) -> f32 {
    // In f64, so that 0 dB is exactly 1 and leaves every sample as it was.
    10_f64.powf(f64::from(db) / 20.0) as f32
}

/// The gain of each channel, left first, for a `level` (a factor) and a `pan` from -1 (left) to
/// 1 (right).
///
/// The pan law is equal power: the two gains are the sine and the cosine of a quarter turn,
/// scaled so that the middle is exactly 1. A sound keeps its loudness wherever it is panned, and
/// in the middle it is untouched. Hard left or right, the channel that plays it is √2, which is
/// 3 dB above the middle, and the other is exactly 0.
pub fn pan_gains(level: f64, pan: f32) -> [f32; CHANNELS] {
    let pan = f64::from(pan);
    // The part of the quarter turn each channel gets. Both are a sine, so a channel that is
    // panned away is exactly 0 and not a rounding of it.
    let parts = [(1.0 - pan) / 2.0, (1.0 + pan) / 2.0];
    parts.map(|part| (level * SQRT_2 * (part * FRAC_PI_2).sin()) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_db_is_exactly_one_and_minus_inf_exactly_zero() {
        assert_eq!(amplitude(0.0), 1.0);
        assert_eq!(amplitude(f32::NEG_INFINITY), 0.0);
        assert!((amplitude(-6.0) - 0.501_187).abs() < 1e-6);
    }

    #[test]
    fn the_middle_is_exactly_one_and_the_sides_keep_the_power() {
        assert_eq!(pan_gains(1.0, 0.0), [1.0, 1.0]);
        assert_eq!(pan_gains(1.0, -1.0), [std::f32::consts::SQRT_2, 0.0]);
        assert_eq!(pan_gains(1.0, 1.0), [0.0, std::f32::consts::SQRT_2]);
        for pan in [-0.7, -0.2, 0.4, 0.9] {
            let [left, right] = pan_gains(1.0, pan);
            assert!((left * left + right * right - 2.0).abs() < 1e-5, "{pan}");
        }
    }
}
