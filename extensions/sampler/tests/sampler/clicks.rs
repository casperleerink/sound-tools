//! No clicks: a note that starts or ends on a loud part of its sample, an end of the part that
//! plays in the middle of the sound, a note that takes over a voice, and an edit while notes
//! sound. Measured as the largest step from one frame to the next, against the steps of the
//! sine itself.

use sampler::SamplerState;

use crate::support::{Harness, SAMPLE_RATE, largest_step, note, peak, sine};

/// A sine of 100 Hz steps by at most `2 pi 100 / 48000` of its amplitude per frame.
fn sine_step(amplitude: f32) -> f32 {
    std::f32::consts::TAU * 100.0 / SAMPLE_RATE as f32 * amplitude
}

/// The largest part of full level the envelope moves by in one frame: its first frame, where
/// its curves are steepest. The attack aims 30 % past full level, the release 0.1 % under
/// silence, and each takes its time in frames.
fn envelope_step(attack_seconds: f32, release_seconds: f32) -> f32 {
    let frames = |seconds: f32| (f64::from(seconds) * f64::from(SAMPLE_RATE)).max(1.0);
    let first = |seconds: f32, overshoot: f64| {
        let coefficient = (-((1.0 + overshoot) / overshoot).ln() / frames(seconds)).exp();
        (1.0 + overshoot) * (1.0 - coefficient)
    };
    first(attack_seconds, 0.3).max(first(release_seconds, 0.001)) as f32
}

/// The sine starts at its peak, a note starts there and ends while it is loud, and the part
/// that plays may end at a peak too. The largest step is the sine's own and what the envelope
/// adds by its shape, never the step of a hard edge. With the defaults, 2 ms and 300 ms, that
/// is a few hundredths; at the shortest times, 1 ms each, the first frame of the release takes
/// 13 % off, which is how short the composer asked it to be.
#[test]
fn a_note_that_starts_and_ends_at_the_loudest_place_of_its_sample_does_not_click() {
    let sample = sine(SAMPLE_RATE, 100.0, 0.9, 2.0, 0.25);
    let cases = [
        (0.002, 0.3, None),
        (0.002, 0.3, Some(0.5)),
        (0.01, 0.05, None),
        (0.001, 0.001, None),
    ];
    for (attack, release, end) in cases {
        let state = SamplerState {
            attack_seconds: attack,
            release_seconds: release,
            end_seconds: end,
            velocity_to_volume: 0.0,
            ..SamplerState::default()
        };
        let notes = vec![note(1_000, 30_000, 60, 127)];
        let mut harness = Harness::playing(("sine.wav", SAMPLE_RATE, sample.clone()), notes, state);
        let played = harness.play(60_000);
        let step = largest_step(&played);
        // The edge before the end of the part that plays is a straight line of 96 frames.
        let bound = sine_step(0.9) + 0.9 * envelope_step(attack, release).max(1.0 / 96.0);
        println!(
            "attack {attack} s, release {release} s, end {end:?}: largest step {step:.4}, bound {bound:.4}, the sine alone {:.4}, a hard edge 0.9",
            sine_step(0.9)
        );
        assert!(peak(&played) > 0.85);
        assert!(step <= bound, "{step} > {bound}");
    }
}

/// Twenty notes held at once: the 17th and later each take over a voice, which fades out over
/// 5 ms next to the new note instead of stopping.
#[test]
fn a_note_that_takes_over_a_voice_does_not_click() {
    let amplitude = 1.0 / 16.0;
    let sample = sine(SAMPLE_RATE, 100.0, amplitude, 3.0, 0.25);
    let state = SamplerState {
        attack_seconds: 0.001,
        velocity_to_volume: 0.0,
        ..SamplerState::default()
    };
    // 1000 frames apart, each held to the end, so the older ones are still at full level.
    let notes = (0..20)
        .map(|index| note(1_000 + index * 1_000, 100_000, 60, 127))
        .collect();
    let mut harness = Harness::playing(("sine.wav", SAMPLE_RATE, sample), notes, state);
    let played = harness.play(40_000);
    let step = largest_step(&played);
    // Sixteen sines at most, each 100 Hz, plus an attack of 1 ms and a fade of 5 ms.
    let bound = sine_step(1.0) + amplitude / 48.0 + amplitude / 240.0;
    println!(
        "20 notes on 16 voices: largest step {step:.5}, bound {bound:.5}, a hard takeover {amplitude:.4}"
    );
    assert!(step <= bound, "{step} > {bound}");
    // After the 16th note, sixteen voices sound: the level stays that of sixteen.
    let sixteen = peak(&played[17_000..17_900]);
    let twenty = peak(&played[21_000..21_900]);
    println!("peak with 16 voices {sixteen:.4}, after four more notes {twenty:.4}");
    assert!(twenty <= 1.0 + 1e-3, "{twenty}");
}

/// An agent writes a new gain while a note sounds: it glides there over 20 ms, in a straight
/// line of 960 frames, and the record is heard afterwards as if it had been there all along.
#[test]
fn a_gain_edit_while_a_note_sounds_glides_over_20_ms() {
    let sample = vec![0.5; 2 * SAMPLE_RATE as usize];
    let state = SamplerState {
        velocity_to_volume: 0.0,
        ..SamplerState::default()
    };
    let notes = vec![note(0, 60_000, 60, 127)];
    let mut harness = Harness::playing(("steady.wav", SAMPLE_RATE, sample), notes, state);
    let mut played = harness.play(9_600);
    let record = r#"{"tool": "sampler", "state": {"sample": "steady.wav", "velocity_to_volume": 0.0, "gain_db": -12.0}}"#;
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", record),
        1
    );
    assert_eq!(harness.project.problems(), []);
    let [left, _] = harness.render(9_600);
    played.extend(left);
    let quieter = 0.5 * 10_f32.powf(-12.0 / 20.0);
    // The edit arrives at the start of the next render, frame 9600.
    assert_eq!(played[9_599], 0.5);
    let glide = &played[9_600..9_600 + 960];
    let step = largest_step(glide);
    println!(
        "gain 0 dB to -12 dB while it sounds: from {} to {:.5} in 960 frames, largest step {step:.6} ({:.6} for a straight line)",
        played[9_599],
        played[9_600 + 960],
        (0.5 - quieter) / 960.0
    );
    assert!(glide.windows(2).all(|pair| pair[1] < pair[0]));
    assert!(step <= (0.5 - quieter) / 960.0 * 1.001, "{step}");
    assert!((played[9_600 + 960] - quieter).abs() < 1e-6);
    assert!((played[15_000] - quieter).abs() < 1e-6);
}
