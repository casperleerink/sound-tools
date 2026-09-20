//! No stuck notes: a note that started gets its off, whatever happens to its clip, the tempo
//! or the transport while it sounds. And a note that nothing happened to is left alone.

use sound_core::{Changes, Ticks};

use crate::support::{Harness, TICK, clip, clip_json, level_changes, note, tempo};

const CLIP: &str = "state/arrangement/piano/clip-0.json";
/// Where every test below makes its change: tick 480, on a device buffer edge, so the change
/// applies on exactly this frame.
const CHANGE: usize = 480 * TICK;

/// A clip of one bar with a note of two beats, played up to tick 480.
fn held_note() -> (Harness, Vec<f32>) {
    let notes = vec![note(0, 1920, 60), note(2880, 480, 67)];
    let mut harness = Harness::with_clips(vec![clip(0, 3840, notes)]);
    let output = harness.play(CHANGE);
    assert_eq!(level_changes(&output), [(0, 60.0)]);
    (harness, output)
}

#[test]
fn a_note_that_is_edited_away_while_it_sounds_ends_at_once() {
    let (mut harness, mut output) = held_note();
    let edited = clip(0, 3840, vec![note(2880, 480, 67)]);
    harness.write_and_apply(CLIP, &clip_json(&edited));
    output.extend(harness.render(3840 * TICK - CHANGE));
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (CHANGE, 0.0),
            (2880 * TICK, 67.0),
            (3360 * TICK, 0.0)
        ]
    );
}

#[test]
fn a_sounding_note_follows_an_edit_of_its_length() {
    for (length, end) in [(960, 960), (2880, 2880), (480, 480), (100, 480)] {
        let (mut harness, mut output) = held_note();
        let edited = clip(0, 3840, vec![note(0, length, 60)]);
        harness.write_and_apply(CLIP, &clip_json(&edited));
        output.extend(harness.render(3840 * TICK - CHANGE));
        assert_eq!(level_changes(&output), [(0, 60.0), (end * TICK, 0.0)]);
    }
}

#[test]
fn a_sounding_note_ends_when_its_start_moves() {
    let (mut harness, mut output) = held_note();
    // The same note, one beat later: what sounds is no longer in the clip, and the note at
    // its new place starts when the playhead gets there.
    let edited = clip(960, 3840, vec![note(0, 1920, 60)]);
    harness.write_and_apply(CLIP, &clip_json(&edited));
    output.extend(harness.render(3840 * TICK - CHANGE));
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (CHANGE, 0.0),
            (960 * TICK, 60.0),
            (2880 * TICK, 0.0)
        ]
    );
}

#[test]
fn a_sounding_note_ends_when_its_clip_is_deleted() {
    let (mut harness, mut output) = held_note();
    std::fs::remove_file(harness.path(CLIP)).unwrap();
    assert_eq!(harness.apply(&[CLIP]), 1);
    output.extend(harness.render(3840 * TICK - CHANGE));
    assert_eq!(level_changes(&output), [(0, 60.0), (CHANGE, 0.0)]);

    // Undo brings the clip back. The note that was cut is not started in its middle.
    harness.project.undo().unwrap();
    output.extend(harness.render(3840 * TICK));
    assert_eq!(level_changes(&output)[2..], []);
}

#[test]
fn a_sounding_note_ends_when_its_clip_moves_to_another_track() {
    let (mut harness, mut output) = held_note();
    harness.add_track("bass", 1000.0);
    let moved = "state/arrangement/bass/clip-0.json";
    std::fs::rename(harness.path(CLIP), harness.path(moved)).unwrap();
    assert_eq!(harness.apply(&[CLIP, moved]), 2);
    output.extend(harness.render(3840 * TICK - CHANGE));
    // The piano is silent at once. The other track plays what starts from here on.
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (CHANGE, 0.0),
            (2880 * TICK, 67_000.0),
            (3360 * TICK, 0.0)
        ]
    );
}

#[test]
fn a_sounding_note_ends_when_its_track_is_deleted() {
    let (mut harness, mut output) = held_note();
    std::fs::remove_dir_all(harness.path("state/arrangement/piano")).unwrap();
    assert_eq!(harness.apply(&["state/arrangement/piano"]), 3);
    output.extend(harness.render(3840 * TICK - CHANGE));
    assert_eq!(level_changes(&output), [(0, 60.0), (CHANGE, 0.0)]);
}

#[test]
fn pause_silences_a_held_note_and_play_goes_on_without_it() {
    let (mut harness, mut output) = held_note();
    harness.project.engine().pause();
    output.extend(harness.render(960 * TICK));
    harness.project.engine().play();
    output.extend(harness.render(3840 * TICK - CHANGE));
    // The pause lasted 960 ticks of engine time. The project went on from tick 480.
    let paused = 960 * TICK;
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (CHANGE, 0.0),
            (paused + 2880 * TICK, 67.0),
            (paused + 3360 * TICK, 0.0)
        ]
    );
}

#[test]
fn stop_silences_a_held_note_and_play_starts_over() {
    let (mut harness, mut output) = held_note();
    harness.project.engine().stop();
    output.extend(harness.render(960 * TICK));
    harness.project.engine().play();
    output.extend(harness.render(2400 * TICK));
    let restart = CHANGE + 960 * TICK;
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (CHANGE, 0.0),
            (restart, 60.0),
            (restart + 1920 * TICK, 0.0)
        ]
    );
}

