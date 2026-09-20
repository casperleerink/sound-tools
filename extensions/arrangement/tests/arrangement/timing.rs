//! Notes land on the frame the clock gives for their tick.

use sound_core::Changes;

use crate::support::{Harness, TICK, clip, level_changes, note, tempo};

#[test]
fn notes_start_and_end_on_the_frames_of_their_ticks() {
    // The clip starts at beat 2. Its notes count from there.
    let notes = vec![note(0, 480, 60), note(480, 480, 64), note(1000, 7, 72)];
    let mut harness = Harness::with_clips(vec![clip(960, 1920, notes)]);
    let output = harness.play(3 * 960 * TICK);
    assert_eq!(
        level_changes(&output),
        [
            (960 * TICK, 60.0),
            (1440 * TICK, 64.0),
            (1920 * TICK, 0.0),
            (1960 * TICK, 72.0),
            (1967 * TICK, 0.0),
        ]
    );
}

#[test]
fn another_tempo_moves_every_note() {
    let mut harness = Harness::with_clips(vec![clip(960, 960, vec![note(0, 480, 60)])]);
    let mut changes = Changes::new();
    changes.set_tempo_map(tempo(60.0));
    harness.project.commit("Set tempo", changes).unwrap();
    let output = harness.play(4 * 960 * TICK);
    // Half the tempo: a tick is twice as many frames.
    assert_eq!(
        level_changes(&output),
        [(960 * 2 * TICK, 60.0), (1440 * 2 * TICK, 0.0)]
    );
}

#[test]
fn a_tempo_change_during_playback_moves_the_notes_that_follow() {
    let notes = vec![note(0, 240, 60), note(960, 240, 64)];
    let mut harness = Harness::with_clips(vec![clip(0, 3840, notes)]);
    let mut output = harness.play(480 * TICK);
    let mut changes = Changes::new();
    changes.set_tempo_map(tempo(60.0));
    harness.project.commit("Set tempo", changes).unwrap();
    output.extend(harness.render(960 * 2 * TICK));
    // The playhead is at tick 480 when the tempo halves. Tick 960 is 480 slow ticks later.
    let second = 480 * TICK + 480 * 2 * TICK;
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (240 * TICK, 0.0),
            (second, 64.0),
            (second + 240 * 2 * TICK, 0.0),
        ]
    );
}

#[test]
fn a_note_that_is_longer_than_the_rest_of_its_clip_ends_with_the_clip() {
    let mut harness = Harness::with_clips(vec![clip(0, 960, vec![note(480, 4800, 60)])]);
    let output = harness.play(2 * 960 * TICK);
    assert_eq!(
        level_changes(&output),
        [(480 * TICK, 60.0), (960 * TICK, 0.0)]
    );
}

#[test]
fn a_note_is_found_among_the_notes_of_a_hundred_clips() {
    // A hundred clips of one bar, sixteen notes each. Each note has the pitch of its clip.
    let clips = (0..100).map(|index| {
        let notes = (0..16).map(|step| note(step * 240, 120, 20 + index as u8));
        clip(index * 3840, 3840, notes.collect())
    });
    let mut harness = Harness::with_clips(clips.collect());
    // Seek to the last sixteenth of clip 57 and play a little more than it.
    harness
        .project
        .engine()
        .seek(sound_core::Ticks(57 * 3840 + 15 * 240));
    let output = harness.play(300 * TICK);
    assert_eq!(
        level_changes(&output),
        [(0, 77.0), (120 * TICK, 0.0), (240 * TICK, 78.0)]
    );
}
