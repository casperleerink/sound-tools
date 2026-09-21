//! What this host holds and what the engine plays must not drift apart.
//!
//! The project applies an edit group whole or not at all, and a group it rejects never reaches
//! the engine. So every sequence here ends the same way: a record that names a plugin this
//! machine has plays, and nothing is reported.

use plugin_host::PluginFormat;
use sound_core::Changes;

use crate::support::{FORMATS, Harness, Picky, Played, id, peak, plugin_id, record};

/// A note every 512 frames for long enough that every render of a test finds notes in it.
/// Engine time never goes back, so each call plays a later part of the same list.
fn played() -> Vec<Played> {
    (0..400)
        .flat_map(|index| {
            let frame = 100 + index * 512;
            [
                Played::On {
                    frame,
                    pitch: 60,
                    velocity: 100,
                },
                Played::Off {
                    frame: frame + 200,
                    pitch: 60,
                },
            ]
        })
        .collect()
}

/// Something sounds in the last part of a render, so a test hears the state after its edits
/// and not a note that was already ringing.
fn plays(harness: &mut Harness) -> bool {
    let render = harness.play(8192);
    let left = render.left();
    left[left.len() / 2..].iter().any(|sample| *sample != 0.0)
}

/// An edit group that another record refuses is rolled back whole. The host must not have let
/// go of the plugin that is playing, nor kept the one it loaded for the edit that never was.
#[test]
fn an_edit_group_that_the_project_rejects_leaves_the_plugin_playing() {
    for format in FORMATS {
        a_rejected_group_leaves_it_playing(format);
    }
}

fn a_rejected_group_leaves_it_playing(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), played());
    assert!(plays(&mut harness), "{format:?}");

    // One group: the plugin record changes, and a record that refuses goes in with it.
    let mut changes = Changes::new();
    changes.create(id("track/instrument"), record(format, "other"));
    changes.create(id("picky"), Picky { refuses: true });
    let error = harness
        .project
        .commit("Change the plugin", changes)
        .expect_err("the group is refused");
    assert!(error.to_string().contains("refuses"), "{error}");

    // The record is the one from before, and so is the sound.
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");
    harness.plugins.poll(&harness.project);
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");

    // And the next edit that the project does take still reaches the engine.
    let mut changes = Changes::new();
    changes.create(id("track/instrument"), record(format, "other"));
    harness
        .project
        .commit("Change the plugin", changes)
        .expect("the group applies");
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");
}

/// Undo brings the record back before the host has polled, so the host still holds the plugin
/// of the deleted record while the project has made a new node for it.
#[test]
fn a_delete_and_an_undo_before_a_poll_leave_the_plugin_playing() {
    for format in FORMATS {
        a_delete_and_an_undo_before_a_poll(format);
    }
}

fn a_delete_and_an_undo_before_a_poll(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), played());
    assert!(plays(&mut harness), "{format:?}");

    let mut changes = Changes::new();
    changes.delete(&id("track/instrument"));
    harness
        .project
        .commit("Delete the instrument", changes)
        .expect("the delete applies");
    // No poll in between: the host still has the plugin of the record that just went.
    assert!(harness.project.undo().expect("undo").is_some());

    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");
    harness.plugins.poll(&harness.project);
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");
}

/// The same record written again with the same plugin and the same asset. The project applies
/// nothing, the plugin keeps playing, and a write that does change something takes effect.
#[test]
fn writing_the_same_record_again_changes_nothing_and_a_real_change_still_applies() {
    for format in FORMATS {
        the_same_record_again(format);
    }
}

fn the_same_record_again(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), played());
    assert!(plays(&mut harness), "{format:?}");

    let same = format!(
        r#"{{"tool": "plugin", "state": {{"format": "{}", "plugin_id": "{}", "state_asset": "piano"}}}}"#,
        format.as_str(),
        plugin_id(format)
    );
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", &same),
        0
    );
    assert!(plays(&mut harness), "{format:?}");

    let other = same.replace("\"piano\"", "\"other\"");
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", &other),
        1
    );
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");
}

/// The same two sequences with an effect after the instrument, which is two plugins in one
/// chain. Both ends of it must still play, and the chain must be whole.
#[test]
fn the_same_sequences_with_an_effect_in_the_chain_leave_the_whole_chain_playing() {
    for format in FORMATS {
        a_rejected_group_with_an_effect(format);
        a_delete_and_an_undo_with_an_effect(format);
    }
}

fn a_rejected_group_with_an_effect(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), played());
    // An offset of its own, so that what comes out is the effect's and not the instrument's.
    harness.write_offset(format, "trim", 25);
    harness.add_effect(record(format, "trim"));
    let through = harness.play(4096).samples().to_vec();
    assert!(plays(&mut harness), "{format:?}");

    // One group: the effect record changes, and a record that refuses goes in with it.
    let mut changes = Changes::new();
    changes.create(id("track/effect"), record(format, "other"));
    changes.create(id("picky"), Picky { refuses: true });
    let error = harness
        .project
        .commit("Change the effect", changes)
        .expect_err("the group is refused");
    assert!(error.to_string().contains("refuses"), "{error}");

    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    harness.plugins.poll(&harness.project);
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    // The chain is the one it was: the effect still has the state it had.
    let again = harness.play(4096).samples().to_vec();
    assert_eq!(peak(&again), peak(&through), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");
}

fn a_delete_and_an_undo_with_an_effect(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), played());
    harness.write_offset(format, "trim", 25);
    harness.add_effect(record(format, "trim"));
    let through = harness.play(4096).samples().to_vec();

    let mut changes = Changes::new();
    changes.delete(&id("track/effect"));
    harness
        .project
        .commit("Delete the effect", changes)
        .expect("the delete applies");
    // No poll in between: the host still has the plugin of the record that just went.
    assert!(harness.project.undo().expect("undo").is_some());

    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    harness.plugins.poll(&harness.project);
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
    let again = harness.play(4096).samples().to_vec();
    assert_eq!(peak(&again), peak(&through), "{format:?}");
    assert!(plays(&mut harness), "{format:?}");
}

/// Undo and redo of the record itself, several times over, with polls in between.
#[test]
fn undo_and_redo_of_the_record_leave_the_plugin_playing() {
    for format in FORMATS {
        undo_and_redo_of_the_record(format);
    }
}

fn undo_and_redo_of_the_record(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), played());

    let mut changes = Changes::new();
    changes.create(id("track/instrument"), record(format, "other"));
    harness
        .project
        .commit("Change the asset", changes)
        .expect("the change applies");

    for _ in 0..3 {
        assert!(harness.project.undo().expect("undo").is_some());
        harness.plugins.poll(&harness.project);
        assert!(plays(&mut harness), "{format:?}");
        assert!(harness.project.redo().expect("redo").is_some());
        harness.plugins.poll(&harness.project);
        assert!(plays(&mut harness), "{format:?}");
    }
    assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
}