#[test]
fn seek_silences_a_held_note_and_does_not_start_a_note_in_its_middle() {
    let notes = vec![note(0, 1920, 60), note(1920, 960, 64), note(2880, 480, 67)];
    let mut harness = Harness::with_clips(vec![clip(0, 3840, notes)]);
    let mut output = harness.play(CHANGE);
    // Into the middle of the second note. It is not chased. The third one plays.
    harness.project.engine().seek(Ticks(2400));
    output.extend(harness.render(1440 * TICK));
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (CHANGE, 0.0),
            (CHANGE + 480 * TICK, 67.0),
            (CHANGE + 960 * TICK, 0.0)
        ]
    );
}

#[test]
fn a_held_note_ends_on_its_tick_after_a_tempo_change() {
    let (mut harness, mut output) = held_note();
    let mut changes = Changes::new();
    changes.set_tempo_map(tempo(60.0));
    harness.project.commit("Set tempo", changes).unwrap();
    output.extend(harness.render(1920 * 2 * TICK));
    // 1440 more ticks at half the tempo.
    assert_eq!(
        level_changes(&output),
        [(0, 60.0), (CHANGE + 1440 * 2 * TICK, 0.0)]
    );
}

#[test]
fn the_same_pitch_back_to_back_sounds_without_a_gap_and_ends() {
    let notes = (0..8).map(|index| note(index * 240, 240, 60));
    let mut harness = Harness::with_clips(vec![clip(0, 3840, notes.collect())]);
    let output = harness.play(3840 * TICK);
    // The probe drops to 0 for a frame when an off comes after the on of the same frame, and
    // shows 120 when an off is missing.
    assert_eq!(level_changes(&output), [(0, 60.0), (1920 * TICK, 0.0)]);
}

#[test]
fn clips_that_overlap_both_play() {
    let first = clip(0, 960, vec![note(0, 960, 60)]);
    let second = clip(480, 960, vec![note(0, 960, 64)]);
    let mut harness = Harness::with_clips(vec![first, second]);
    let output = harness.play(1920 * TICK);
    assert_eq!(
        level_changes(&output),
        [
            (0, 60.0),
            (480 * TICK, 124.0),
            (960 * TICK, 64.0),
            (1440 * TICK, 0.0)
        ]
    );
}

#[test]
fn the_same_pitch_in_clips_that_overlap_ends_with_the_first_off() {
    let first = clip(0, 960, vec![note(0, 960, 60)]);
    let second = clip(480, 960, vec![note(0, 960, 60)]);
    let mut harness = Harness::with_clips(vec![first, second]);
    let output = harness.play(1920 * TICK);
    // The note contract: an off releases every held note of its pitch. Nothing is stuck.
    assert_eq!(
        level_changes(&output),
        [(0, 60.0), (480 * TICK, 120.0), (960 * TICK, 0.0)]
    );
}

#[test]
fn more_notes_than_the_held_list_takes_are_counted_and_nothing_is_stuck() {
    let extra = 12;
    let count = (arrangement::HELD_CAPACITY + extra) as u64;
    // Long notes two ticks apart: all of them want to sound at once.
    let notes = (0..count).map(|index| note(index * 2, 2000, (index % 128) as u8));
    let mut harness = Harness::with_clips(vec![clip(0, 3840, notes.collect())]);
    harness.project.engine().play();
    let (output, status) = harness.render_with_status(3840 * TICK);
    assert_eq!(status.event_overflows, extra as u64);
    // All 128 pitches once, while the list is full.
    let full: u32 = (0..128).sum();
    assert_eq!(output[1000 * TICK], full as f32);
    assert_eq!(output.last(), Some(&0.0));

    // The list is free again: the next round plays the same.
    harness.project.engine().seek(Ticks(0));
    let (again, status) = harness.render_with_status(3840 * TICK);
    assert_eq!(status.event_overflows, 2 * extra as u64);
    assert_eq!(again, output);
}

#[test]
fn a_snapshot_swap_leaves_a_held_note_alone() {
    let notes = vec![note(0, 1920, 60), note(1920, 1920, 64)];
    let run = |edits: bool| {
        let mut harness = Harness::with_clips(vec![clip(0, 3840, notes.clone())]);
        harness.project.engine().play();
        let mut output = Vec::new();
        // A drag in another clip: one new snapshot per device buffer, 210 in a row, across
        // the end of the first note and the start of the second.
        for step in 0..210 {
            if edits {
                let other = clip(3840, 3840, vec![note(step, 240, 72)]);
                let changed = harness
                    .write_and_apply("state/arrangement/piano/other.json", &clip_json(&other));
                assert_eq!(changed, 1);
            }
            output.extend(harness.render(480));
        }
        output
    };
    let (quiet, edited) = (run(false), run(true));
    assert_eq!(quiet, edited);
    assert_eq!(
        level_changes(&edited),
        [(0, 60.0), (1920 * TICK, 64.0), (3840 * TICK, 0.0)]
    );
}
