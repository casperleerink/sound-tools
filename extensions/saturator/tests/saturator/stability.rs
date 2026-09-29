//! Every setting is stable: nothing that is not a number, a bounded output at the loudest
//! settings, the saturator comes to rest, and a render is the same every time.

use proptest::prelude::*;
use saturator::{Curve, DRIVE, OUTPUT, SaturatorState, TONE};

use crate::support::{Rig, SAMPLE_RATE, Signal, noise, peak, sine};

/// The loudest the saturator makes a full scale input. The curves hold what comes out of them
/// under 1.5 after the automatic gain, which is largest at drive 0; the tone and the output
/// can add up to 18 dB; the edges of a square ring the DC blocker and the tone a little over.
const BOUND: f32 = 16.0;

fn assert_bounded(label: &str, [left, right]: &[Vec<f32>; 2]) {
    for sample in left.iter().chain(right) {
        assert!(sample.is_finite(), "{label}: {sample}");
    }
    let loudest = peak(left).max(peak(right));
    println!("{label}: peak {loudest:.3}");
    assert!(loudest < BOUND, "{label}: peak {loudest}");
}

/// A square at full scale, whose every edge rings the filters.
fn square(hz: f64) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(SAMPLE_RATE);
    Box::new(move || {
        phase = (phase + step).fract();
        [if phase < 0.5 { 1.0 } else { -1.0 }; 2]
    })
}

#[test]
fn the_loudest_settings_of_every_curve_stay_bounded() {
    for curve in Curve::ALL {
        for drive_db in [DRIVE.min, DRIVE.max] {
            for tone_db in [TONE.min, TONE.max] {
                let state = SaturatorState {
                    curve,
                    drive_db,
                    tone_db,
                    output_db: OUTPUT.max,
                    mix: 1.0,
                };
                let label = format!("{curve:?} drive {drive_db} tone {tone_db}");
                let mut rig = Rig::new(state, square(50.0));
                assert_bounded(&label, &rig.render(SAMPLE_RATE as usize / 2));
                let mut rig = Rig::new(state, noise(1.0));
                assert_bounded(&label, &rig.render(SAMPLE_RATE as usize / 2));
            }
        }
    }
}

/// A sample that is not a number, or is infinite, is silence or the limit to the saturator. It
/// goes on as if nothing had happened.
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
            _ => clean(),
        }
    });
    let state = SaturatorState {
        mix: 0.5,
        ..SaturatorState::default()
    };
    let output = Rig::new(state, broken).render(SAMPLE_RATE as usize);
    for sample in output.iter().flatten() {
        assert!(sample.is_finite());
    }
    let [left, _] = &output;
    assert!(peak(&left[SAMPLE_RATE as usize / 2..]) < 0.5);
}

/// After a sound ends, what is left in the delays and the filters dies away, and the output is
/// exactly silent: the saturator is at rest and does no work.
#[test]
fn it_comes_to_rest_after_a_sound() {
    let mut frame = 0_usize;
    let mut sound = noise(0.8);
    let burst: Signal = Box::new(move || {
        frame += 1;
        if frame < 4_800 { sound() } else { [0.0; 2] }
    });
    let state = SaturatorState {
        curve: Curve::Tube,
        drive_db: 24.0,
        tone_db: -12.0,
        ..SaturatorState::default()
    };
    let mut rig = Rig::new(state, burst);
    let [sounding, _] = rig.render(SAMPLE_RATE as usize / 5);
    assert!(peak(&sounding[4_800..4_900]) > 0.0);
    rig.render(2 * SAMPLE_RATE as usize);
    let [rest, other] = rig.render(SAMPLE_RATE as usize);
    assert!(rest.iter().chain(&other).all(|sample| *sample == 0.0));
}

#[test]
fn renders_are_the_same_every_time_to_the_byte() {
    let state = SaturatorState {
        curve: Curve::Tube,
        drive_db: 18.0,
        tone_db: 4.0,
        output_db: -2.0,
        mix: 0.8,
    };
    let render = || {
        let mut rig = Rig::new(state, noise(0.8));
        let first = rig.render(SAMPLE_RATE as usize);
        rig.update(SaturatorState {
            curve: Curve::Tape,
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

fn any_state() -> impl Strategy<Value = SaturatorState> {
    (
        0..4_usize,
        0.0_f32..=36.0,
        -12.0_f32..=12.0,
        -12.0_f32..=12.0,
        0.0_f32..=1.0,
    )
        .prop_map(
            |(curve, drive_db, tone_db, output_db, mix)| SaturatorState {
                curve: Curve::ALL[curve],
                drive_db,
                tone_db,
                output_db,
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
            let output = rig.render(480);
            for sample in output.iter().flatten() {
                prop_assert!(sample.is_finite());
                prop_assert!(sample.abs() < BOUND, "{sample} after {state:?}");
            }
        }
    }
}
