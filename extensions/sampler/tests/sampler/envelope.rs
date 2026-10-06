//! The envelope, velocity to volume, the gain and the part of the file that plays, each
//! measured on a sample of one steady level, where what comes out is the level times them.

use sampler::SamplerState;

use crate::support::{Harness, SAMPLE_RATE, decibels, note, playing};

/// The level of the steady sample.
const LEVEL: f32 = 0.5;

fn steady(seconds: f64) -> (&'static str, u32, Vec<f32>) {
    let frames = (seconds * f64::from(SAMPLE_RATE)) as usize;
    ("steady.wav", SAMPLE_RATE, vec![LEVEL; frames])
}

fn frames(seconds: f32) -> f64 {
    f64::from(seconds) * f64::from(SAMPLE_RATE)
}

/// Attack and release with a sustain of 1, where the release starts from full level.
#[test]
fn the_attack_and_the_release_take_their_times() {
    for (attack, release) in [(0.001, 0.001), (0.05, 0.3), (0.4, 1.5)] {
        let state = SamplerState {
            attack_seconds: attack,
            release_seconds: release,
            velocity_to_volume: 0.0,
            ..SamplerState::default()
        };
        // Held for one second.
        let notes = vec![note(0, 48_000, 60, 100)];
        let mut harness = Harness::playing(steady(4.0), notes, state);
        let played = harness.play(48_000 + 2 * 48_000);
        let level: Vec<f32> = played.iter().map(|sample| sample / LEVEL).collect();
        // Frames counted from the note on, the first one included.
        let full = level.iter().position(|level| *level >= 1.0).unwrap() + 1;
        // The last frame that sounds, after the note off at frame 48000.
        let silent = 48_000
            + level[48_000..]
                .iter()
                .position(|level| *level == 0.0)
                .unwrap();
        println!(
            "attack {attack} s: full level after {full} frames ({:.0} expected); release {release} s: silent after {} frames ({:.0} expected)",
            frames(attack),
            silent - 48_000,
            frames(release)
        );
        assert!((full as f64 - frames(attack)).abs() <= 1.0, "{full}");
        assert!(((silent - 48_000) as f64 - frames(release)).abs() <= 1.0);
        // Nothing sounds before the note or after its release.
        assert!(level[silent..].iter().all(|level| *level == 0.0));
    }
}

/// The decay falls to within 0.1 % of the way to the sustain level in its time, and the
/// sustain holds there.
#[test]
fn the_decay_reaches_the_sustain_level_in_its_time_and_stays() {
    for (decay, sustain) in [(0.05, 0.25), (0.4, 0.55), (2.0, 0.8)] {
        let state = SamplerState {
            attack_seconds: 0.001,
            decay_seconds: decay,
            sustain,
            velocity_to_volume: 0.0,
            ..SamplerState::default()
        };
        let notes = vec![note(0, 5 * 48_000, 60, 100)];
        let mut harness = Harness::playing(steady(6.0), notes, state);
        let played = harness.play(5 * 48_000);
        let level: Vec<f32> = played.iter().map(|sample| sample / LEVEL).collect();
        let peak = level.iter().position(|level| *level >= 1.0).unwrap();
        let near = 0.001 * (1.0 - sustain);
        let settled = level[peak..]
            .iter()
            .position(|level| level - sustain <= near)
            .unwrap();
        let held = level[5 * 48_000 - 100];
        // The decay aims 0.1 % of the way past the sustain level and gets there in its time,
        // so it is within 0.1 % a little sooner: after `ln 1000 / ln 1001` of it.
        let expected = frames(decay) * 1000_f64.ln() / 1001_f64.ln();
        println!(
            "decay {decay} s to {sustain}: within 0.1 % after {settled} frames ({expected:.1} expected), held at {held:.5}"
        );
        // Late in a long decay the level moves by less per frame than an `f32` sample tells
        // apart, so the frame it crosses is known to a few frames.
        let within = (frames(decay) * 5e-5).max(2.0);
        assert!((settled as f64 - expected).abs() <= within, "{settled}");
        assert!((held - sustain).abs() < 1e-4, "{held}");
    }
}

#[test]
fn velocity_changes_the_volume_as_much_as_it_is_set_to() {
    let velocities = [127, 100, 64, 32, 1];
    for amount in [0.0_f32, 0.5, 1.0] {
        let state = SamplerState {
            attack_seconds: 0.001,
            release_seconds: 0.001,
            velocity_to_volume: amount,
            ..SamplerState::default()
        };
        // Each held for 2000 frames, 1000 apart.
        let notes = velocities
            .iter()
            .enumerate()
            .map(|(index, velocity)| note(index as u64 * 3_000, 2_000, 60, *velocity))
            .collect();
        let mut harness = Harness::playing(steady(1.0), notes, state);
        let played = harness.play(velocities.len() * 3_000);
        for (index, velocity) in velocities.iter().enumerate() {
            let measured = played[index * 3_000 + 1_000] / LEVEL;
            let played = f32::from(*velocity) / 127.0;
            let expected = 1.0 - amount + amount * played * played;
            println!(
                "velocity_to_volume {amount}, velocity {velocity}: {:+.3} dB ({:+.3} dB expected)",
                decibels(f64::from(measured)),
                decibels(f64::from(expected))
            );
            assert!((measured - expected).abs() < 1e-6, "{measured} {expected}");
        }
    }
}

