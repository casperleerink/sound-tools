//! The plugin's own state: saved as an asset when the plugin says it changed, back on reopen,
//! and untouched by undo.

use crate::support::{Harness, Played, record, state_asset};

/// Pedal 100 makes the test plugin transpose by 36 semitones and mark its state dirty.
fn change_the_state_and_play() -> Vec<Played> {
    vec![
        Played::Pedal {
            frame: 0,
            value: 100,
        },
        Played::On {
            frame: 128,
            pitch: 60,
            velocity: 100,
        },
    ]
}

#[test]
fn changed_plugin_state_is_in_the_project_after_close_and_comes_back_on_reopen() {
    let mut harness = Harness::new();
    harness.add_track(record("piano"), change_the_state_and_play());
    let before = harness.play(2048);
    assert!(before.first_sound().is_some());

    let asset = harness.project.assets().path(&state_asset("piano"));
    assert!(
        asset.exists(),
        "the plugin state was not written to {asset:?}"
    );
    let saved = std::fs::read(&asset).unwrap();
    assert_eq!(&saved[..4], b"STT1");
    assert_eq!(
        i32::from_le_bytes([saved[4], saved[5], saved[6], saved[7]]),
        36
    );

    // Reopen with the pedal gone from what is played. The notes are still transposed, which
    // can only come from the state the plugin loaded.
    let mut harness = harness.reopen();
    harness.write_and_apply(
        "state/track/keys.json",
        r#"{"tool": "test.keys", "state": {"played": [{"kind": "on", "frame": 128, "pitch": 60, "velocity": 100}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let after = harness.play(2048);

    // Frame for frame the same sound, without the pedal in the right channel.
    assert_eq!(after.left(), before.left());
    assert!(after.right().iter().all(|sample| *sample == 0.0));
}

/// While it plays, only a plugin that says its state changed is written. Closing writes every
/// plugin, because a plugin that changes its state without saying so must not lose it.
#[test]
fn a_plugin_that_says_nothing_is_written_when_the_project_closes_and_not_before() {
    let mut harness = Harness::new();
    harness.add_track(
        record("piano"),
        vec![Played::On {
            frame: 0,
            pitch: 60,
            velocity: 100,
        }],
    );
    harness.play(1024);
    let asset = harness.project.assets().path(&state_asset("piano"));
    assert!(
        !asset.exists(),
        "{asset:?} was written while nothing changed"
    );

    assert_eq!(harness.plugins.close(&harness.project), []);
    let saved = std::fs::read(&asset).expect("the state of every plugin is saved on close");
    assert_eq!(&saved[..4], b"STT1");

    // A session that changes nothing writes nothing again, so a project in git gets no diff.
    let written = std::fs::metadata(&asset).unwrap().modified().unwrap();
    let mut harness = harness.reopen();
    harness.play(1024);
    assert_eq!(harness.plugins.close(&harness.project), []);
    assert_eq!(
        std::fs::metadata(&asset).unwrap().modified().unwrap(),
        written
    );
}

/// Plugin state is opaque and is not project state. An undo of anything else leaves it alone.
#[test]
fn undo_and_redo_do_not_touch_plugin_state() {
    let mut harness = Harness::new();
    harness.add_track(record("piano"), change_the_state_and_play());
    harness.play(2048);
    let asset = harness.project.assets().path(&state_asset("piano"));
    let saved = std::fs::read(&asset).unwrap();

    // The undo step of "Add track" takes the record away and brings it back.
    assert_eq!(
        harness.project.undo().unwrap().as_deref(),
        Some("Add track")
    );
    assert_eq!(std::fs::read(&asset).unwrap(), saved);
    assert_eq!(
        harness.project.redo().unwrap().as_deref(),
        Some("Add track")
    );
    assert_eq!(std::fs::read(&asset).unwrap(), saved);
    assert!(harness.project.undo_label().is_some());
}

#[test]
fn a_project_open_read_only_loads_plugins_and_writes_no_state() {
    let folder = tempfile::tempdir().unwrap();
    let mut harness = Harness::open(folder, true);
    harness.add_track(record("piano"), change_the_state_and_play());
    let before = harness.play(2048);
    let asset = harness.project.assets().path(&state_asset("piano"));
    std::fs::remove_file(&asset).unwrap();

    let Harness {
        project, folder, ..
    } = harness;
    drop(project);
    let mut harness = Harness::open(folder, false);
    let after = harness.play(2048);
    assert_eq!(after.left(), before.left());
    assert!(!asset.exists(), "a read-only project wrote {asset:?}");
}

/// The window is quit without unwinding and GPUI drops the views before any quit handler, so
/// dropping the project is the one moment that always happens. It must still save.
#[test]
fn dropping_the_project_saves_the_state_of_every_plugin() {
    let folder = tempfile::tempdir().unwrap();
    let mut harness = Harness::open(folder, true);
    harness.add_track(record("piano"), change_the_state_and_play());
    harness.play(2048);
    let asset = harness.project.assets().path(&state_asset("piano"));
    std::fs::remove_file(&asset).unwrap();

    // No `close`: only the project and the host go.
    let Harness {
        project,
        plugins,
        folder,
        ..
    } = harness;
    drop((project, plugins));
    assert!(asset.exists(), "dropping the project wrote no state");
    let saved = std::fs::read(&asset).unwrap();
    assert_eq!(
        i32::from_le_bytes([saved[4], saved[5], saved[6], saved[7]]),
        36
    );
    drop(folder);
}
