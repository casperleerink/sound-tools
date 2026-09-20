//! Notes from the transport: start frame, pitch, release, chords, voice stealing and the
//! response to pause, stop and seek.

use instrument::{SynthState, VOICES, Waveform};
use sound_core::{Changes, Tempo, TempoMap, Ticks, TimeSignature};
use sound_notes::Pitch;

use crate::support::{
    Harness, SAMPLE_RATE, largest_step, level_at, note, peak, rising_zero_crossings,
};

const SECOND: usize = SAMPLE_RATE as usize;

/// No filter peak and short envelope times, so levels and times are easy to reason about.
fn plain(waveform: Waveform) -> SynthState {
    SynthState {
        waveform,
        cutoff_hz: 1_000.0,
        resonance: 0.0,
        attack_seconds: 0.005,
        decay_seconds: 0.1,
        sustain: 0.7,
        release_seconds: 0.1,
        gain: 0.25,
    }
}

fn hz(pitch: u8) -> f32 {
    Pitch::new(pitch).unwrap().frequency_hz()
}

#[test]
fn a_note_starts_on_the_frame_of_its_tick_at_the_right_pitch() {
    // At 120 bpm tick 1000 is 1000/960 of half a second. At 93.5 bpm it is frame 32 085.56,
    // and a tick lands on the frame that contains it. Neither is the start of a block.
    let cases = [
        (120.0, 69, 25_000, 440),
        (93.5, 60, 32_085, 262),
        (93.5, 45, 32_085, 110),
    ];
    for waveform in [Waveform::Saw, Waveform::Square] {
        for (bpm, pitch, start, cycles_per_second) in cases {
            let mut harness =
                Harness::with_track(vec![note(1000, 9600, pitch, 100)], plain(waveform));
            let mut changes = Changes::new();
            let time_signature = TimeSignature::new(4, 4).unwrap();
            let tempo = Tempo::from_bpm(bpm).unwrap();
            changes.set_tempo_map(TempoMap::constant(time_signature, tempo));
            harness.project.commit("Set tempo", changes).unwrap();
            let clock = harness.project.engine().clock().clone();
            assert_eq!(clock.frame_of(Ticks(1000)).0, start as u64);

            let output = harness.play(start + 2 * SECOND);
            assert_eq!(peak(&output[..start]), 0.0, "{waveform:?} {pitch}: early");
            assert_ne!(output[start], 0.0, "{waveform:?} {pitch}: late");

            let held = &output[start + SECOND / 2..start + SECOND / 2 + SECOND];
            let cycles = rising_zero_crossings(held).len();
            assert!(
                cycles.abs_diff(cycles_per_second) <= 1,
                "{waveform:?} {pitch}: {cycles} cycles in a second"
            );
            let fundamental = level_at(held, hz(pitch));
            assert!(fundamental > 0.05, "{waveform:?} {pitch}: {fundamental}");
        }
    }
}

#[test]
fn after_note_off_and_the_release_time_the_synth_is_silent() {
    // The note ends at tick 960, frame 24 000. The release is 0.1 s, 4800 frames.
    let mut harness = Harness::with_track(vec![note(0, 960, 69, 127)], plain(Waveform::Saw));
    let output = harness.play(SECOND);
    let (note_off, release) = (24_000, 4_800);
    assert!(peak(&output[note_off - 480..note_off]) > 0.1);

    // Exact zeros: every voice is idle, not just quiet. The note was at the sustain level,
    // so it ends a little before the full release time, and not right away.
    assert_eq!(peak(&output[note_off + release..]), 0.0);
    let last_sound = output.iter().rposition(|sample| *sample != 0.0).unwrap();
    let sounded = last_sound - note_off;
    assert!((release * 8 / 10..release).contains(&sounded), "{sounded}");

    // The release is a fade, not a cut.
    let early = peak(&output[note_off..note_off + 480]);
    let late = peak(&output[note_off + 2_400..note_off + 2_880]);
    assert!(early > 4.0 * late && late > 0.0, "{early} {late}");
}

#[test]
fn a_chord_is_the_sum_of_its_notes() {
    let chord = [60, 64, 67];
    let single_renders: Vec<Vec<f32>> = chord
        .iter()
        .map(|pitch| {
            let notes = vec![note(100, 960, *pitch, 100)];
            Harness::with_track(notes, plain(Waveform::Saw)).play(SECOND)
        })
        .collect();
    let notes = chord.iter().map(|pitch| note(100, 960, *pitch, 100));
    let together = Harness::with_track(notes.collect(), plain(Waveform::Saw)).play(SECOND);

    for (frame, sample) in together.iter().enumerate() {
        let sum: f32 = single_renders.iter().map(|render| render[frame]).sum();
        assert!((sample - sum).abs() < 1e-5, "frame {frame}: {sample} {sum}");
    }
    for pitch in chord {
        let level = level_at(&together[4_800..24_000], hz(pitch));
        assert!(level > 0.05, "pitch {pitch}: {level}");
    }
}

