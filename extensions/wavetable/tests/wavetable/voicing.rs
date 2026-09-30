//! Poly and mono, legato, glide, the pedal, the polyphony, and that nothing is ever stuck.

use wavetable::WavetableState;
use wavetable::state::{Oscillator, VoiceMode, Voicing};

use crate::sound::plain;
use crate::support::{Harness, Track, frames, level_at, loudest_hz, note, peak, pedal, rms, ticks};

fn hz(pitch: u8) -> f32 {
    440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2()
}

fn with_voicing(voicing: Voicing) -> WavetableState {
    WavetableState {
        voicing,
        ..plain(Oscillator::default())
    }
}

const MONO: Voicing = Voicing {
    mode: VoiceMode::Mono,
    polyphony: 8,
    glide_seconds: 0.0,
};

/// The level of the fundamental of each of `pitches` in `samples`.
fn fundamentals(samples: &[f32], pitches: &[u8]) -> Vec<f32> {
    pitches
        .iter()
        .map(|pitch| level_at(samples, hz(*pitch)))
        .collect()
}

#[test]
fn a_chord_plays_every_note_in_poly_and_one_in_mono() {
    let chord: Vec<_> = [60, 64, 67]
        .map(|pitch| note(0, ticks(1.0), pitch, 127))
        .to_vec();
    let mut poly = Harness::with_notes(chord.clone(), plain(Oscillator::default()));
    let steady = poly.play_left(frames(0.8))[frames(0.5)..].to_vec();
    assert!(
        fundamentals(&steady, &[60, 64, 67])
            .iter()
            .all(|level| *level > 0.02)
    );

    let mut mono = Harness::with_notes(chord, with_voicing(MONO));
    let steady = mono.play_left(frames(0.8))[frames(0.5)..].to_vec();
    let levels = fundamentals(&steady, &[60, 64, 67]);
    assert_eq!(
        levels.iter().filter(|level| **level > 0.02).count(),
        1,
        "{levels:?}"
    );
}

/// Mono: a key pressed while another is held moves the note there without starting its
/// envelope again, and letting go of it goes back to the key still held.
#[test]
fn mono_moves_a_held_note_without_starting_it_again() {
    let notes = vec![
        note(0, ticks(1.5), 60, 127),
        note(ticks(0.5), ticks(0.5), 67, 127),
    ];
    let mut harness = Harness::with_notes(notes, with_voicing(MONO));
    let left = harness.play_left(frames(1.4));
    let level = |from: f32| rms(&left[frames(from)..frames(from + 0.05)]);
    // At its sustain before and after the second key: no new attack.
    assert!((level(0.4) / level(0.55) - 1.0).abs() < 0.05);
    let pitch = |from: f32| loudest_hz(&left[frames(from)..frames(from) + 8_192], 200.0, 450.0);
    assert!(
        (pitch(0.55) / hz(67) - 1.0).abs() < 0.003,
        "{}",
        pitch(0.55)
    );
    assert!((pitch(1.1) / hz(60) - 1.0).abs() < 0.003, "{}", pitch(1.1));
}

#[test]
fn a_glide_slides_from_the_last_note_to_the_new_one() {
    let voicing = Voicing {
        glide_seconds: 0.3,
        ..Voicing::default()
    };
    let notes = vec![
        note(0, ticks(0.5), 60, 127),
        note(ticks(0.5), ticks(1.0), 72, 127),
    ];
    let mut harness = Harness::with_notes(notes, with_voicing(voicing));
    let left = harness.play_left(frames(1.4));
    let pitch = |from: f32| {
        let window = &left[frames(from)..frames(from) + 2_048];
        12.0 * (loudest_hz(window, 200.0, 600.0) / hz(60)).log2()
    };
    // Half way through the glide, about half way there; after it, there.
    let middle = pitch(0.63);
    assert!((4.0..8.0).contains(&middle), "{middle}");
    assert!((pitch(1.0) - 12.0).abs() < 0.05, "{}", pitch(1.0));
}

#[test]
fn the_pedal_holds_a_note_until_it_comes_up() {
    let track = Track {
        notes: vec![note(0, ticks(0.2), 60, 127)],
        pedal: vec![pedal(0, 127), pedal(ticks(0.6), 0)],
        ..Track::default()
    };
    let mut harness = Harness::with(track, plain(Oscillator::default()));
    let left = harness.play_left(frames(1.2));
    assert!(peak(&left[frames(0.5)..frames(0.6)]) > 0.02);
    // The release takes 0.3 s from the pedal.
    assert_eq!(peak(&left[frames(0.95)..]), 0.0);
}

/// Two notes may play; a third takes over the oldest, which sounds out over its release.
#[test]
fn the_polyphony_limits_the_notes_that_play() {
    let voicing = Voicing {
        polyphony: 2,
        ..Voicing::default()
    };
    let notes = vec![
        note(0, ticks(2.0), 60, 127),
        note(0, ticks(2.0), 64, 127),
        note(ticks(0.3), ticks(2.0), 67, 127),
    ];
    let mut harness = Harness::with_notes(notes, with_voicing(voicing));
    let left = harness.play_left(frames(1.5));
    let later = fundamentals(&left[frames(1.0)..], &[60, 64, 67]);
    assert!(later[0] < 1e-4, "{later:?}");
    assert!(later[1] > 0.02 && later[2] > 0.02, "{later:?}");
}

/// A stop sends `AllOff`: every voice, held or under the pedal, ends within its release, and
/// then the synth makes exact silence.
#[test]
fn a_stop_silences_every_voice_within_its_release() {
    let track = Track {
        notes: [60, 64, 67, 71]
            .map(|pitch| note(0, ticks(10.0), pitch, 127))
            .to_vec(),
        pedal: vec![pedal(0, 127)],
        ..Track::default()
    };
    let mut harness = Harness::with(track, WavetableState::default());
    harness.play(frames(0.5));
    harness.project.engine().stop();
    let [left, right] = harness.render(frames(1.0));
    assert!(peak(&left[..frames(0.1)]) > 0.0);
    // The release takes 0.3 s.
    assert_eq!(peak(&left[frames(0.35)..]), 0.0);
    assert_eq!(peak(&right[frames(0.35)..]), 0.0);
}
