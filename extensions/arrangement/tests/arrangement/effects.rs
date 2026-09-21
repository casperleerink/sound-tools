//! The effects of a track: the chain the record names, what happens to a slot that is not
//! there, and what the mixer still does after all of it.
//!
//! The effect here is the test `Trim` of `support.rs`, not a plugin: the arrangement finds an
//! effect by its ports, so any tool with them fits, and these tests need no plugin at all. The
//! probe holds the pitch of its note as a level, so every sample is arithmetic a test can do.

use arrangement::TrackState;
use sound_core::{Changes, EngineConfig};

use crate::support::{Harness, SAMPLE_RATE, Trim, clip, id, note};

/// Ticks in a bar of 4/4.
const BAR: u64 = 3840;

/// The one note every test here plays. The probe puts out its pitch as a level.
const PITCH: u8 = 60;
const LEVEL: f32 = 60.0;

const TRACK_FILE: &str = "state/arrangement/piano/instance.json";

/// A stereo project with one track holding one long note, playing from its first frame.
fn playing() -> Harness {
    let mut harness = Harness::with_config(EngineConfig::new(SAMPLE_RATE, 2));
    harness.add_track("piano", 1.0);
    let mut changes = Changes::new();
    changes.create(
        id("arrangement/piano/long"),
        clip(0, 8 * BAR, vec![note(0, 8 * BAR, PITCH)]),
    );
    harness.project.commit("Add clip", changes).unwrap();
    harness.project.engine().play();
    harness
}

/// The left channel of a render.
fn left(interleaved: &[f32]) -> Vec<f32> {
    interleaved.iter().step_by(2).copied().collect()
}

/// The level the track plays, once the mixer has finished its ramp. Reading one sample says
/// everything, because the note is held for the whole render.
fn steady(harness: &mut Harness, frames: usize) -> f32 {
    let render = harness.render(frames * 2);
    left(&render)[frames - 1]
}

/// The record of a track with these effects, as an agent writes it.
fn track_record(effects: &[&str]) -> String {
    let names: Vec<String> = effects.iter().map(|name| format!("{name:?}")).collect();
    format!(
        r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "effects": [{}]}}}}"#,
        names.join(", ")
    )
}

