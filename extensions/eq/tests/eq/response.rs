//! The measured curve matches the band settings: a quiet sine at stated frequencies through the
//! EQ, once it has settled, against the exact response of `eq::response`, against the analog
//! shapes, and for several bands at once against the bands measured one by one.
//!
//! Tolerances: measured against `response`, 0.02 dB wherever the gain is above -80 dB. Against
//! the analog shapes, which the EQ bends near the top of the scale: 0.02 dB for the gain of a
//! bell at its frequency and of a shelf at its frequency and far on its side; 0.2 dB for the
//! Butterworth cut from a quarter to twice its frequency at 1 kHz. Several bands against the
//! sum in dB of each band alone: 0.05 dB.

use eq::{Band, EqState, Shape, response};

use crate::support::{SAMPLE_RATE, band, measured_db, measured_db_at, with_bands};

const FREQUENCIES: [f64; 9] = [
    50.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

fn exact_db_at(state: &EqState, hz: f64, sample_rate: u32) -> f64 {
    20.0 * f64::from(response(state, hz as f32, sample_rate as f32)).log10()
}

fn exact_db(state: &EqState, hz: f64) -> f64 {
    exact_db_at(state, hz, SAMPLE_RATE)
}

/// Measures `state` at every frequency and holds it to its exact response. The largest
/// difference.
fn check_against_exact(label: &str, state: &EqState, frequencies: &[f64], sample_rate: u32) -> f64 {
    let mut worst = 0.0_f64;
    for hz in frequencies {
        let exact = exact_db_at(state, *hz, sample_rate);
        if exact < -80.0 {
            continue;
        }
        let measured = measured_db_at(*state, *hz, sample_rate);
        worst = worst.max((measured - exact).abs());
        assert!(
            (measured - exact).abs() < 0.02,
            "{label} at {hz} Hz, {sample_rate} Hz: measured {measured:.3} dB, exact {exact:.3} dB"
        );
    }
    worst
}

#[test]
fn every_shape_gain_and_q_measures_as_its_exact_response() {
    let mut worst = 0.0_f64;
    for shape in Shape::ALL {
        let gains: &[f32] = if shape.has_gain() {
            &[-12.0, 9.0]
        } else {
            &[0.0]
        };
        for gain_db in gains {
            for q in [0.3, 0.71, 4.0, 18.0] {
                let state = with_bands(&[band(shape, 1_000.0, *gain_db, q)]);
                let label = format!("{shape:?} {gain_db} dB Q {q}");
                worst = worst.max(check_against_exact(
                    &label,
                    &state,
                    &FREQUENCIES,
                    SAMPLE_RATE,
                ));
            }
        }
    }
    println!("largest difference from the exact response: {worst:.4} dB");
}

/// A band in each place of the list: band 4 is measured as band 1 is.
#[test]
fn every_band_of_the_list_is_heard() {
    for index in 0..eq::BANDS {
        let mut state = with_bands(&[]);
        state.bands[index] = band(Shape::Bell, 700.0, 6.0, 2.0);
        let measured = measured_db(state, 700.0);
        assert!(
            (measured - 6.0).abs() < 0.02,
            "band {}: {measured}",
            index + 1
        );
    }
}

/// The analog shapes, from their definitions: what a composer reads on the card is what the
/// EQ does.
#[test]
fn the_shapes_do_what_their_settings_say() {
    let near = |label: &str, measured: f64, expected: f64, tolerance: f64| {
        println!("{label}: {measured:.3} dB, expected {expected:.3} dB");
        assert!(
            (measured - expected).abs() < tolerance,
            "{label}: measured {measured:.3} dB, expected {expected:.3} dB"
        );
    };
    // A bell is its gain at its frequency, at every Q.
    for (gain_db, q) in [(9.0, 0.5), (-12.0, 2.0), (15.0, 18.0), (-15.0, 0.1)] {
        let state = with_bands(&[band(Shape::Bell, 1_000.0, gain_db, q)]);
        let label = format!("bell {gain_db} dB Q {q} at its frequency");
        near(
            &label,
            measured_db(state, 1_000.0),
            f64::from(gain_db),
            0.02,
        );
    }
    // A shelf is half its gain at its frequency and all of it far on its side, and nothing on
    // the other.
    for gain_db in [-10.0, 6.0] {
        let gain = f64::from(gain_db);
        let low = with_bands(&[band(Shape::LowShelf, 1_000.0, gain_db, 0.71)]);
        near(
            "low shelf at its frequency",
            measured_db(low, 1_000.0),
            gain / 2.0,
            0.02,
        );
        near("low shelf at 30 Hz", measured_db(low, 30.0), gain, 0.02);
        near("low shelf at 16 kHz", measured_db(low, 16_000.0), 0.0, 0.02);
        let high = with_bands(&[band(Shape::HighShelf, 1_000.0, gain_db, 0.71)]);
        near(
            "high shelf at its frequency",
            measured_db(high, 1_000.0),
            gain / 2.0,
            0.02,
        );
        near(
            "high shelf at 16 kHz",
            measured_db(high, 16_000.0),
            gain,
            0.02,
        );
        near("high shelf at 30 Hz", measured_db(high, 30.0), 0.0, 0.02);
    }
    // A cut at Q 0.71 is a second order Butterworth filter: 3 dB down at its frequency, 12 dB
    // per octave past it.
    let butterworth = |ratio: f64| -10.0 * (1.0 + ratio.powi(4)).log10();
    let q = std::f32::consts::FRAC_1_SQRT_2;
    for hz in [250.0, 500.0, 1_000.0, 2_000.0] {
        let low_cut = with_bands(&[band(Shape::LowCut, 1_000.0, 0.0, q)]);
        let label = format!("low cut at {hz} Hz");
        near(
            &label,
            measured_db(low_cut, hz),
            butterworth(1_000.0 / hz),
            0.2,
        );
        let high_cut = with_bands(&[band(Shape::HighCut, 1_000.0, 0.0, q)]);
        let label = format!("high cut at {hz} Hz");
        near(
            &label,
            measured_db(high_cut, hz),
            butterworth(hz / 1_000.0),
            0.2,
        );
    }
    // A notch takes its frequency out, and an octave away is where Q puts it.
    for q in [1.0, 8.0] {
        let notch = with_bands(&[band(Shape::Notch, 1_000.0, 0.0, q)]);
        let gap = measured_db(notch, 1_000.0);
        assert!(gap < -60.0, "notch Q {q}: {gap:.1} dB");
        // |1 - x²| / sqrt((1 - x²)² + (x / Q)²) at x = 2.
        let q = f64::from(q);
        let expected = 20.0 * (3.0 / (9.0 + 4.0 / (q * q)).sqrt()).log10();
        near(
            "notch an octave up",
            measured_db(notch, 2_000.0),
            expected,
            0.1,
        );
    }
}

/// Several bands at once are the product of the bands: their gains in dB add up. The example
/// of the agent doc, and four bands that overlap.
#[test]
fn several_bands_measure_as_the_sum_of_each_band_alone() {
    let vocal = [
        band(Shape::LowCut, 100.0, 0.0, 0.71),
        band(Shape::Bell, 400.0, -3.0, 1.5),
        band(Shape::Bell, 3_000.0, 2.0, 1.0),
        band(Shape::HighShelf, 10_000.0, 3.0, 0.71),
    ];
    let overlapping = [
        band(Shape::LowShelf, 200.0, 6.0, 1.2),
        band(Shape::Bell, 300.0, -9.0, 3.0),
        band(Shape::Notch, 1_000.0, 0.0, 6.0),
        band(Shape::HighCut, 5_000.0, 0.0, 2.0),
    ];
    for (name, bands) in [("vocal", vocal), ("overlapping", overlapping)] {
        let mut state = with_bands(&bands);
        state.output_gain_db = -2.5;
        let frequencies = [
            60.0, 150.0, 300.0, 700.0, 1_500.0, 3_000.0, 6_000.0, 12_000.0,
        ];
        let worst = check_against_exact(name, &state, &frequencies, SAMPLE_RATE);
        println!("{name}: largest difference from the exact response {worst:.4} dB");
        for hz in frequencies {
            let alone: f64 = bands
                .iter()
                .map(|one| measured_db(with_bands(&[*one]), hz))
                .sum::<f64>()
                - 2.5;
            let together = measured_db(state, hz);
            println!(
                "{name} at {hz} Hz: {together:.3} dB, the bands alone add up to {alone:.3} dB"
            );
            assert!(
                (together - alone).abs() < 0.05,
                "{name} at {hz} Hz: together {together:.3} dB, alone {alone:.3} dB"
            );
        }
    }
}

/// At 44.1 and 96 kHz too, and with a band at the top of the range at the most extreme Q and
/// gain: the measured sound is the exact response, which keeps the band under 45 % of the
/// sample rate.
#[test]
fn other_sample_rates_and_the_top_of_the_range_measure_as_the_exact_response() {
    let extremes: [Band; 4] = [
        band(Shape::Bell, 20_000.0, 15.0, 18.0),
        band(Shape::HighShelf, 20_000.0, -15.0, 18.0),
        band(Shape::HighCut, 20_000.0, 0.0, 18.0),
        band(Shape::LowCut, 20_000.0, 0.0, 0.1),
    ];
    for sample_rate in [44_100, 48_000, 96_000] {
        for extreme in extremes {
            let state = with_bands(&[extreme]);
            let frequencies = [1_000.0, 10_000.0, 16_000.0, 19_000.0, 20_000.0];
            let label = format!(
                "{:?} at {} Hz Q {}",
                extreme.shape, extreme.frequency_hz, extreme.q
            );
            check_against_exact(&label, &state, &frequencies, sample_rate);
        }
        let mid = with_bands(&[band(Shape::Bell, 1_000.0, -6.0, 1.0)]);
        check_against_exact(
            "bell at 1 kHz",
            &mid,
            &[500.0, 1_000.0, 2_000.0],
            sample_rate,
        );
    }
    // At 44.1 kHz the bell at 20 kHz is at 45 % of the sample rate, and there it has its gain.
    let top = with_bands(&[band(Shape::Bell, 20_000.0, 15.0, 18.0)]);
    let measured = measured_db_at(top, 0.45 * 44_100.0, 44_100);
    assert!((measured - 15.0).abs() < 0.02, "{measured}");
}

/// The default EQ changes no sample.
#[test]
fn the_default_eq_is_the_input_exactly() {
    let mut rig = crate::support::Rig::new(EqState::default(), crate::support::noise(0.8));
    let output = rig.render(SAMPLE_RATE as usize / 10);
    let mut source = crate::support::noise(0.8);
    for (frame, (left, right)) in output[0].iter().zip(&output[1]).enumerate() {
        assert_eq!([*left, *right], source(), "frame {frame}");
    }
    assert!(exact_db(&EqState::default(), 1_000.0).abs() < 1e-9);
}
