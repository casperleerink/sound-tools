//! What this host holds and what the engine plays must not drift apart.
//!
//! The project applies an edit group whole or not at all, and a group it rejects never reaches
//! the engine. So every sequence here ends the same way: a record that names a plugin this
//! machine has plays, and nothing is reported.

use sound_core::Changes;

use crate::support::{Harness, Picky, Played, id, record};

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
    let mut harness = Harness::new();
    harness.add_track(record("piano"), played());
    assert!(plays(&mut harness));

    // One group: the plugin record changes, and a record that refuses goes in with it.
    let mut changes = Changes::new();
    changes.create(id("track/instrument"), record("other"));
    changes.create(id("picky"), Picky { refuses: true });
    let error = harness
        .project
        .commit("Change the plugin", changes)
        .expect_err("the group is refused");
    assert!(error.to_string().contains("refuses"), "{error}");

    // The record is the one from before, and so is the sound.
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert!(plays(&mut harness));
    harness.plugins.poll(&harness.project);
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert!(plays(&mut harness));

    // And the next edit that the project does take still reaches the engine.
    let mut changes = Changes::new();
    changes.create(id("track/instrument"), record("other"));
    harness
        .project
        .commit("Change the plugin", changes)
        .expect("the group applies");
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert!(plays(&mut harness));
}

/// Undo brings the record back before the host has polled, so the host still holds the plugin
/// of the deleted record while the project has made a new node for it.
#[test]
fn a_delete_and_an_undo_before_a_poll_leave_the_plugin_playing() {
    let mut harness = Harness::new();
    harness.add_track(record("piano"), played());
    assert!(plays(&mut harness));

    let mut changes = Changes::new();
    changes.delete(&id("track/instrument"));
    harness
        .project
        .commit("Delete the instrument", changes)
        .expect("the delete applies");
    // No poll in between: the host still has the plugin of the record that just went.
    assert!(harness.project.undo().expect("undo").is_some());

    assert_eq!(harness.problems(), Vec::<String>::new());
    assert!(plays(&mut harness));
    harness.plugins.poll(&harness.project);
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert!(plays(&mut harness));
}

/// The same record written again with the same plugin and the same asset. The project applies
/// nothing, the plugin keeps playing, and a write that does change something takes effect.
#[test]
fn writing_the_same_record_again_changes_nothing_and_a_real_change_still_applies() {
    let mut harness = Harness::new();
    harness.add_track(record("piano"), played());
    assert!(plays(&mut harness));

    let same = format!(
        r#"{{"tool": "plugin", "state": {{"format": "clap", "plugin_id": "{}", "state_asset": "piano"}}}}"#,
        test_clap_plugin::PLUGIN_ID
    );
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", &same),
        0
    );
    assert!(plays(&mut harness));

    let other = same.replace("\"piano\"", "\"other\"");
    assert_eq!(
        harness.write_and_apply("state/track/instrument.json", &other),
        1
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert!(plays(&mut harness));
}

/// Undo and redo of the record itself, several times over, with polls in between.
#[test]
fn undo_and_redo_of_the_record_leave_the_plugin_playing() {
    let mut harness = Harness::new();
    harness.add_track(record("piano"), played());

    let mut changes = Changes::new();
    changes.create(id("track/instrument"), record("other"));
    harness
        .project
        .commit("Change the asset", changes)
        .expect("the change applies");

    for _ in 0..3 {
        assert!(harness.project.undo().expect("undo").is_some());
        harness.plugins.poll(&harness.project);
        assert!(plays(&mut harness));
        assert!(harness.project.redo().expect("redo").is_some());
        harness.plugins.poll(&harness.project);
        assert!(plays(&mut harness));
    }
    assert_eq!(harness.problems(), Vec::<String>::new());
}
