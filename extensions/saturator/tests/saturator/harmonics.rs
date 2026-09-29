//! What the curves add to a tone: odd harmonics from the symmetric curves, a second one from
//! the tube, no DC, nothing from the clip curve at drive 0, and next to nothing out of tune.
//!
//! A tone is at a whole number of cycles in a window of 0.1 s, a prime number of them, so its
//! harmonics fall on their own frequencies of the window and what folded back from above the
//! Nyquist frequency falls between them. The same curve with no oversampling, the record's
//! `transfer` sample by sample, shows what the oversampling takes away.

use saturator::{Curve, SaturatorState, transfer};

use crate::support::{Rig, SAMPLE_RATE, Spectrum, sine};

const WINDOW: usize = SAMPLE_RATE as usize / 10;
/// The frequencies of the window are 10 Hz apart.
const BIN_HZ: f64 = 10.0;

/// A settled tone at `bin` times 10 Hz and -12 dBFS through the saturator, and through its
/// curve at the sample rate.
fn spectra(state: SaturatorState, bin: usize, amplitude: f32) -> (Spectrum, Spectrum) {
    let hz = bin as f64 * BIN_HZ;
    let mut rig = Rig::new(state, sine(hz, amplitude));
    let settle = SAMPLE_RATE as usize;
    rig.render(settle);
    let [left, right] = rig.render(WINDOW);
    assert_eq!(left, right);
    let ours = Spectrum::of(&left, bin, SAMPLE_RATE);
    let mut plain = sine(hz, amplitude);
    let plain: Vec<f32> = (0..settle + WINDOW)
        .map(|_| transfer(&state, plain()[0]))
        .skip(settle)
        .collect();
    (ours, Spectrum::of(&plain, bin, SAMPLE_RATE))
}

fn at(curve: Curve, drive_db: f32) -> SaturatorState {
    SaturatorState {
        curve,
        drive_db,
        ..SaturatorState::default()
    }
}

#[test]
fn the_symmetric_curves_add_odd_harmonics_only_and_the_tube_a_second_one() {
    for curve in Curve::ALL {
        // The clip curve is clean under its ceiling, so it gets past it.
        let drive_db = if curve == Curve::Clip { 24.0 } else { 12.0 };
        let (spectrum, _) = spectra(at(curve, drive_db), 101, 0.25);
        let [second, third, fourth] = [2, 3, 4].map(|number| spectrum.harmonic_db(number));
        println!("{curve:?}: second {second:.1} dB, third {third:.1} dB, fourth {fourth:.1} dB");
        assert!(third > -40.0, "{curve:?}: {third}");
        match curve {
            Curve::Tube => assert!((-30.0..-15.0).contains(&second), "{second}"),
            _ => assert!(
                second < -120.0 && fourth < -120.0,
                "{curve:?}: {second} {fourth}"
            ),
        }
    }
}

/// A 5 kHz tone driven hard, whose harmonics are nearly all above the Nyquist frequency: at the
/// sample rate most of them would fold back out of tune.
#[test]
fn nearly_nothing_folds_back() {
    for curve in Curve::ALL {
        for drive_db in [12.0, 24.0, 36.0] {
            let (ours, plain) = spectra(at(curve, drive_db), 499, 0.25);
            let (ours, plain) = (ours.out_of_tune_db, plain.out_of_tune_db);
            println!(
                "{curve:?} at {drive_db} dB: out of tune {ours:.1} dB, {plain:.1} dB without oversampling"
            );
            // Under its ceiling the clip curve makes nothing to fold back, with or without.
            if plain < -120.0 {
                assert!(ours < -120.0, "{curve:?} {drive_db}");
                continue;
            }
            match drive_db < 30.0 {
                true => assert!(ours < -65.0 && ours < plain - 35.0, "{curve:?} {drive_db}"),
                false => assert!(ours < -35.0 && ours < plain - 20.0, "{curve:?} {drive_db}"),
            }
        }
        // A 1 kHz tone at the most drive.
        let (ours, _) = spectra(at(curve, 36.0), 101, 0.25);
        println!(
            "{curve:?} at 1 kHz: out of tune {:.1} dB",
            ours.out_of_tune_db
        );
        assert!(ours.out_of_tune_db < -75.0, "{curve:?}");
    }
}

/// The tube leans, so what it makes of a tone has a mean that is not 0. The DC blocker takes it
/// away.
#[test]
fn the_tube_adds_no_dc() {
    let state = at(Curve::Tube, 24.0);
    let mut rig = Rig::new(state, sine(100.0, 0.25));
    rig.render(SAMPLE_RATE as usize);
    let [left, _] = rig.render(WINDOW);
    let mean = |samples: &[f32]| {
        samples.iter().map(|sample| f64::from(*sample)).sum::<f64>() / samples.len() as f64
    };
    let mut plain = sine(100.0, 0.25);
    let plain: Vec<f32> = (0..WINDOW).map(|_| transfer(&state, plain()[0])).collect();
    println!(
        "mean {:.2e}, without the blocker {:.2e}",
        mean(&left),
        mean(&plain)
    );
    assert!(mean(&plain).abs() > 0.01);
    assert!(mean(&left).abs() < 1e-6);
}

/// The clip curve at drive 0 is a straight line up to full scale: a sine just under it comes
/// out with nothing added.
#[test]
fn the_clip_curve_at_drive_zero_adds_nothing() {
    let (spectrum, _) = spectra(at(Curve::Clip, 0.0), 101, 0.89);
    let fundamental = 20.0 * (spectrum.harmonics[0] / 0.89).log10();
    assert!(fundamental.abs() < 0.001, "{fundamental}");
    for number in 2..=spectrum.harmonics.len() {
        assert!(spectrum.harmonic_db(number) < -120.0, "{number}");
    }
    assert!(spectrum.out_of_tune_db < -120.0);
}
