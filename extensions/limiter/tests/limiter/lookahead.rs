//! The lookahead: it is the latency the limiter reports, also after a change, and the gain is
//! down along a straight line when a peak arrives, so the top of the wave keeps its shape.

use limiter::{LimiterState, Lookahead};

use crate::support::{Rig, SAMPLE_RATE, burst, steady};

#[test]
fn the_latency_is_the_lookahead_in_frames_and_follows_a_change() {
    for sample_rate in [44_100, 48_000, 96_000] {
        let mut rig = Rig::at(sample_rate, LimiterState::default(), steady(0.1));
        for lookahead in [Lookahead::Five, Lookahead::Off, Lookahead::One] {
            rig.update(LimiterState {
                lookahead,
                ..LimiterState::default()
            });
            let frames = lookahead.frames(sample_rate as f32) as u64;
            assert_eq!(rig.latency(), frames, "{sample_rate} Hz, {lookahead:?}");
        }
    }
    assert_eq!(Lookahead::Five.frames(SAMPLE_RATE as f32), 240);
}

/// A jump from 0.25 to 2.5 under a ceiling of 0 dB. With 5 ms of lookahead the output is
/// untouched until one lookahead before the jump, then comes down along a straight line, and
/// the jump arrives at the ceiling. Without lookahead the first loud frame is cut at the ceiling
/// and nothing before it moves.
#[test]
fn with_lookahead_the_gain_is_down_when_the_peak_arrives() {
    let at = 4_800;
    let render = |lookahead| {
        let state = LimiterState {
            ceiling_db: 0.0,
            lookahead,
            ..LimiterState::default()
        };
        let [output, _] = Rig::new(state, burst(0.25, at, 4_800, 2.5)).render(3 * at);
        output
    };
    let ahead = render(Lookahead::Five);
    let delay = 240;
    let jump = at + delay;
    assert_eq!(ahead[jump - delay - 1], 0.25);
    // The gain goes from 1 to 0.4 in 240 equal steps, so the quiet sound falls by 0.6 / 240 of
    // 0.25 per frame, and arrives at 0.4 of itself.
    let steps = ahead[jump - delay..jump]
        .windows(2)
        .map(|pair| pair[0] - pair[1]);
    let largest = steps.fold(0.0_f32, f32::max);
    assert!(largest < 0.25 * 0.6 / 240.0 * 1.01, "{largest}");
    assert!(
        (ahead[jump - 1] - 0.25 * 0.4).abs() < 1e-3,
        "{}",
        ahead[jump - 1]
    );
    assert!((ahead[jump] - 1.0).abs() < 1e-6, "{}", ahead[jump]);

    let plain = render(Lookahead::Off);
    assert_eq!(plain[at - 1], 0.25);
    assert!(1.0 - plain[at] < 1e-6, "{}", plain[at]);
}
