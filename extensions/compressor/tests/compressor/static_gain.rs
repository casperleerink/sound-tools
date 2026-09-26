//! The measured gain matches the threshold, the ratio and the knee: a steady sine at stated
//! peak levels through the compressor, once it has settled, against `static_gain_db` and
//! against numbers worked out by hand.
//!
//! Tolerance: 0.02 dB. At 1 kHz and 48 kHz a sine from phase 0 has a sample on its peak, so
//! the level the detector sees is the amplitude exactly; at the other frequencies the largest
//! sample of 10 ms is within 0.02 dB of it, which the reduction takes a part of.

use compressor::{CompressorState, Lookahead, static_gain_db};

use crate::support::measured_gain_db;

const TOLERANCE_DB: f64 = 0.02;

fn check(state: CompressorState, hz: f64, levels: impl IntoIterator<Item = f32>) -> f64 {
    let mut worst = 0.0_f64;
    for level in levels {
        let measured = measured_gain_db(state, hz, level);
        let exact = f64::from(static_gain_db(&state, level));
        worst = worst.max((measured - exact).abs());
        assert!(
            (measured - exact).abs() < TOLERANCE_DB,
            "{state:?} at {hz} Hz, {level} dB: measured {measured:.4} dB, exact {exact:.4} dB"
        );
    }
    worst
}

/// A hard knee at -20 dB and 4:1, by hand: nothing under the threshold, then three quarters of
/// every dB over it off.
#[test]
fn a_hard_knee_turns_down_by_the_ratio_above_the_threshold() {
    let state = CompressorState {
        threshold_db: -20.0,
        ratio: 4.0,
        knee_db: 0.0,
        ..CompressorState::default()
    };
    for (level, expected) in [
        (-40.0, 0.0),
        (-20.0, 0.0),
        (-16.0, -3.0),
        (-8.0, -9.0),
        (0.0, -15.0),
    ] {
        let measured = measured_gain_db(state, 1_000.0, level);
        println!("threshold -20, 4:1, hard knee, {level} dB in: {measured:.4} dB");
        assert!(
            (measured - expected).abs() < TOLERANCE_DB,
            "{level} dB: {measured:.4}, expected {expected}"
        );
    }
}

/// The soft knee bends in a quadratic from half its width under the threshold to half its
/// width over it: at the threshold it takes off an eighth of the width times the slope.
#[test]
fn the_knee_is_the_quadratic_bend_between_its_ends() {
    let state = CompressorState {
        threshold_db: -24.0,
        ratio: 3.0,
        knee_db: 12.0,
        ..CompressorState::default()
    };
    let slope = 1.0 - 1.0 / 3.0;
    for (level, expected) in [
        (-30.0, 0.0),
        (-27.0, -slope * 3.0 * 3.0 / 24.0),
        (-24.0, -slope * 12.0 / 8.0),
        (-18.0, -slope * 6.0),
        (-6.0, -slope * 18.0),
    ] {
        let measured = measured_gain_db(state, 1_000.0, level);
        println!(
            "threshold -24, 3:1, knee 12, {level} dB in: {measured:.4} dB, expected {expected:.4}"
        );
        assert!(
            (measured - expected).abs() < TOLERANCE_DB,
            "{level} dB: {measured:.4}, expected {expected}"
        );
    }
}

#[test]
fn every_setting_measures_as_its_static_gain_from_minus_sixty_to_plus_six_db() {
    let levels = (-20..=2).map(|step| step as f32 * 3.0);
    let mut worst = 0.0_f64;
    for threshold_db in [-60.0, -30.0, -10.0, 0.0] {
        for ratio in [1.0, 2.0, 8.0, 100.0] {
            for knee_db in [0.0, 6.0, 18.0] {
                let state = CompressorState {
                    threshold_db,
                    ratio,
                    knee_db,
                    ..CompressorState::default()
                };
                worst = worst.max(check(state, 1_000.0, levels.clone()));
            }
        }
    }
    println!("largest difference from the static gain: {worst:.4} dB");
}

/// Makeup adds its gain, the mix is a sum of the compressed and the dry sound, and the
/// lookahead changes nothing of a steady tone but its delay.
#[test]
fn makeup_mix_and_lookahead_measure_as_the_static_gain() {
    let base = CompressorState {
        threshold_db: -30.0,
        ratio: 6.0,
        ..CompressorState::default()
    };
    let states = [
        CompressorState {
            makeup_db: 12.0,
            ..base
        },
        CompressorState { mix: 0.4, ..base },
        CompressorState {
            mix: 0.0,
            makeup_db: 24.0,
            ..base
        },
        CompressorState {
            lookahead: Lookahead::One,
            ..base
        },
        CompressorState {
            lookahead: Lookahead::Ten,
            mix: 0.5,
            makeup_db: 6.0,
            ..base
        },
    ];
    for state in states {
        check(state, 1_000.0, [-50.0, -30.0, -12.0, 0.0]);
    }
}

/// From 50 Hz up the level of a tone is steady, so its gain is the static one too.
#[test]
fn other_frequencies_measure_as_the_static_gain() {
    let state = CompressorState {
        threshold_db: -24.0,
        ratio: 4.0,
        attack_ms: 1.0,
        release_ms: 50.0,
        ..CompressorState::default()
    };
    for hz in [50.0, 100.0, 440.0, 5_000.0, 15_000.0] {
        let worst = check(state, hz, [-40.0, -24.0, -12.0, -3.0]);
        println!("{hz} Hz: largest difference {worst:.4} dB");
    }
}
