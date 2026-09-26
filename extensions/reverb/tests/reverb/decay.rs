//! The tail falls by 60 dB in the decay time, and the highs in their part of it.

use reverb::{DAMPED_HZ, ReverbState, high_decay_seconds};

use crate::support::{Rig, SAMPLE_RATE, impulse, plain, slope};

/// The time to fall by 60 dB of an impulse response, by Schroeder's backward integration: the
/// energy still to come at each frame, in dB, with a straight line fitted from -5 to -35 dB and
/// carried on to -60 (T30 of ISO 3382). Both channels together.
fn decay_time([left, right]: &[Vec<f32>; 2], sample_rate: u32) -> f64 {
    let energy: Vec<f64> = left
        .iter()
        .zip(right)
        .map(|(left, right)| f64::from(*left).powi(2) + f64::from(*right).powi(2))
        .collect();
    let mut still_to_come = vec![0.0; energy.len()];
    let mut sum = 0.0;
    for (index, energy) in energy.iter().enumerate().rev() {
        sum += energy;
        still_to_come[index] = sum;
    }
    let total = still_to_come[0];
    let points: Vec<(f64, f64)> = still_to_come
        .iter()
        .enumerate()
        .map(|(frame, energy)| (frame as f64 / f64::from(sample_rate), 10.0 * (energy / total).log10()))
        .filter(|(_, level)| (-35.0..=-5.0).contains(level))
        .collect();
    -60.0 / slope(&points)
}

/// The impulse response of a plain reverb, long enough for its tail to fall past -80 dB, so
/// the end of the render does not bend the curve where it is measured.
fn impulse_response(state: ReverbState, sample_rate: u32) -> [Vec<f32>; 2] {
    let seconds = 1.4 * state.decay_seconds + 0.4;
    let mut rig = Rig::at_rate(state, impulse(), sample_rate);
    rig.render((seconds * sample_rate as f32) as usize)
}

/// Measured on an impulse for decays across the range, at three sizes, and the error printed.
#[test]
fn an_impulse_falls_by_sixty_db_in_the_decay_time() {
    let mut worst = 0.0_f64;
    for size in [0.0, 0.5, 1.0] {
        for decay in [0.3, 0.5, 1.0, 2.0, 5.0, 10.0] {
            let state = ReverbState {
                size,
                ..plain(decay)
            };
            let measured = decay_time(&impulse_response(state, SAMPLE_RATE), SAMPLE_RATE);
            let error = measured / f64::from(decay) - 1.0;
            println!("size {size}, decay {decay} s: measured {measured:.3} s, {:+.1} %", error * 100.0);
            worst = worst.max(error.abs());
        }
    }
    println!("worst: {:.1} %", worst * 100.0);
    assert!(worst < 0.05, "{worst}");
}

/// The decay does not depend on the sample rate.
#[test]
fn the_decay_is_the_same_at_other_sample_rates() {
    for sample_rate in [44_100, 96_000] {
        let measured = decay_time(&impulse_response(plain(2.0), sample_rate), sample_rate);
        println!("{sample_rate} Hz: measured {measured:.3} s for 2 s");
        assert!((measured / 2.0 - 1.0).abs() < 0.05, "{sample_rate}: {measured}");
    }
}

/// A band of a third of an octave around `hz`: two band pass biquads (Bristow-Johnson's
/// cookbook) one after the other, so the band edges are steep enough that the late tail of
/// its neighbours does not lengthen what is measured.
fn band(samples: &[f32], hz: f64, sample_rate: u32) -> Vec<f32> {
    let angle = std::f64::consts::TAU * hz / f64::from(sample_rate);
    let alpha = angle.sin() / (2.0 * 4.32);
    let a0 = 1.0 + alpha;
    let (b0, b2) = (alpha / a0, -alpha / a0);
    let (a1, a2) = (-2.0 * angle.cos() / a0, (1.0 - alpha) / a0);
    let mut output: Vec<f64> = samples.iter().map(|sample| f64::from(*sample)).collect();
    for _ in 0..2 {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for sample in &mut output {
            let x = *sample;
            let y = b0 * x + b2 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, x, y1, y);
            *sample = y;
        }
    }
    output.into_iter().map(|sample| sample as f32).collect()
}

/// The decay time of the band around `hz` of an impulse response.
fn band_decay(response: &[Vec<f32>; 2], hz: f64) -> f64 {
    let [left, right] = response;
    let filtered = [band(left, hz, SAMPLE_RATE), band(right, hz, SAMPLE_RATE)];
    decay_time(&filtered, SAMPLE_RATE)
}

/// With damping, the lows keep the decay time and the highs at 5 kHz fall in their part of it.
#[test]
fn damping_makes_the_highs_fall_sooner_by_the_part_it_says() {
    let (mut worst_lows, mut worst_highs) = (0.0_f64, 0.0_f64);
    for damping in [0.0, 0.5, 1.0] {
        for decay in [1.0, 3.0] {
            let state = ReverbState {
                damping,
                ..plain(decay)
            };
            let response = impulse_response(state, SAMPLE_RATE);
            let lows = band_decay(&response, 200.0);
            let highs = band_decay(&response, f64::from(DAMPED_HZ));
            let expected = f64::from(high_decay_seconds(&state));
            println!(
                "damping {damping}, decay {decay} s: 200 Hz {lows:.3} s, 5 kHz {highs:.3} s for {expected:.3} s"
            );
            worst_lows = worst_lows.max((lows / f64::from(decay) - 1.0).abs());
            worst_highs = worst_highs.max((highs / expected - 1.0).abs());
        }
    }
    println!(
        "worst: {:.1} % at 200 Hz, {:.1} % at 5 kHz",
        worst_lows * 100.0,
        worst_highs * 100.0
    );
    // A third of an octave is still wide where the loss rises fast with frequency: its lower
    // edge dies slower than 5 kHz, and the fit sees some of that.
    assert!(worst_lows < 0.05, "{worst_lows}");
    assert!(worst_highs < 0.15, "{worst_highs}");
}
