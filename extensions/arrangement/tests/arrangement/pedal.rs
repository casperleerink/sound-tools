//! The sustain pedal of a clip: it holds notes on playback, it follows a seek, and a stop or
//! an edit leaves nothing hanging under a pedal nobody will lift.

use sound_core::Ticks;

use crate::support::{Harness, TICK, clip, clip_json, clip_with_pedal, level_changes, note};

const CLIP: &str = "state/arrangement/piano/clip-0.json";

/// A bar with a short note at tick 0 and the pedal down from tick 0 to tick 1920. The note is
/// one beat long, so without the pedal it would be silent from tick 960.
fn pedalled_bar() -> Harness {
    let notes = vec![note(0, 960, 60)];
    let pedal = [(0, 127), (1920, 0)];
    Harness::with_clips(vec![clip_with_pedal(0, 3840, notes, &pedal)])
}

#[test]
fn the_pedal_of_a_clip_holds_its_notes_until_it_comes_up() {
    let mut harness = pedalled_bar();
    let output = harness.play(3840 * TICK);
    assert_eq!(level_changes(&output), [(0, 60.0), (1920 * TICK, 0.0)]);
}

/// The same clip without the pedal, so the difference is the pedal and nothing else.
#[test]
fn the_same_clip_without_the_pedal_ends_the_note_at_its_own_end() {
    let mut harness = Harness::with_clips(vec![clip(0, 3840, vec![note(0, 960, 60)])]);
    let output = harness.play(3840 * TICK);
    assert_eq!(level_changes(&output), [(0, 60.0), (960 * TICK, 0.0)]);
}

/// A seek into the middle of a pedal takes the pedal with it. Notes are not chased, but the
/// pedal is: it is one value, so there is nothing to work out.
#[test]
fn a_seek_into_a_held_pedal_arrives_with_the_pedal_down() {
    let notes = vec![note(0, 960, 60), note(1440, 240, 67)];
    let pedal = [(0, 127), (2880, 0)];
    let mut harness = Harness::with_clips(vec![clip_with_pedal(0, 3840, notes, &pedal)]);
    harness.project.engine().seek(Ticks(1200));
    let output = harness.play(2880 * TICK);
    // The note at 1440 starts and is held by the pedal until it comes up at 2880.
    assert_eq!(
        level_changes(&output),
        [(240 * TICK, 67.0), (1680 * TICK, 0.0)]
    );
}

#[test]
fn a_stop_puts_the_pedal_up_and_playing_again_starts_from_no_pedal() {
    let mut harness = pedalled_bar();
    let mut output = harness.play(1200 * TICK);
    harness.project.engine().stop();
    output.extend(harness.render(480 * TICK));
    // The stop released the note at once, though the clip holds the pedal down there.
    assert_eq!(level_changes(&output), [(0, 60.0), (1200 * TICK, 0.0)]);

    // From zero again, the pedal comes back with the clip.
    harness.project.engine().play();
    let again = harness.render(3840 * TICK);
    assert_eq!(level_changes(&again), [(0, 60.0), (1920 * TICK, 0.0)]);
}

/// The pedal is removed from the clip while a note hangs on it. The sequencer compares the
/// pedal of the snapshot with what it sent, so the note is released at once.
#[test]
fn an_edit_that_removes_the_pedal_releases_what_it_held() {
    let mut harness = pedalled_bar();
    let at = 1200 * TICK;
    let mut output = harness.play(at);
    let edited = clip(0, 3840, vec![note(0, 960, 60)]);
    harness.write_and_apply(CLIP, &clip_json(&edited));
    output.extend(harness.render(3840 * TICK - at));
    assert_eq!(level_changes(&output), [(0, 60.0), (at, 0.0)]);
}

/// Half pedal is saved and sent as it was played. The probe, like the synth, has one damper
/// and uses the same rule as MIDI: down from 64.
#[test]
fn a_pedal_below_64_does_not_hold() {
    let notes = vec![note(0, 960, 60)];
    let mut harness = Harness::with_clips(vec![clip_with_pedal(0, 3840, notes, &[(0, 63)])]);
    let output = harness.play(3840 * TICK);
    assert_eq!(level_changes(&output), [(0, 60.0), (960 * TICK, 0.0)]);
}

/// The runtime writes the pedal in the form the agent doc shows: one move per line, with the
/// spaces the runtime uses everywhere. A clip with no pedal keeps the field out of its file.
#[test]
fn the_runtime_writes_the_pedal_one_move_per_line() {
    let notes = vec![note(0, 960, 60)];
    let pedal = [(0, 127), (1920, 0)];
    let recorded = clip_with_pedal(0, 3840, notes.clone(), &pedal);
    let harness = Harness::with_clips(vec![recorded, clip(3840, 3840, notes)]);
    assert_eq!(harness.problems(), Vec::<String>::new());
    let text = std::fs::read_to_string(harness.path(CLIP)).unwrap();
    assert!(text.contains(r#"{"start": 0, "value": 127}"#), "{text}");
    assert!(text.contains(r#"{"start": 1920, "value": 0}"#), "{text}");
    let plain = std::fs::read_to_string(harness.path("state/arrangement/piano/clip-1.json"));
    assert!(!plain.unwrap().contains("pedal"));
}

/// A clip that an agent writes by hand loads, and the runtime does not write it back: the
/// state it decodes to is the state it already has.
#[test]
fn a_pedal_record_written_by_hand_loads_and_is_not_rewritten() {
    let written = clip_with_pedal(0, 3840, vec![note(0, 960, 60)], &[(0, 127)]);
    let mut harness = Harness::new();
    harness.add_track("piano", 1.0);
    let json = clip_json(&written);
    assert_eq!(harness.write_and_apply(CLIP, &json), 1);
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert_eq!(std::fs::read_to_string(harness.path(CLIP)).unwrap(), json);
}