/// The record of an effect, as an agent writes it.
fn trim_record(gain: f32, offset: f32) -> String {
    format!(r#"{{"tool": "test.trim", "state": {{"gain": {gain:?}, "offset": {offset:?}}}}}"#)
}

/// Adds an effect to the track through the helper an interface uses, in one group.
fn add_effect(harness: &mut Harness, name: &str, effect: Trim) {
    let track = harness
        .project
        .resolve::<TrackState>(&id("arrangement/piano"));
    let track = track.unwrap();
    let mut changes = Changes::new();
    let slot = arrangement::add_effect(&harness.project, &mut changes, &track, name).unwrap();
    changes.create(slot, effect);
    harness.project.commit("Add effect", changes).unwrap();
}

#[test]
fn one_effect_after_the_instrument_changes_what_the_track_plays() {
    let mut harness = playing();
    let without = steady(&mut harness, 2400);
    assert_eq!(without, LEVEL);

    add_effect(&mut harness, "trim", Trim::new(0.5, 1.0));
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert_eq!(steady(&mut harness, 2400), LEVEL * 0.5 + 1.0);
}

/// The whole of step 6 in one test: the order is the order of the record, and it is audible.
#[test]
fn two_effects_in_one_order_and_in_the_other_give_different_samples() {
    let (first, second) = (Trim::new(0.5, 1.0), Trim::new(0.5, 4.0));
    let mut harness = playing();
    add_effect(&mut harness, "a", first);
    add_effect(&mut harness, "b", second);
    assert_eq!(harness.problems(), Vec::<String>::new());
    // a then b: (level * 0.5 + 1) * 0.5 + 4.
    let a_then_b = (LEVEL * 0.5 + 1.0) * 0.5 + 4.0;
    assert_eq!(steady(&mut harness, 2400), a_then_b);

    // The other way round, as a file edit: one record, one undo step. Both effects and their
    // records are exactly the ones that were there; only the list changed.
    assert_eq!(
        harness.write_and_apply(TRACK_FILE, &track_record(&["b", "a"])),
        1
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let b_then_a = (LEVEL * 0.5 + 4.0) * 0.5 + 1.0;
    assert_eq!(steady(&mut harness, 2400), b_then_a);
    assert_ne!(a_then_b, b_then_a);

    // One undo step for that reorder, and it sounds as it did before.
    assert!(harness.project.undo().unwrap().is_some());
    assert_eq!(steady(&mut harness, 2400), a_then_b);
    let state = harness
        .project
        .state_json(&id("arrangement/piano"))
        .unwrap();
    assert!(state.contains(r#""effects":["a","b"]"#), "{state}");
}

/// A record that leaves the field out is a track of before effects existed. It loads as it is,
/// nothing is written back, and the file keeps its bytes.
#[test]
fn a_track_record_without_the_field_loads_unchanged_and_is_not_rewritten() {
    let mut harness = playing();
    let old = r#"{"tool": "arrangement.track", "state": {"name": "Grand", "order": 0}}"#;
    assert_eq!(harness.write_and_apply(TRACK_FILE, old), 1);
    assert_eq!(harness.problems(), Vec::<String>::new());
    let track = harness
        .project
        .state_json(&id("arrangement/piano"))
        .unwrap();
    assert!(!track.contains("effects"), "{track}");
    assert_eq!(steady(&mut harness, 2400), LEVEL);

    // The bytes on disk are the ones the agent wrote: the record is not written back for a
    // field it does not name, and the empty list is left out of what is written.
    assert_eq!(
        std::fs::read_to_string(harness.path(TRACK_FILE)).unwrap(),
        old
    );

    // And the runtime writes the same record again when something makes it write: an edit of
    // the record itself, undone.
    let renamed = r#"{"tool": "arrangement.track", "state": {"name": "Upright", "order": 0}}"#;
    harness.write_and_apply(TRACK_FILE, renamed);
    assert!(harness.project.undo().unwrap().is_some());
    let written = std::fs::read_to_string(harness.path(TRACK_FILE)).unwrap();
    assert!(!written.contains("effects"), "{written}");
    assert_eq!(
        written,
        "{\n  \"tool\": \"arrangement.track\",\n  \"state\": {\"name\": \"Grand\", \"colour\": \"blue\", \"order\": 0, \"gain_db\": 0.0, \"pan\": 0.0, \"mute\": false}\n}\n"
    );
}

/// A name in the list with no record behind it. The rest of the chain plays, so one missing
/// plugin never silences a track, and the problem says what to write.
#[test]
fn a_listed_effect_with_no_record_is_reported_and_the_sound_goes_through_the_rest() {
    let mut harness = playing();
    add_effect(&mut harness, "trim", Trim::new(0.5, 1.0));
    let with_one = steady(&mut harness, 2400);

    assert_eq!(
        harness.write_and_apply(TRACK_FILE, &track_record(&["gone", "trim"])),
        1
    );
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("no gone.json"), "{problems:?}");
    assert!(
        problems[0].contains("take \"gone\" out of `effects`"),
        "{problems:?}"
    );
    // The track plays through the effect that is there, exactly as it did.
    assert_eq!(steady(&mut harness, 2400), with_one);

    // Correcting the list clears the problem, with no restart.
    harness.write_and_apply(TRACK_FILE, &track_record(&["trim"]));
    assert_eq!(harness.problems(), Vec::<String>::new());
}

/// A record that looks like an effect and is in no list is a mistake an agent should hear
/// about: nothing goes through it, and it would otherwise be silent in every sense.
#[test]
fn a_child_that_looks_like_an_effect_and_is_not_listed_is_reported() {
    let mut harness = playing();
    harness.write_and_apply(
        "state/arrangement/piano/reverb.json",
        &trim_record(0.5, 1.0),
    );
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("\"reverb\""), "{problems:?}");
    assert!(
        problems[0].contains("Add \"reverb\" to `effects`"),
        "{problems:?}"
    );
    // And it is really not in the chain.
    assert_eq!(steady(&mut harness, 2400), LEVEL);

    harness.write_and_apply(TRACK_FILE, &track_record(&["reverb"]));
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert_eq!(steady(&mut harness, 2400), LEVEL * 0.5 + 1.0);
}

/// The mixer is not a device: it is the track, and it acts after every effect.
#[test]
fn the_gain_and_the_mute_of_the_track_act_after_the_chain() {
    let mut harness = playing();
    add_effect(&mut harness, "trim", Trim::new(0.5, 1.0));
    let chained = LEVEL * 0.5 + 1.0;
    assert_eq!(steady(&mut harness, 2400), chained);

    let record = r#"{"tool": "arrangement.track", "state": {"name": "piano", "gain_db": -6.0, "effects": ["trim"]}}"#;
    harness.write_and_apply(TRACK_FILE, record);
    let level = 10_f32.powf(-6.0 / 20.0);
    let played = steady(&mut harness, 2400);
    assert!((played - chained * level).abs() < 1e-4, "{played}");

    let record = r#"{"tool": "arrangement.track", "state": {"name": "piano", "mute": true, "effects": ["trim"]}}"#;
    harness.write_and_apply(TRACK_FILE, record);
    assert_eq!(steady(&mut harness, 2400), 0.0);
}

/// The tail rule. An effect is a processor like any other: the engine runs it every block,
/// whether the project plays or not, so what it holds rings out after a stop. Taking it off
/// the track takes its processor out of the graph, so its tail goes with it at once.
#[test]
fn a_tail_rings_out_after_a_stop_and_goes_at_once_when_the_effect_is_removed() {
    let mut harness = playing();
    let echo = Trim {
        gain: 1.0,
        offset: 0.0,
        tail: 0.5,
    };
    add_effect(&mut harness, "echo", echo);
    // The one-frame feedback settles at level / (1 - tail).
    let settled = LEVEL / 0.5;
    assert_eq!(steady(&mut harness, 2400), settled);

    // A stop silences the instrument and the effect goes on running: what it held decays by
    // half a frame at a time instead of stopping dead.
    harness.project.engine().stop();
    let after = left(&harness.render(64));
    assert!(after[0] > 0.0 && after[0] < settled, "{:?}", &after[..4]);
    assert_eq!(after[1], after[0] * 0.5);
    assert!(after[20] > 0.0, "the tail stopped in 20 frames");

    // Taking the effect off takes its tail with it: the slot is gone from the graph and the
    // track plays what its instrument makes, which after a stop is nothing.
    let track = harness
        .project
        .resolve::<TrackState>(&id("arrangement/piano"));
    let mut changes = Changes::new();
    arrangement::remove_effect(
        &harness.project,
        &mut changes,
        &track.unwrap(),
        &id("arrangement/piano/echo"),
    )
    .unwrap();
    harness.project.commit("Remove effect", changes).unwrap();
    assert_eq!(left(&harness.render(64)), vec![0.0; 32]);
}

/// The two helpers an interface uses. Adding is one group and one undo step, and so is
/// removing, which is what makes undo bring an effect back where it was.
#[test]
fn adding_and_removing_an_effect_are_one_undo_step_each() {
    let mut harness = playing();
    add_effect(&mut harness, "trim", Trim::new(0.5, 1.0));
    let with_one = steady(&mut harness, 2400);
    assert!(harness.path("state/arrangement/piano/trim.json").exists());

    let track = harness
        .project
        .resolve::<TrackState>(&id("arrangement/piano"));
    let mut changes = Changes::new();
    arrangement::remove_effect(
        &harness.project,
        &mut changes,
        &track.unwrap(),
        &id("arrangement/piano/trim"),
    )
    .unwrap();
    harness.project.commit("Remove effect", changes).unwrap();
    assert_eq!(steady(&mut harness, 2400), LEVEL);
    assert!(!harness.path("state/arrangement/piano/trim.json").exists());

    // One undo brings the record and the name in the list back together.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Remove effect")
    );
    assert_eq!(steady(&mut harness, 2400), with_one);
    assert!(harness.path("state/arrangement/piano/trim.json").exists());

    // And one more takes the whole of the add back.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Add effect")
    );
    assert_eq!(steady(&mut harness, 2400), LEVEL);
    assert!(!harness.path("state/arrangement/piano/trim.json").exists());
}

/// What the record refuses, so that an agent is told where its mistake is instead of hearing
/// an order nobody can read.
#[test]
fn a_list_with_no_order_to_read_does_not_load() {
    let mut harness = playing();
    for (effects, expected) in [
        (r#"["a", "a"]"#, "which the list already has"),
        (
            r#"["instrument"]"#,
            "the instrument of a track is its own slot",
        ),
        (r#"["Reverb"]"#, "lowercase letters"),
        (r#"[""]"#, "lowercase letters"),
    ] {
        let record = format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "piano", "effects": {effects}}}}}"#
        );
        harness.write_and_apply(TRACK_FILE, &record);
        let problems = harness.problems();
        assert_eq!(problems.len(), 1, "{effects}: {problems:?}");
        assert!(problems[0].contains(expected), "{effects}: {problems:?}");
        // The live track is untouched: it still plays.
        assert_eq!(steady(&mut harness, 2400), LEVEL);
    }
}
