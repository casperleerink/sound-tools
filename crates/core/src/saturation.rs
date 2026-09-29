//! A soft clip curve, for a processor that drives a sound into saturation: the drive of the
//! Filter and the `clip` curve of the Saturator. Next to [`Smoothed`](crate::Smoothed), a helper
//! a processor uses per frame. The core itself shapes nothing.

/// The curve is exactly clean up to full scale, then bends softly towards `KNEE + BEND`.
const KNEE: f32 = 1.0;
const BEND: f32 = 0.5;

/// Clean up to full scale, then a soft bend that never passes 1.5. Its slope is 1 at the knee
/// on both sides, so the bend starts without a corner. A gain in front of it is a drive: up to
/// full scale nothing changes, and turning it up never makes the sound louder than the curve
/// allows.
pub fn soft_clip(sample: f32) -> f32 {
    let size = sample.abs();
    if size <= KNEE {
        return sample;
    }
    (KNEE + BEND * ((size - KNEE) / BEND).tanh()).copysign(sample)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_clean_up_to_full_scale_and_never_passes_one_and_a_half() {
        for sample in [0.0, 0.25, -0.5, 1.0, -1.0] {
            assert_eq!(soft_clip(sample), sample);
        }
        for sample in [1.01, 2.0, 100.0, f32::MAX] {
            let clipped = soft_clip(sample);
            assert!(clipped > 1.0 && clipped <= 1.5, "{sample}: {clipped}");
            assert_eq!(soft_clip(-sample), -clipped);
        }
        // No corner at the knee: the slope just past it is 1.
        let slope = (soft_clip(1.0 + 1e-3) - 1.0) / 1e-3;
        assert!((slope - 1.0).abs() < 1e-2, "{slope}");
    }
}
