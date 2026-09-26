//! Every setting is stable: nothing that is not a number, the output is bounded by the makeup
//! gain, the compressor lets go completely and comes to rest, and a render is the same every
//! time.

use compressor::{ATTACK, CompressorState, KNEE, Lookahead, MAKEUP, RATIO, RELEASE, THRESHOLD};
use proptest::prelude::*;

use crate::support::{Rig, SAMPLE_RATE, Signal, noise, peak, sine};

/// The loudest the compressor makes a full scale input: the makeup gain at its top, +24 dB.
const BOUND: f32 = 15.9;

fn assert_bounded(label: &str, [left, right]: &[Vec<f32>; 2]) {
    for sample in left.iter().chain(right) {
        assert!(sample.is_finite(), "{label}: {sample}");
    }
    let loudest = peak(left).max(peak(right));
    assert!(loudest < BOUND, "{label}: peak {loudest}");
}

#[test]
fn the_ends_of_every_range_stay_bounded() {
    for threshold_db in [THRESHOLD.min, THRESHOLD.max] {
        for ratio in [RATIO.min, RATIO.max] {
            for (attack_ms, release_ms) in [(ATTACK.min, RELEASE.min), (ATTACK.max, RELEASE.max)] {
                for knee_db in [KNEE.min, KNEE.max] {
                    for lookahead in Lookahead::ALL {
                        let state = CompressorState {
                            threshold_db,
                            ratio,
                            attack_ms,
                            release_ms,
                            knee_db,
                            makeup_db: MAKEUP.max,
                            lookahead,
                            ..CompressorState::default()
                        };
                        let label = format!("{state:?}");
                        let mut rig = Rig::new(state, noise(1.0));
                        assert_bounded(&label, &rig.render(SAMPLE_RATE as usize / 4));
                    }
                }
            }
        }
    }
}

/// A sample that is not a number, or is infinite, is silence or the limit to the compressor.
/// It goes on as if nothing had happened.
#[test]
fn input_that_is_not_a_number_or_infinite_does_not_reach_the_output() {
    let mut frame = 0_usize;
    let mut clean = sine(440.0, 0.5);
    let broken: Signal = Box::new(move || {
        frame += 1;
        match frame {
            1_000 => [f32::NAN, f32::INFINITY],
            2_000 => [f32::NEG_INFINITY, f32::NAN],
            3_000 => [f32::MAX, -f32::MAX],
            4_000 => [f32::MIN_POSITIVE / 4.0, -f32::MIN_POSITIVE / 8.0],
            _ => clean(),
        }
    });
    let state = CompressorState {
        lookahead: Lookahead::Ten,
        ..CompressorState::default()
    };
    let output = Rig::new(state, broken).render(SAMPLE_RATE as usize);
    for sample in output.iter().flatten() {
        assert!(sample.is_finite());
    }
    // The limit is turned down hard, and after the release the sine is as loud as it is with
    // clean input all along.
    let [left, _] = &output;
    assert!(peak(left) < 64.0);
    let [clean, _] = Rig::new(state, sine(440.0, 0.5)).render(SAMPLE_RATE as usize);
    let tail = 9 * SAMPLE_RATE as usize / 10;
    let (heard, expected) = (peak(&left[tail..]), peak(&clean[tail..]));
    assert!((heard / expected - 1.0).abs() < 0.01, "{heard} {expected}");
}

/// After a loud part the reduction lets go completely: a quiet sound comes out exactly as it
/// went in, to the bit, so no reduction ever lingers in numbers too small to hear. And after
/// silence the output is exactly silent.
#[test]
fn after_the_loud_part_it_lets_go_completely_and_comes_to_rest() {
    const LOUD: usize = 4_800;
    const QUIET: usize = 5 * SAMPLE_RATE as usize;
    let mut frames = 0_usize;
    let mut loud = noise(1.0);
    let mut quiet = sine(440.0, 0.001);
    let signal: Signal = Box::new(move || {
        frames += 1;
        match frames {
            ..=LOUD => loud(),
            ..=QUIET => quiet(),
            _ => [0.0; 2],
        }
    });
    let state = CompressorState {
        threshold_db: -40.0,
        ratio: 20.0,
        release_ms: 200.0,
        ..CompressorState::default()
    };
    let mut rig = Rig::new(state, signal);
    let [output, _] = rig.render(QUIET);
    let mut quiet = sine(440.0, 0.001);
    let input: Vec<f32> = (LOUD..QUIET).map(|_| quiet()[0]).collect();
    // Right after the loud part the quiet sound is turned down.
    let (heard, given) = (&output[LOUD..], &input[..]);
    assert!(
        heard[..1_000]
            .iter()
            .zip(given)
            .any(|(out, inp)| out != inp)
    );
    // The reduction was 38 dB. At 0.0001 dB it has arrived, after 13 release times: from 3 s
    // on, the gain is exactly 1 again.
    let back = 3 * SAMPLE_RATE as usize - LOUD;
    assert!(
        heard[back..]
            .iter()
            .zip(&given[back..])
            .all(|(out, inp)| out == inp)
    );
    // Silence from here: the output is exactly silent.
    let [rest, other] = rig.render(SAMPLE_RATE as usize);
    assert!(rest.iter().chain(&other).all(|sample| *sample == 0.0));
}

#[test]
fn renders_are_the_same_every_time_to_the_byte() {
    let state = CompressorState {
        threshold_db: -30.0,
        ratio: 6.0,
        attack_ms: 3.0,
        release_ms: 80.0,
        knee_db: 9.0,
        makeup_db: 8.0,
        mix: 0.7,
        lookahead: Lookahead::One,
    };
    let render = || {
        let mut rig = Rig::new(state, noise(0.8));
        let first = rig.render(SAMPLE_RATE as usize);
        rig.update(CompressorState {
            lookahead: Lookahead::Ten,
            threshold_db: -12.0,
            ..state
        });
        let [left, right] = rig.render(SAMPLE_RATE as usize);
        let bytes = |samples: &[f32]| -> Vec<u8> {
            samples
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect()
        };
        [first[0].clone(), first[1].clone(), left, right].map(|samples| bytes(&samples))
    };
    assert_eq!(render(), render());
}

fn any_state() -> impl Strategy<Value = CompressorState> {
    (
        -60.0_f32..=0.0,
        1.0_f32..=100.0,
        0.1_f32..=300.0,
        1.0_f32..=3_000.0,
        0.0_f32..=18.0,
        0.0_f32..=24.0,
        0.0_f32..=1.0,
        0..3_usize,
    )
        .prop_map(
            |(threshold_db, ratio, attack_ms, release_ms, knee_db, makeup_db, mix, lookahead)| {
                CompressorState {
                    threshold_db,
                    ratio,
                    attack_ms,
                    release_ms,
                    knee_db,
                    makeup_db,
                    mix,
                    lookahead: Lookahead::ALL[lookahead],
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any valid records, one after the other every few milliseconds while full scale noise
    /// plays: the output is always a number and bounded.
    #[test]
    fn any_edits_while_it_plays_keep_the_output_bounded(
        states in proptest::collection::vec(any_state(), 2..12),
    ) {
        let mut rig = Rig::new(states[0], noise(1.0));
        for state in &states {
            rig.update(*state);
            let output = rig.render(480);
            for sample in output.iter().flatten() {
                prop_assert!(sample.is_finite());
                prop_assert!(sample.abs() < BOUND, "{sample} after {state:?}");
            }
        }
    }
}