#[test]
fn the_gain_measures_as_set() {
    for gain_db in [-48.0_f32, -12.0, 0.0, 6.0, 24.0] {
        let state = SamplerState {
            velocity_to_volume: 0.0,
            gain_db,
            ..SamplerState::default()
        };
        let notes = vec![note(0, 10_000, 60, 127)];
        let mut harness = Harness::playing(steady(1.0), notes, state);
        let played = harness.play(10_000);
        let measured = decibels(f64::from(played[5_000] / LEVEL));
        println!("gain_db {gain_db}: {measured:+.4} dB");
        assert!((measured - f64::from(gain_db)).abs() < 1e-4, "{measured}");
    }
}

/// A ramp of distinct values, so where a note reads in the file shows in every frame. From a
/// file at the engine's rate at the root, the note plays the file itself; from one at another
/// rate, the part that plays lasts as long in seconds.
#[test]
fn a_note_plays_from_start_to_end_and_no_further() {
    let ramp: Vec<f32> = (0..48_000).map(|frame| frame as f32 / 48_000.0).collect();
    let state = playing(
        "ramp.wav",
        SamplerState {
            start_seconds: 0.25,
            end_seconds: Some(0.5),
            attack_seconds: 0.001,
            velocity_to_volume: 0.0,
            ..SamplerState::default()
        },
    );
    let mut harness = Harness::with_samples(&[("ramp.wav", SAMPLE_RATE, ramp.clone())]);
    harness.add_track(vec![note(0, 48_000, 60, 127)], state.clone());
    let played = harness.play(48_000);
    // From frame 12000 of the file, sample for sample after the attack, until the ramp of
    // 2 ms before the end.
    assert_eq!(played[100..12_000 - 96], ramp[12_100..24_000 - 96]);
    let last = played.iter().rposition(|sample| *sample != 0.0).unwrap();
    println!(
        "start 0.25 s, end 0.5 s of a 48 kHz file: first frame read {}, sound for {} frames (12000 expected)",
        12_100 - 100,
        last + 1
    );
    assert_eq!(last + 1, 12_000);
    // The edge before the end falls to silence in 96 frames, in a straight line.
    let edge: Vec<f32> = (0..96)
        .map(|frame| played[12_000 - 96 + frame] / ramp[24_000 - 96 + frame])
        .collect();
    assert!((edge[0] - 1.0).abs() < 0.02 && edge[95] < 0.02, "{edge:?}");

    // The same part of a file at 44.1 kHz is as long in seconds.
    let slow: Vec<f32> = (0..44_100).map(|_| 0.5).collect();
    let mut harness = Harness::with_samples(&[("ramp.wav", 44_100, slow)]);
    harness.add_track(vec![note(0, 48_000, 60, 127)], state);
    let played = harness.play(48_000);
    let last = played.iter().rposition(|sample| *sample != 0.0).unwrap();
    println!(
        "the same from a 44.1 kHz file: sound for {} frames",
        last + 1
    );
    assert!((last as i64 + 1 - 12_000).abs() <= 1, "{last}");
}

/// Reversed, the same part of the ramp plays from the end line back to the start line, sample
/// for sample after the attack, and stops there.
#[test]
fn a_reversed_note_plays_from_end_back_to_start() {
    let ramp: Vec<f32> = (0..48_000).map(|frame| frame as f32 / 48_000.0).collect();
    let state = playing(
        "ramp.wav",
        SamplerState {
            start_seconds: 0.25,
            end_seconds: Some(0.5),
            reverse: true,
            attack_seconds: 0.001,
            velocity_to_volume: 0.0,
            ..SamplerState::default()
        },
    );
    let mut harness = Harness::with_samples(&[("ramp.wav", SAMPLE_RATE, ramp.clone())]);
    harness.add_track(vec![note(0, 48_000, 60, 127)], state);
    let played = harness.play(48_000);
    // Frame 23999 of the file first, down to frame 12000 last.
    let backwards: Vec<f32> = ramp[12_000..24_000].iter().rev().copied().collect();
    assert_eq!(played[100..12_000 - 96], backwards[100..12_000 - 96]);
    let last = played.iter().rposition(|sample| *sample != 0.0).unwrap();
    assert_eq!(last + 1, 12_000);
}
