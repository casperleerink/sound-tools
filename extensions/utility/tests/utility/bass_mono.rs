//! Bass mono takes the side away below its frequency, as the high pass of a Linkwitz-Riley
//! crossover of the fourth order, and a mono sound comes out at its own level at every
//! frequency: the bands add up flat.

use std::f64::consts::PI;

use utility::UtilityState;

use crate::support::{measured, measured_at};

fn bass_mono(hz: f32) -> UtilityState {
    UtilityState {
        bass_mono: true,
        bass_mono_hz: hz,
        ..UtilityState::default()
    }
}

fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// The gain of the high pass of the crossover at `hz`: two Butterworth sections, each
/// `x² / (1 + j √2 x - x²)`, so `x⁴ / (1 + x⁴)` in all. The trapezoidal form is the analog filter
/// with its frequencies bent by `tan(π f / sample rate)`, so `x` is the ratio of the bent
/// frequencies.
fn high_pass(hz: f64, crossover_hz: f64, sample_rate: u32) -> f64 {
    let bend = |hz: f64| (PI * hz / f64::from(sample_rate)).tan();
    let x4 = (bend(hz) / bend(crossover_hz)).powi(4);
    x4 / (1.0 + x4)
}

#[test]
fn a_mono_sound_comes_out_at_its_own_level_at_every_frequency() {
    for hz in [30.0, 60.0, 120.0, 250.0, 1_000.0, 8_000.0] {
        let [left, right] = measured(bass_mono(120.0), hz, [0.5, 0.5]);
        for heard in [left, right] {
            let off = db(heard / 0.5);
            assert!(off.abs() < 0.01, "{hz} Hz: {off} dB");
        }
    }
}

/// At the sample rates of a device, the side is where the formula says.
#[test]
fn the_side_follows_the_high_pass_of_the_crossover() {
    for sample_rate in [44_100, 48_000, 96_000] {
        for crossover_hz in [50.0, 120.0, 500.0] {
            for hz in [25.0, 60.0, 120.0, 250.0, 500.0, 2_000.0] {
                let state = bass_mono(crossover_hz as f32);
                let [left, right] = measured_at(state, hz, [0.5, -0.5], sample_rate);
                let expected = 0.5 * high_pass(hz, crossover_hz, sample_rate);
                for heard in [left, right] {
                    let off = (heard - expected).abs();
                    assert!(
                        off < 1e-4 || db(heard / expected).abs() < 0.05,
                        "{sample_rate}: {crossover_hz} Hz crossover at {hz} Hz: {heard} where {expected}"
                    );
                }
            }
        }
    }
    // At the crossover the side is half as loud, 6 dB down, and three octaves under it 72 dB.
    assert!((db(high_pass(120.0, 120.0, 48_000)) + 6.02).abs() < 0.01);
    assert!(db(high_pass(15.0, 120.0, 48_000)) < -70.0);
}

/// A sound only on the left: the bass comes from both sides, the highs only from the left.
#[test]
fn a_sound_on_one_side_keeps_its_highs_there_and_its_bass_goes_to_the_middle() {
    let [left, right] = measured(bass_mono(120.0), 30.0, [0.5, 0.0]);
    assert!(db(left / right).abs() < 0.1, "{left} {right}");
    let [left, right] = measured(bass_mono(120.0), 4_000.0, [0.5, 0.0]);
    assert!(db(left / 0.5).abs() < 0.01, "{left}");
    assert!(db(right / 0.5) < -60.0, "{right}");
}
