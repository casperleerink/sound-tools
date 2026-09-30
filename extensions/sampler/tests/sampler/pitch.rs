//! Each key plays the sample at its pitch: a sine sample, measured per key over eight octaves,
//! from files at the engine's rate and at others.

use sampler::SamplerState;
use sound_notes::Pitch;

use crate::support::{
    Harness, INSTRUMENT, SAMPLE_RATE, Track, frequency, id, note, peak, playing, sine,
};

/// The sine of the sample: A4, so the root is 69.
const HZ: f64 = 440.0;

/// Keys from four octaves under the root to four over, and some between.
const KEYS: [u8; 13] = [21, 33, 45, 50, 57, 64, 68, 69, 70, 81, 88, 105, 117];

#[test]
fn each_key_plays_the_sample_at_its_pitch_also_from_a_file_at_another_rate() {
    for file_rate in [48_000, 44_100, 96_000, 22_050] {
        // Long enough for a quarter second at 16 times the speed.
        let sample = sine(file_rate, HZ, 0.5, 5.0, 0.0);
        let state = SamplerState {
            root: Pitch::new(69).unwrap(),
            // Each note is over before the next one starts.
            release_seconds: 0.001,
            ..SamplerState::default()
        };
        // One key at a time, each held for a quarter second, then 1000 frames of rest.
        let notes = KEYS
            .iter()
            .enumerate()
            .map(|(index, key)| note(index as u64 * 13_000, 12_000, *key, 127))
            .collect();
        let mut harness = Harness::playing(("a4.wav", file_rate, sample), notes, state);
        let played = harness.play(KEYS.len() * 13_000);
        for (index, key) in KEYS.iter().enumerate() {
            // Past the attack, and before the release.
            let from = index * 13_000 + 500;
            let held = &played[from..from + 11_000];
            let measured = frequency(held);
            let expected = HZ * ((f64::from(*key) - 69.0) / 12.0).exp2();
            let cents = 1200.0 * (measured / expected).log2();
            let level = 20.0 * (f64::from(peak(held)) / 0.5).log10();
            println!(
                "{file_rate} Hz file at {SAMPLE_RATE} Hz, key {key}: {measured:.3} Hz for {expected:.3} Hz, {cents:+.4} cents, level {level:+.3} dB"
            );
            assert!(cents.abs() < 0.01, "{file_rate} key {key}: {cents} cents");
            // A key far above the root of a file at a low rate takes a little off the top.
            assert!(level.abs() < 0.1, "{file_rate} key {key}: {level} dB");
        }
    }
}

/// At the root, from a file at the engine's rate, the sample comes out sample for sample.
#[test]
fn the_root_plays_the_file_as_it_is() {
    let sample: Vec<f32> = (0..20_000)
        .map(|frame| (frame as f32 * 0.37).sin() * 0.8)
        .collect();
    let state = SamplerState {
        attack_seconds: 0.001,
        velocity_to_volume: 0.0,
        ..SamplerState::default()
    };
    let notes = vec![note(0, 15_000, 60, 100)];
    let mut harness = Harness::playing(("noise.wav", SAMPLE_RATE, sample.clone()), notes, state);
    let played = harness.play(15_000);
    // After the attack of 1 ms, 48 frames: the envelope is at full level and the sustain is 1.
    assert_eq!(played[100..14_000], sample[100..14_000]);
}

/// The bend moves the note that sounds, two semitones either way at full bend, and the vibrato
/// of the mod wheel is the synth's: both come from `sound_notes::Wheels`.
#[test]
fn a_full_bend_moves_the_note_two_semitones() {
    let sample = sine(SAMPLE_RATE, HZ, 0.5, 3.0, 0.0);
    let state = SamplerState {
        root: Pitch::new(69).unwrap(),
        ..SamplerState::default()
    };
    let mut harness = Harness::with_samples(&[("a4.wav", SAMPLE_RATE, sample)]);
    let mut changes = sound_core::Changes::new();
    // Bent up from the first frame, and down from tick 960, frame 24 000.
    let track = Track {
        notes: vec![note(0, 48_000, 69, 127)],
        bend: vec![(0, 8191), (960, -8192)],
    };
    let track = changes.create(id("track"), track);
    let sampler = playing("a4.wav", state);
    changes.create(track.id().child(INSTRUMENT).unwrap(), sampler);
    harness.project.commit("Add track", changes).unwrap();
    let played = harness.play(48_000);
    for (frames, semitones) in [(1_000..23_000, 2.0), (25_000..47_000, -2.0)] {
        let measured = frequency(&played[frames]);
        let expected = HZ * (semitones / 12.0_f64).exp2();
        let cents = 1200.0 * (measured / expected).log2();
        assert!(cents.abs() < 0.1, "{semitones}: {cents} cents");
    }
}
