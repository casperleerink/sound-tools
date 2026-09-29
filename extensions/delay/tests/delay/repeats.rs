//! How loud each repeat is: one pass through the cuts for the first, and the feedback on top
//! for each after it, as `delay::response` says.

use delay::{DelayState, FEEDBACK, response};

use crate::support::{Rig, SAMPLE_RATE, SECOND, db, impulse, peak, rms, sine_for, wet};

/// A tone of 50 ms and its first four repeats, 100 ms apart, each measured in its middle 30 ms,
/// where no edge of the tone is.
#[test]
fn each_repeat_is_the_response_times_the_feedback() {
    let time = SECOND / 10;
    let mut worst = 0.0_f64;
    for (low_cut_hz, high_cut_hz) in [(20.0, 20_000.0), (100.0, 8_000.0), (400.0, 2_000.0)] {
        for hz in [150.0, 1_000.0, 5_000.0] {
            for feedback in [0.3, 0.7, FEEDBACK.max] {
                let state = DelayState {
                    sync: false,
                    time_ms: 100.0,
                    feedback,
                    low_cut_hz,
                    high_cut_hz,
                    ..wet()
                };
                let tone = sine_for(hz, 0.5, SECOND / 20, SAMPLE_RATE);
                let [left, _] = Rig::new(state, tone).render(5 * time);
                let middle = |start: usize| rms(&left[start + SECOND / 100..][..SECOND * 3 / 100]);
                let tone_level = 0.5 / 2.0_f64.sqrt();
                let pass = f64::from(response(&state, hz as f32, SAMPLE_RATE as f32));
                for number in 1..=4 {
                    let expected =
                        tone_level * pass.powi(number) * f64::from(feedback).powi(number - 1);
                    let measured = middle(number as usize * time);
                    let error = db(measured) - db(expected);
                    worst = worst.max(error.abs());
                    assert!(
                        error.abs() < 0.1,
                        "{hz} Hz through {low_cut_hz} to {high_cut_hz} Hz, feedback {feedback}, \
                         repeat {number}: {error:+.3} dB"
                    );
                }
            }
        }
    }
    println!("worst: {worst:.3} dB");
}

/// Feedback 0 is one repeat, and nothing where a second would be.
#[test]
fn feedback_zero_is_one_repeat() {
    let state = DelayState {
        sync: false,
        time_ms: 50.0,
        feedback: 0.0,
        ..wet()
    };
    let time = SECOND / 20;
    let [left, _] = Rig::new(state, impulse()).render(5 * time);
    let around = |number: usize| peak(&left[number * time - 10..number * time + 10]);
    assert!(around(1) > 0.5);
    for number in 2..=4 {
        assert!(around(number) < 1e-4 * around(1), "{number}");
    }
}

/// The sound as it came in and the repeats, by the mix: at 0.3, 70 % of a tone until its
/// repeat comes, and then 30 % of the repeat on top.
#[test]
fn the_mix_is_the_dry_sound_and_the_repeats_in_their_parts() {
    let state = DelayState {
        sync: false,
        time_ms: 100.0,
        feedback: 0.0,
        mix: 0.3,
        ..wet()
    };
    let tone = || sine_for(1_000.0, 0.5, SECOND / 20, SAMPLE_RATE);
    let [left, _] = Rig::new(state, tone()).render(SECOND / 5);
    let mut dry = tone();
    for sample in &left[..SECOND / 20] {
        let [expected, _] = dry();
        assert!((sample - 0.7 * expected).abs() < 1e-6);
    }
    let time = SECOND / 10;
    let repeat = rms(&left[time + SECOND / 100..][..SECOND * 3 / 100]);
    let pass = f64::from(response(&state, 1_000.0, SAMPLE_RATE as f32));
    let expected = 0.3 * pass * 0.5 / 2.0_f64.sqrt();
    assert!((db(repeat) - db(expected)).abs() < 0.1);
}
