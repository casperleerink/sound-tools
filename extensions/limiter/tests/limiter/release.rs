//! The release: after a peak the gain comes back along its time constant, 63 % of the way in
//! the release time.

use limiter::{LimiterState, Lookahead};

use crate::support::{Rig, SAMPLE_RATE, burst};

/// A steady 0.25 with a burst of 2.5 under a ceiling of 0 dB: the burst takes the gain down to
/// 0.4, and after it the gain is `1 - 0.6 e^(-t / release)`, so the output shows it.
#[test]
fn the_gain_comes_back_along_the_release() {
    for release_ms in [10.0, 100.0, 1_000.0] {
        let state = LimiterState {
            ceiling_db: 0.0,
            release_ms,
            lookahead: Lookahead::Off,
            ..LimiterState::default()
        };
        let (from, length) = (4_800, 480);
        let mut rig = Rig::new(state, burst(0.25, from, length, 2.5));
        let release_frames = (release_ms / 1_000.0 * SAMPLE_RATE as f32) as usize;
        let [output, _] = rig.render(from + length + 3 * release_frames);
        // At the ceiling through the burst.
        assert!(output[from..from + length].iter().all(|s| *s == 1.0));
        let after = from + length;
        for time_constants in [1.0_f32, 2.0] {
            let frames = (time_constants * release_frames as f32) as usize;
            let expected = 0.25 * (1.0 - 0.6 * (-time_constants).exp());
            let heard = output[after + frames - 1];
            assert!(
                (heard / expected - 1.0).abs() < 1e-3,
                "{release_ms} ms, {time_constants} time constants: {heard} {expected}"
            );
        }
    }
}

/// A longer release keeps the gain down longer between peaks: the same bursts come out with
/// less of the quiet sound between them.
#[test]
fn a_longer_release_holds_the_gain_down_longer() {
    let quiet_after = |release_ms| {
        let state = LimiterState {
            release_ms,
            ..LimiterState::default()
        };
        let mut rig = Rig::new(state, burst(0.25, 4_800, 480, 2.5));
        let [output, _] = rig.render(9_600);
        output[9_000]
    };
    assert!(quiet_after(10.0) > quiet_after(100.0));
    assert!(quiet_after(100.0) > quiet_after(1_000.0));
}
