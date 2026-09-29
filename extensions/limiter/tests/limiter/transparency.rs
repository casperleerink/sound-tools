//! Under the ceiling the limiter does nothing: every sample comes out as it went in, one
//! lookahead later, and after a peak the gain comes back to exactly 1.

use limiter::{LimiterState, Lookahead, RELEASE};

use crate::support::{Rig, SAMPLE_RATE, burst, noise, peak, sine, take};

/// Every sample to the bit, only later, for every lookahead and at the ceiling itself.
#[test]
fn under_the_ceiling_the_sound_is_only_delayed() {
    for lookahead in Lookahead::ALL {
        let state = LimiterState {
            lookahead,
            ..LimiterState::default()
        };
        let delay = lookahead.frames(SAMPLE_RATE as f32);
        let frames = SAMPLE_RATE as usize / 10;
        for signal in [|| sine(440.0, 0.89), || noise(0.5, 3)] {
            let [output, _] = Rig::new(state, signal()).render(frames);
            let input = take(signal(), frames);
            assert!(output[..delay].iter().all(|sample| *sample == 0.0));
            assert_eq!(&output[delay..], &input[..frames - delay], "{lookahead:?}");
        }
    }
}

/// A burst over the ceiling, then the quiet sound again: once the release has let go, the
/// quiet sound comes out bit for bit and the meter says no reduction. A gain that stopped just
/// under 1 would scale every sample for the rest of the session.
#[test]
fn after_a_peak_the_gain_comes_back_to_exactly_one() {
    for sample_rate in [44_100, 48_000, 96_000] {
        for release_ms in [RELEASE.min, 100.0, RELEASE.max] {
            for lookahead in Lookahead::ALL {
                let case = format!("{sample_rate} Hz, {release_ms} ms, {lookahead:?}");
                let state = LimiterState {
                    release_ms,
                    lookahead,
                    ..LimiterState::default()
                };
                let quiet = 0.01;
                let rate = sample_rate as usize;
                let mut rig = Rig::at(sample_rate, state, burst(quiet, rate / 10, 100, 4.0));
                // The burst, and ten times the release after it.
                let frames = rate / 5 + (10.0 * release_ms / 1_000.0 * rate as f32) as usize;
                let [hit, _] = rig.render(frames);
                assert!(peak(&hit) > 0.8, "{case}: the burst was not there");
                assert!(rig.meters.reduction.take()[0] > 1.0, "{case}: no reduction");
                let [after, _] = rig.render(4_800);
                assert!(
                    after.iter().all(|sample| *sample == quiet),
                    "{case}: {:?}",
                    after.iter().find(|sample| **sample != quiet)
                );
                assert_eq!(rig.meters.reduction.take(), [0.0, 0.0], "{case}");
            }
        }
    }
}
