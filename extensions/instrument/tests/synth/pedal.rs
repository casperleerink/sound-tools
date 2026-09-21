//! The sustain pedal: while it is down a note off does not release, and the note sounds until
//! the pedal comes up. Nothing may hang: an `AllOff` puts the pedal up as well.

use instrument::{SynthState, Waveform};
use sound_core::Ticks;

use crate::support::{Harness, SAMPLE_RATE, Track, id, note, peak, pedal};

const SECOND: usize = SAMPLE_RATE as usize;

/// Short envelope times and full sustain, so a held note is loud and a released one is silent
/// within a tenth of a second.
fn plain() -> SynthState {
    SynthState {
        waveform: Waveform::Saw,
        cutoff_hz: 2_000.0,
        resonance: 0.0,
        attack_seconds: 0.005,
        decay_seconds: 0.05,
        sustain: 1.0,
        release_seconds: 0.1,
        gain: 0.25,
    }
}

/// A track with one note of a quarter (tick 0 to 960, frame 0 to 24 000 at 120 bpm) and the
/// pedal moves it is played with.
fn played(pedal_moves: Vec<sound_notes::PedalChange>) -> Harness {
    let mut harness = Harness::new();
    let mut changes = sound_core::Changes::new();
    let track = changes.create(
        id("track"),
        Track {
            notes: vec![note(0, 960, 69, 100)],
            pedal: pedal_moves,
        },
    );
    changes.create(track.id().child("instrument").unwrap(), plain());
    harness.project.commit("Add track", changes).unwrap();
    harness
}

#[test]
fn a_note_off_under_the_pedal_holds_until_the_pedal_comes_up() {
    // Pedal down before the note, up at tick 1920, frame 48 000.
    let mut harness = played(vec![pedal(0, 127), pedal(1920, 0)]);
    let output = harness.play(3 * SECOND);
    // The key came up at frame 24 000 and the note still sounds a tenth of a second later.
    assert!(peak(&output[26_000..28_000]) > 0.05);
    // The pedal came up at frame 48 000 and the release has run by 53 000.
    assert!(peak(&output[46_000..48_000]) > 0.05);
    assert_eq!(peak(&output[54_000..]), 0.0);
}

#[test]
fn without_the_pedal_the_same_note_ends_at_its_note_off() {
    let mut harness = played(Vec::new());
    let output = harness.play(3 * SECOND);
    assert!(peak(&output[22_000..24_000]) > 0.05);
    assert_eq!(peak(&output[30_000..]), 0.0);
}

#[test]
fn a_pedal_below_the_threshold_does_not_hold() {
    let mut harness = played(vec![pedal(0, 63)]);
    let output = harness.play(2 * SECOND);
    assert_eq!(peak(&output[30_000..]), 0.0);
}

/// A stop sends `AllOff`, which puts the pedal up too. Without that the note would hang: the
/// pedal move that would lift it lies after the stop and is never reached.
#[test]
fn a_stop_under_the_pedal_leaves_no_note_hanging() {
    let mut harness = played(vec![pedal(0, 127), pedal(3840, 0)]);
    let mut output = harness.play(SECOND);
    harness.project.engine().stop();
    output.extend(harness.render(SECOND));
    let stop = SECOND;
    assert!(peak(&output[stop - 2_000..stop]) > 0.05);
    assert_eq!(peak(&output[stop + 6_000..]), 0.0);
}

/// A note played again after the pedal came up is not held by it any more.
#[test]
fn the_pedal_comes_up_before_a_new_note_and_that_note_is_not_held() {
    let mut harness = Harness::new();
    let mut changes = sound_core::Changes::new();
    let track = changes.create(
        id("track"),
        Track {
            notes: vec![note(0, 480, 69, 100), note(1920, 480, 72, 100)],
            pedal: vec![pedal(0, 127), pedal(960, 0)],
        },
    );
    changes.create(track.id().child("instrument").unwrap(), plain());
    harness.project.commit("Add track", changes).unwrap();
    let output = harness.play(3 * SECOND);
    // The first note is held past its own end (frame 12 000) until the pedal lifts at 24 000.
    assert!(peak(&output[20_000..24_000]) > 0.05);
    // The second note runs from frame 48 000 to 60 000 and is silent soon after.
    assert!(peak(&output[50_000..54_000]) > 0.05);
    assert_eq!(peak(&output[66_000..]), 0.0);
    let _ = Ticks(0);
}
