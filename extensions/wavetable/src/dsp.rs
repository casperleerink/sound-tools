//! Small curves the voices use per sample, and the Fold Sweep table uses to build its frames.

use std::f32::consts::TAU;

/// A wavefolder: the sound as it is between -1 and 1, and mirrored back at those edges past
/// them, again and again. So a louder sound folds into more and more bends and does not get
/// louder.
#[inline]
pub(crate) fn fold(sample: f32) -> f32 {
    1.0 - ((sample + 1.0).rem_euclid(4.0) - 2.0).abs()
}

/// `sin(2π phase)` for a phase in cycles, within 1e-5 of it: a polynomial after folding the
/// phase into the quarter cycle around 0. A few times faster than `f32::sin`, and a voice needs
/// one per sample for its sub and its FM.
#[inline]
pub(crate) fn sine(phase: f32) -> f32 {
    let mut at = phase - phase.round();
    if at > 0.25 {
        at = 0.5 - at;
    } else if at < -0.25 {
        at = -0.5 - at;
    }
    let x = TAU * at;
    let square = x * x;
    // The Taylor series to x⁹: under 1e-5 off within a quarter cycle of 0.
    x * (1.0
        + square
            * (-1.0 / 6.0
                + square * (1.0 / 120.0 + square * (-1.0 / 5_040.0 + square / 362_880.0))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fold_is_the_sound_between_the_edges_and_mirrored_past_them() {
        for (input, output) in [
            (0.0, 0.0),
            (0.5, 0.5),
            (1.0, 1.0),
            (1.5, 0.5),
            (2.0, 0.0),
            (3.0, -1.0),
            (-1.5, -0.5),
            (5.0, 1.0),
        ] {
            assert!(
                (fold(input) - output).abs() < 1e-6,
                "{input}: {}",
                fold(input)
            );
        }
    }

    #[test]
    fn the_sine_is_within_a_hundred_thousandth() {
        for step in -4_000..=4_000 {
            let phase = step as f32 / 1_000.0;
            let error = (sine(phase) - (TAU * phase).sin()).abs();
            assert!(error < 1e-5, "{phase}: {error}");
        }
    }
}
