//! The measured response matches the record: a quiet sine at stated frequencies through the
//! filter, once it has settled, against the exact response of `filter::response` and against
//! the textbook Butterworth filter.
//!
//! Tolerances: measured against `response`, 0.02 dB wherever the gain is above -80 dB. Against
//! the analog Butterworth filter, which the filter bends near the top of the scale, 0.2 dB from
//! a quarter of the cutoff to twice the cutoff at a cutoff of 1 kHz.

use filter::{FilterState, FilterType, MAX_Q, Slope, response};

use crate::support::{SAMPLE_RATE, measured_db};

const OCTAVES: [f64; 7] = [125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0];

fn exact_db(state: &FilterState, hz: f64) -> f64 {
    20.0 * f64::from(response(state, hz as f32, SAMPLE_RATE as f32)).log10()
}

fn state(kind: FilterType, slope: Slope, resonance: f32, cutoff_hz: f32) -> FilterState {
    FilterState {
        kind,
        slope,
        resonance,
        cutoff_hz,
        ..FilterState::default()
    }
}

/// `1 / sqrt(1 + (f / cutoff)^(2 n))` for a low pass of order `n`, in dB; the high pass is the
/// same with the ratio turned round.
fn butterworth_db(kind: FilterType, slope: Slope, ratio: f64) -> f64 {
    let order = match slope {
        Slope::Twelve => 2,
        Slope::TwentyFour => 4,
    };
    let ratio = match kind {
        FilterType::HighPass => 1.0 / ratio,
        _ => ratio,
    };
    -10.0 * (1.0 + ratio.powi(2 * order)).log10()
}

#[test]
fn every_type_and_slope_measures_as_its_exact_response_at_every_octave() {
    let mut worst = 0.0_f64;
    for kind in FilterType::ALL {
        for slope in Slope::ALL {
            for resonance in [0.0, 0.5, 1.0] {
                let state = state(kind, slope, resonance, 1_000.0);
                for hz in OCTAVES {
                    let exact = exact_db(&state, hz);
                    if exact < -80.0 {
                        continue;
                    }
                    let measured = measured_db(state, hz);
                    worst = worst.max((measured - exact).abs());
                    assert!(
                        (measured - exact).abs() < 0.02,
                        "{kind:?} {slope:?} resonance {resonance} at {hz} Hz: measured {measured:.3} dB, exact {exact:.3} dB"
                    );
                }
            }
        }
    }
    println!("largest difference from the exact response: {worst:.4} dB");
}

#[test]
fn low_and_high_pass_at_resonance_zero_are_butterworth_filters() {
    for kind in [FilterType::LowPass, FilterType::HighPass] {
        for slope in Slope::ALL {
            let state = state(kind, slope, 0.0, 1_000.0);
            for hz in [250.0, 500.0, 1_000.0, 2_000.0] {
                let measured = measured_db(state, hz);
                let expected = butterworth_db(kind, slope, hz / 1_000.0);
                println!(
                    "{kind:?} {slope:?} at {hz} Hz: {measured:.2} dB, Butterworth {expected:.2} dB"
                );
                assert!(
                    (measured - expected).abs() < 0.2,
                    "{kind:?} {slope:?} at {hz} Hz: measured {measured:.3} dB, Butterworth {expected:.3} dB"
                );
            }
        }
    }
}

#[test]
fn the_cutoff_is_three_db_down_wherever_it_is_set() {
    for slope in Slope::ALL {
        for cutoff in [50.0, 200.0, 1_000.0, 5_000.0, 15_000.0] {
            for kind in [FilterType::LowPass, FilterType::HighPass] {
                let state = state(kind, slope, 0.0, cutoff);
                let measured = measured_db(state, f64::from(cutoff));
                assert!(
                    (measured + 3.0103).abs() < 0.02,
                    "{kind:?} {slope:?} at {cutoff} Hz: {measured:.3} dB"
                );
            }
        }
    }
}

/// Each step of resonance is the same step of the peak in dB, up to a Q of 20 on the last
/// section: +26 dB at 12 dB per octave, and 20 times the first section's Q at 24.
#[test]
fn resonance_raises_the_gain_at_the_cutoff_in_equal_steps_of_db() {
    let expected = |slope: Slope, resonance: f64| match slope {
        Slope::Twelve => {
            let base = std::f64::consts::FRAC_1_SQRT_2;
            20.0 * (base * (f64::from(MAX_Q) / base).powf(resonance)).log10()
        }
        Slope::TwentyFour => {
            let [first, second] = [0.541_196_1_f64, 1.306_563];
            20.0 * (first * second * (f64::from(MAX_Q) / second).powf(resonance)).log10()
        }
    };
    for slope in Slope::ALL {
        for resonance in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let state = state(FilterType::LowPass, slope, resonance as f32, 1_000.0);
            let measured = measured_db(state, 1_000.0);
            let expected = expected(slope, resonance);
            println!(
                "{slope:?} resonance {resonance}: {measured:.2} dB at the cutoff, expected {expected:.2} dB"
            );
            assert!(
                (measured - expected).abs() < 0.02,
                "{slope:?} resonance {resonance}: {measured:.3} dB, expected {expected:.3} dB"
            );
        }
    }
    let full = state(FilterType::LowPass, Slope::Twelve, 1.0, 1_000.0);
    assert!((measured_db(full, 1_000.0) - 26.02).abs() < 0.02);
}

#[test]
fn the_band_pass_peaks_at_zero_db_and_the_notch_takes_the_cutoff_out() {
    for slope in Slope::ALL {
        for resonance in [0.0, 0.5, 1.0] {
            let band = state(FilterType::BandPass, slope, resonance, 1_000.0);
            let peak = measured_db(band, 1_000.0);
            assert!(peak.abs() < 0.02, "{slope:?} {resonance}: {peak:.3} dB");
            let notch = state(FilterType::Notch, slope, resonance, 1_000.0);
            let gap = measured_db(notch, 1_000.0);
            assert!(gap < -60.0, "{slope:?} {resonance}: {gap:.1} dB");
        }
    }
}

/// Mix 0.5 is half the filtered sound and half the sound as it came in, as numbers with a
/// phase: the measured gain is the exact one.
#[test]
fn the_mix_blends_the_filtered_and_the_dry_sound() {
    for mix in [0.0, 0.25, 0.5, 1.0] {
        let state = FilterState {
            mix,
            resonance: 0.6,
            ..FilterState::default()
        };
        for hz in [500.0, 1_000.0, 4_000.0] {
            let (measured, exact) = (measured_db(state, hz), exact_db(&state, hz));
            assert!((measured - exact).abs() < 0.02, "mix {mix} at {hz} Hz");
        }
    }
}
