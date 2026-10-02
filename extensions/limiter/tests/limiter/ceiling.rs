//! Nothing goes over the ceiling: at any setting, on any sound, at any sample rate and buffer
//! size, and through edits while it plays. A level far over it comes out at the ceiling, and
//! what is not a number is silence.

use limiter::{CEILING, GAIN, LimiterState, Lookahead, RELEASE};
use proptest::prelude::*;

use crate::support::{Rig, SAMPLE_RATE, Signal, amplitude, noise, peak, sine, steady};

fn lookahead(index: usize) -> Lookahead {
    Lookahead::ALL[index % Lookahead::ALL.len()]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn no_sample_ever_goes_over_the_ceiling(
        gain_db in GAIN.min..=GAIN.max,
        ceiling_db in CEILING.min..=CEILING.max,
        release_ms in RELEASE.min..=RELEASE.max,
        first_lookahead in 0..3_usize,
        level in 0.01_f32..8.0,
        seed: u64,
        second_ceiling_db in CEILING.min..=CEILING.max,
        second_gain_db in GAIN.min..=GAIN.max,
        second_lookahead in 0..3_usize,
        sample_rate in prop::sample::select(vec![44_100_u32, 48_000, 96_000]),
        block in 1_usize..600,
    ) {
        let first = LimiterState {
            gain_db,
            ceiling_db,
            release_ms,
            lookahead: lookahead(first_lookahead),
        };
        let mut rig = Rig::at(sample_rate, first, noise(level, seed));
        let frames = sample_rate as usize / 10;
        let [left, right] = rig.render_in_blocks(frames, block);
        let most = peak(&left).max(peak(&right));
        prop_assert!(most <= amplitude(ceiling_db), "{most} over {ceiling_db} dB");
        // Another ceiling, gain and lookahead while it plays: from the next frame on the new
        // ceiling holds, also for the sound that was already in the lookahead.
        let second = LimiterState {
            gain_db: second_gain_db,
            ceiling_db: second_ceiling_db,
            lookahead: lookahead(second_lookahead),
            ..first
        };
        rig.update(second);
        let [left, right] = rig.render_in_blocks(frames, block);
        let most = peak(&left).max(peak(&right));
        prop_assert!(most <= amplitude(second_ceiling_db), "{most} over {second_ceiling_db} dB");
    }
}

/// A steady level 20 dB over the ceiling comes out at the ceiling exactly, in both channels.
#[test]
fn a_level_far_over_the_ceiling_comes_out_at_the_ceiling() {
    for lookahead in Lookahead::ALL {
        let state = LimiterState {
            ceiling_db: -6.0,
            lookahead,
            ..LimiterState::default()
        };
        let loud = amplitude(-6.0) * 10.0;
        let mut rig = Rig::new(state, steady(loud));
        let [left, right] = rig.render(SAMPLE_RATE as usize / 10);
        let ceiling = amplitude(-6.0);
        let delay = lookahead.frames(SAMPLE_RATE as f32);
        assert!(peak(&left) <= ceiling, "{lookahead:?}");
        let settled = &left[delay + 10..];
        assert!(
            settled.iter().all(|sample| (ceiling - sample).abs() < 1e-6),
            "{lookahead:?}: {:?}",
            settled
                .iter()
                .find(|sample| (ceiling - *sample).abs() >= 1e-6)
        );
        assert_eq!(left, right);
    }
}

/// The gain pushes the sound into the ceiling: a sine 12 dB under it with 18 dB of gain comes
/// out with its peaks at the ceiling, 6 dB louder than it went in.
#[test]
fn the_gain_makes_a_quiet_sound_louder_up_to_the_ceiling() {
    let state = LimiterState {
        gain_db: 18.0,
        ceiling_db: -1.0,
        ..LimiterState::default()
    };
    let mut rig = Rig::new(state, sine(100.0, amplitude(-13.0)));
    let [left, _] = rig.render(SAMPLE_RATE as usize);
    let tail = peak(&left[SAMPLE_RATE as usize / 2..]);
    assert!(tail <= amplitude(-1.0));
    assert!(tail > amplitude(-1.05), "{tail}");
}

/// A sample that is not a number is silence to the limiter, and an infinite one is held to
/// +36 dBFS. The output stays under the ceiling and the sound goes on as it would have.
#[test]
fn input_that_is_not_a_number_or_infinite_does_not_reach_the_output() {
    let mut frame = 0_usize;
    let mut clean = sine(440.0, 0.5);
    let broken: Signal = Box::new(move || {
        frame += 1;
        let sample = clean();
        match frame {
            1_000 => [f32::NAN, f32::INFINITY],
            2_000 => [f32::NEG_INFINITY, f32::NAN],
            3_000 => [f32::MAX, -f32::MAX],
            _ => sample,
        }
    });
    let state = LimiterState {
        release_ms: RELEASE.min,
        ..LimiterState::default()
    };
    let [left, right] = Rig::new(state, broken).render(SAMPLE_RATE as usize);
    let ceiling = amplitude(state.ceiling_db);
    for sample in left.iter().chain(&right) {
        assert!(sample.is_finite() && sample.abs() <= ceiling, "{sample}");
    }
    // After the release, the sine is where it is with clean input all along.
    let [clean, _] = Rig::new(state, sine(440.0, 0.5)).render(SAMPLE_RATE as usize);
    let tail = 9 * SAMPLE_RATE as usize / 10;
    assert_eq!(left[tail..], clean[tail..]);
}

/// The same sound through the same limiter gives the same samples in any buffer size.
#[test]
fn the_buffer_size_changes_nothing() {
    let state = LimiterState {
        gain_db: 12.0,
        lookahead: Lookahead::Five,
        ..LimiterState::default()
    };
    let render = |block| Rig::new(state, noise(0.5, 7)).render_in_blocks(24_000, block);
    let reference = render(480);
    for block in [1, 7, 64, 511] {
        assert!(render(block) == reference, "{block}");
    }
}
