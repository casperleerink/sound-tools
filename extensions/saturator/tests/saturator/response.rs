//! The measured response matches the record: a quiet sine at stated frequencies through the
//! saturator, once it has settled, against the exact response of `saturator::response`. A
//! quiet sound goes through every curve on its straight part, so this holds the drive, the
//! automatic gain, the DC blocker, the tone, the output and the mix to their numbers, and the
//! dry sound to the saturated one in time: were they a frame apart, the mix would comb.
//!
//! Tolerance: 0.02 dB, from 20 Hz to 20 kHz, at 44.1, 48 and 96 kHz.

use saturator::{Curve, LATENCY, Saturator, SaturatorState, response};
use sound_core::Processor;

use crate::support::{Rig, SAMPLE_RATE, measured_db_at, sine};

const FREQUENCIES: [f64; 9] = [
    20.0, 50.0, 125.0, 500.0, 1_000.0, 4_000.0, 10_000.0, 16_000.0, 20_000.0,
];

fn exact_db(state: &SaturatorState, hz: f64, sample_rate: u32) -> f64 {
    20.0 * f64::from(response(state, hz as f32, sample_rate as f32)).log10()
}

#[test]
fn every_curve_tone_output_and_mix_measures_as_its_exact_response() {
    let states = [
        SaturatorState::default(),
        SaturatorState {
            curve: Curve::Clip,
            drive_db: 0.0,
            ..SaturatorState::default()
        },
        SaturatorState {
            curve: Curve::Tape,
            drive_db: 12.0,
            tone_db: 12.0,
            ..SaturatorState::default()
        },
        SaturatorState {
            curve: Curve::Tube,
            drive_db: 3.0,
            tone_db: -12.0,
            output_db: -6.0,
            ..SaturatorState::default()
        },
        // Half dry and half saturated, with the tone turning the phase of the saturated half.
        SaturatorState {
            curve: Curve::Soft,
            drive_db: 12.0,
            tone_db: 6.0,
            output_db: 12.0,
            mix: 0.5,
        },
    ];
    let mut worst = 0.0_f64;
    for sample_rate in [44_100, SAMPLE_RATE, 96_000] {
        for state in states {
            for hz in FREQUENCIES {
                let exact = exact_db(&state, hz, sample_rate);
                let measured = measured_db_at(state, hz, sample_rate);
                worst = worst.max((measured - exact).abs());
                assert!(
                    (measured - exact).abs() < 0.02,
                    "{state:?} at {hz} Hz and {sample_rate} Hz: measured {measured:.4} dB, exact {exact:.4} dB"
                );
            }
        }
    }
    println!("largest difference from the exact response: {worst:.4} dB");
}

/// With mix 0 the output is the input, the latency later, to the bit.
#[test]
fn mix_zero_is_the_sound_as_it_came_the_latency_later() {
    let state = SaturatorState {
        curve: Curve::Tube,
        drive_db: 30.0,
        mix: 0.0,
        ..SaturatorState::default()
    };
    let mut rig = Rig::new(state, sine(440.0, 0.8));
    let [output, _] = rig.render(4_800);
    let mut input = sine(440.0, 0.8);
    let input: Vec<f32> = (0..4_800).map(|_| input()[0]).collect();
    let latency = LATENCY as usize;
    assert_eq!(output[..latency], [0.0; LATENCY as usize]);
    assert_eq!(output[latency..], input[..4_800 - latency]);
}

#[test]
fn the_latency_is_reported_and_is_where_an_impulse_comes_out() {
    assert_eq!(Saturator::new(SaturatorState::default()).latency(), 64);
    let state = SaturatorState {
        curve: Curve::Clip,
        drive_db: 0.0,
        ..SaturatorState::default()
    };
    let mut once = true;
    let mut rig = Rig::new(
        state,
        Box::new(move || [if std::mem::take(&mut once) { 0.5 } else { 0.0 }; 2]),
    );
    let [output, _] = rig.render(480);
    let loudest = (0..output.len())
        .max_by(|a, b| output[*a].abs().total_cmp(&output[*b].abs()))
        .unwrap();
    assert_eq!(loudest, LATENCY as usize);
}
