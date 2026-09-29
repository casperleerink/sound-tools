//! Every setting is stable: nothing grows, nothing that is not a number, the wet sound keeps
//! about the level of what goes in, the effect comes to rest, and a render is the same every
//! time.

use modulation::{DEPTH, FEEDBACK, MIX, Mode, ModulationState, RATE, SPREAD};
use proptest::prelude::*;

use crate::support::{Rig, SECOND, Signal, db, for_frames, noise, peak, rms, sine};

/// The loudest full scale noise comes out at any setting, with room to spare. The peaks between
/// the notches at full feedback are 13 dB over the level of the noise.
const BOUND: f32 = 8.0;

fn assert_bounded(label: &str, [left, right]: &[Vec<f32>; 2]) {
    for sample in left.iter().chain(right) {
        assert!(sample.is_finite(), "{label}: {sample}");
    }
    let loudest = peak(left).max(peak(right));
    assert!(loudest < BOUND, "{label}: peak {loudest}");
}

/// Full scale noise at full feedback and depth, at the slowest and the fastest rate, in every
/// mode: bounded, and silent a while after the noise stops.
#[test]
fn full_feedback_at_every_end_stays_bounded_and_dies_away() {
    for mode in Mode::ALL {
        for rate_hz in [RATE.min, RATE.max] {
            let state = ModulationState {
                mode,
                rate_hz,
                depth: 1.0,
                feedback: 1.0,
                spread: 1.0,
                mix: 1.0,
            };
            let label = format!("{state:?}");
            let mut rig = Rig::new(state, for_frames(noise(1.0), 4 * SECOND));
            let output = rig.render(4 * SECOND);
            assert_bounded(&label, &output);
            // A feedback of 0.9 around 20 ms falls by 180 dB, to rest, in under 4 s.
            rig.render(4 * SECOND);
            let [left, right] = rig.render(SECOND);
            assert!(
                left.iter().chain(&right).all(|sample| *sample == 0.0),
                "{label}: {}",
                peak(&left).max(peak(&right))
            );
        }
    }
}

/// The wet sound of noise is about as loud as the noise, at every feedback: `√(1 - g²)` takes
/// back what the loop adds.
#[test]
fn the_wet_sound_of_noise_keeps_its_level_at_every_feedback() {
    let dry = {
        let state = ModulationState {
            mix: 0.0,
            ..ModulationState::default()
        };
        let [left, _] = Rig::new(state, noise(0.5)).render(SECOND);
        rms(&left)
    };
    for mode in Mode::ALL {
        for feedback in [0.0, 0.5, 1.0] {
            let state = ModulationState {
                mode,
                feedback,
                mix: 1.0,
                ..ModulationState::default()
            };
            let [left, right] = Rig::new(state, noise(0.5)).render(4 * SECOND);
            let level =
                rms(&left[SECOND..]).hypot(rms(&right[SECOND..])) / std::f64::consts::SQRT_2;
            let change = db(level / dry);
            println!("{mode:?} at feedback {feedback}: {change:+.2} dB");
            assert!(change.abs() < 3.0, "{mode:?}, {feedback}: {change}");
        }
    }
}

/// A sample that is not a number, or is infinite, is silence or the limit to the effect. Only
/// the dry sound of those frames carries them; the loop comes back to the sound it had.
#[test]
fn input_that_is_not_a_number_or_infinite_does_not_break_the_loop() {
    const BROKEN: [usize; 3] = [1_000, 2_000, 3_000];
    let with_broken_frames = || -> Signal {
        let mut frame = 0_usize;
        let mut clean = sine(440.0, 0.5);
        Box::new(move || {
            frame += 1;
            let sound = clean();
            match frame {
                1_000 => [f32::NAN, f32::INFINITY],
                2_000 => [f32::NEG_INFINITY, f32::NAN],
                3_000 => [f32::MAX, -f32::MAX],
                _ => sound,
            }
        })
    };
    for mode in Mode::ALL {
        let state = ModulationState {
            mode,
            feedback: 1.0,
            ..ModulationState::default()
        };
        let [left, right] = Rig::new(state, with_broken_frames()).render(3 * SECOND);
        for (index, (left, right)) in left.iter().zip(&right).enumerate() {
            if !BROKEN.contains(&(index + 1)) {
                assert!(left.is_finite() && right.is_finite(), "{mode:?} at {index}");
            }
        }
        let [clean, _] = Rig::new(state, sine(440.0, 0.5)).render(3 * SECOND);
        let late = 2 * SECOND..3 * SECOND;
        let apart = left[late.clone()]
            .iter()
            .zip(&clean[late])
            .fold(0.0_f32, |apart, (heard, clean)| {
                apart.max((heard - clean).abs())
            });
        assert!(apart < 1e-3, "{mode:?}: {apart}");
    }
}

#[test]
fn a_render_is_the_same_every_time() {
    let state = ModulationState {
        mode: Mode::Flanger,
        feedback: 0.8,
        rate_hz: 2.0,
        ..ModulationState::default()
    };
    let first = Rig::new(state, noise(0.5)).render(SECOND);
    let second = Rig::new(state, noise(0.5)).render(SECOND);
    assert_eq!(first, second);
}

fn any_state() -> impl Strategy<Value = ModulationState> {
    let range = |parameter: &modulation::Parameter| parameter.min..=parameter.max;
    (
        prop_oneof![Just(Mode::Chorus), Just(Mode::Flanger), Just(Mode::Phaser)],
        range(&RATE),
        range(&DEPTH),
        range(&FEEDBACK),
        range(&SPREAD),
        range(&MIX),
    )
        .prop_map(
            |(mode, rate_hz, depth, feedback, spread, mix)| ModulationState {
                mode,
                rate_hz,
                depth,
                feedback,
                spread,
                mix,
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
            let output = rig.render(2_400);
            for sample in output.iter().flatten() {
                prop_assert!(sample.is_finite());
                prop_assert!(sample.abs() < BOUND, "{sample} after {state:?}");
            }
        }
    }
}