#[test]
fn one_note_too_many_takes_over_the_oldest_voice() {
    // Four more notes than voices, one every eighth of a second, all held for five seconds.
    let count = VOICES as u64 + 4;
    let notes =
        (0..count).map(|index| note(index * 240, 9_600 - index * 240, 40 + index as u8, 100));
    let mut harness = Harness::with_track(notes.collect(), plain(Waveform::Saw));
    let output = harness.play(6 * SECOND);

    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(peak(&output) < 4.0, "{}", peak(&output));

    // While only the first 16 play, the first note sounds. Once all 20 have started, the
    // first four are gone and the rest sound, the last four included.
    let before = &output[SECOND..2 * SECOND];
    let after = &output[3 * SECOND..5 * SECOND];
    assert!(level_at(before, hz(40)) > 0.05);
    for index in 0..count as u8 {
        let level = level_at(after, hz(40 + index));
        if index < 4 {
            assert!(level < 0.01, "note {index} still sounds: {level}");
        } else {
            assert!(level > 0.05, "note {index} is missing: {level}");
        }
    }

    // All 20 note offs arrive at tick 9600, frame 240 000. None is left hanging.
    assert_eq!(peak(&output[240_000 + 4_800..]), 0.0);
}

#[test]
fn a_voice_taken_over_does_not_click() {
    // Low notes through a low filter move slowly, so a click would stand out. The 17th note
    // takes over the first voice while it is held.
    let quiet = SynthState {
        cutoff_hz: 100.0,
        attack_seconds: 0.05,
        ..plain(Waveform::Saw)
    };
    let count = VOICES as u64 + 1;
    let notes =
        (0..count).map(|index| note(index * 240, 9_600, 33 + index as u8, 100 - index as u8));
    let output = Harness::with_track(notes.collect(), quiet).play(3 * SECOND);
    let takeover = 16 * 6_000;
    let usual = largest_step(&output[takeover - SECOND / 2..takeover - 1]);
    let around = largest_step(&output[takeover - 1..takeover + SECOND / 2]);
    assert!(around < 1.5 * usual, "{around} {usual}");
}

#[test]
fn pause_stop_and_seek_release_held_notes() {
    type Action = fn(&mut Harness);
    let actions: [(&str, Action); 3] = [
        ("pause", |harness| harness.project.engine().pause()),
        ("stop", |harness| harness.project.engine().stop()),
        // Into the middle of the same long note: nothing starts there, so nothing sounds.
        ("seek", |harness| {
            harness.project.engine().seek(Ticks(40_000))
        }),
    ];
    for (name, action) in actions {
        let notes = vec![note(0, 96_000, 57, 100), note(0, 96_000, 64, 100)];
        let mut harness = Harness::with_track(notes, plain(Waveform::Square));
        let before = harness.play(SECOND / 2);
        assert!(peak(&before[SECOND / 4..]) > 0.1, "{name}");

        action(&mut harness);
        let after = harness.render(SECOND / 2);
        // The release of 0.1 s sounds, then nothing. One block of 64 frames for the command.
        assert!(peak(&after[..480]) > 0.05, "{name}");
        assert_eq!(peak(&after[4_800 + 64..]), 0.0, "{name}");
    }
}

#[test]
fn notes_play_again_after_a_stop() {
    let mut harness = Harness::with_track(vec![note(0, 960, 69, 100)], plain(Waveform::Saw));
    let first = harness.play(SECOND);
    harness.project.engine().stop();
    harness.render(SECOND / 10);
    let second = harness.play(SECOND);
    assert_eq!(first, second);
}

#[test]
fn a_note_that_starts_where_the_same_pitch_ends_is_held() {
    // Tick 480 is frame 12 000. The second note is first in the list on purpose: the sender
    // sends the off of the first note before the on of the second, whatever their order.
    let notes = vec![note(480, 480, 69, 100), note(0, 480, 69, 100)];
    let output = Harness::with_track(notes, plain(Waveform::Saw)).play(SECOND);
    // Well past the release of the first note, the second still sounds. Then it ends too.
    assert!(peak(&output[20_000..24_000]) > 0.05);
    assert_eq!(peak(&output[24_000 + 4_800..]), 0.0);
}
