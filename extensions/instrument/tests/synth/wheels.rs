//! The wheels: the bend moves every note up to two semitones, the mod wheel adds a vibrato, and
//! an `AllOff` puts both back at rest.

use instrument::{SynthState, Waveform};
use sound_core::Ticks;
use sound_notes::Pitch;

use crate::support::{Harness, SAMPLE_RATE, Track, id, note, rising_zero_crossings};

const SECOND: usize = SAMPLE_RATE as usize;
/// At 120 bpm: 25 frames a tick.
const FRAMES_PER_TICK: usize = 25;

fn plain() -> SynthState {
    SynthState {
        waveform: Waveform::Saw,
        cutoff_hz: 1_000.0,
        resonance: 0.0,
        attack_seconds: 0.005,
        decay_seconds: 0.05,
        sustain: 1.0,
        release_seconds: 0.05,
        gain: 0.25,
    }
}

fn played(track: Track) -> Harness {
    let mut harness = Harness::new();
    let mut changes = sound_core::Changes::new();
    let track = changes.create(id("track"), track);
    changes.create(track.id().child("instrument").unwrap(), plain());
    harness.project.commit("Add track", changes).unwrap();
    harness
}

/// Cycles per second in `frames` of `output`.
fn frequency(output: &[f32], frames: std::ops::Range<usize>) -> f32 {
    let seconds = frames.len() as f32 / SAMPLE_RATE as f32;
    rising_zero_crossings(&output[frames]).len() as f32 / seconds
}

fn hz(pitch: u8, semitones: f32) -> f32 {
    Pitch::new(pitch).unwrap().frequency_hz() * (semitones / 12.0).exp2()
}

fn assert_near(measured: f32, expected: f32, what: &str) {
    // A zero crossing more or less in half a second.
    assert!(
        (measured - expected).abs() <= 2.5,
        "{what}: {measured} Hz, not {expected} Hz"
    );
}

/// The bend moves a note that sounds, and a note that starts after it moved starts bent.
#[test]
fn a_full_bend_moves_every_note_two_semitones() {
    let second = SECOND / FRAMES_PER_TICK;
    let track = Track {
        notes: vec![
            note(0, 4 * second as u64, 69, 100),
            note(4 * second as u64, 2 * second as u64, 57, 100),
        ],
        bend: vec![(second as u64, 8191), (3 * second as u64, -8192)],
        ..Track::default()
    };
    let output = played(track).play(6 * SECOND);
    let half = SECOND / 2;
    let at = |second: usize| second * SECOND + half / 2..second * SECOND + half / 2 + half;
    assert_near(frequency(&output, at(0)), hz(69, 0.0), "before the bend");
    assert_near(frequency(&output, at(1)), hz(69, 2.0), "bent up");
    assert_near(frequency(&output, at(3)), hz(69, -2.0), "bent down");
    assert_near(
        frequency(&output, at(4)),
        hz(57, -2.0),
        "a new note under the bend",
    );
}

/// With the wheel all the way up the pitch swings half a semitone either way, and without it
/// every cycle is as long as the next.
#[test]
fn the_mod_wheel_adds_a_vibrato_of_half_a_semitone() {
    let periods = |mod_wheel: Vec<(u64, u8)>| {
        let track = Track {
            notes: vec![note(0, 3_840, 69, 100)],
            mod_wheel,
            ..Track::default()
        };
        let output = played(track).play(2 * SECOND);
        let crossings = rising_zero_crossings(&output[SECOND / 2..SECOND / 2 + SECOND]);
        let periods = crossings.windows(2).map(|pair| pair[1] - pair[0]);
        let shortest = periods.clone().min().unwrap();
        (shortest, periods.max().unwrap())
    };
    // 440 Hz is 109.1 frames a cycle; half a semitone either way is 106.0 and 112.3.
    let (shortest, longest) = periods(Vec::new());
    assert!(shortest >= 108 && longest <= 110, "{shortest} to {longest}");
    let (shortest, longest) = periods(vec![(0, 127)]);
    assert!(shortest <= 107 && longest >= 111, "{shortest} to {longest}");
    assert!(shortest >= 105 && longest <= 113, "{shortest} to {longest}");
}

/// A seek sends `AllOff`, which puts the wheels at rest: a note after it plays in tune, though
/// the wheel was moved before it and never came back.
#[test]
fn all_off_puts_the_bend_back_in_the_middle() {
    let track = Track {
        notes: vec![note(0, 1_920, 69, 100), note(40_000, 9_600, 69, 100)],
        bend: vec![(0, 8191)],
        ..Track::default()
    };
    let mut harness = played(track);
    let bent = harness.play(SECOND);
    assert_near(
        frequency(&bent, SECOND / 4..3 * SECOND / 4),
        hz(69, 2.0),
        "bent",
    );
    harness.project.engine().seek(Ticks(40_000));
    let output = harness.render(SECOND);
    assert_near(
        frequency(&output, SECOND / 4..3 * SECOND / 4),
        hz(69, 0.0),
        "after the seek",
    );
}
