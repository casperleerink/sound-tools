//! The plugin's own state: saved as an asset when the plugin says it changed, back on reopen,
//! and untouched by undo.

use std::time::Duration;

use plugin_host::PluginFormat;

use crate::support::{
    FORMATS, Harness, Played, id, peak, record, saved_controller_level, saved_transpose,
    state_asset, tell_the_plugin_that_its_controller_fails,
    tell_the_plugin_to_keep_a_controller_state, tell_the_plugin_to_write_its_header_last,
    vst3_state,
};

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
    for format in FORMATS {
        state_survives_close_and_reopen(format);
    }
}

fn state_survives_close_and_reopen(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), change_the_state_and_play());
    let before = harness.play(2048);
    assert!(before.first_sound().is_some());

    let asset = harness.project.assets().path(&state_asset("piano"));
    assert!(
        asset.exists(),
        "the plugin state was not written to {asset:?}"
    );
    let saved = std::fs::read(&asset).unwrap();
    assert_eq!(saved_transpose(format, &saved), 36, "{format:?}");

    // Without the pedal, the transpose can only come from the state the plugin loaded.
    let mut harness = harness.reopen();
    harness.write_and_apply(
        "state/track/keys.json",
        r#"{"tool": "test.keys", "state": {"played": [{"kind": "on", "frame": 128, "pitch": 60, "velocity": 100}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let after = harness.play(2048);

    assert_eq!(after.left(), before.left());
    assert!(after.right().iter().all(|sample| *sample == 0.0));
}

/// A plugin that changes its state without saying so must not lose it.
#[test]
fn a_plugin_that_says_nothing_is_written_when_the_project_closes_and_not_before() {
    for format in FORMATS {
        written_on_close_and_not_before(format);
    }
}

fn written_on_close_and_not_before(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(
        record(format, "piano"),
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

    assert_eq!(harness.plugins.close(&mut harness.project), []);
    let saved = std::fs::read(&asset).expect("the state of every plugin is saved on close");
    assert_eq!(saved_transpose(format, &saved), 0, "{format:?}");

    // A session that changes nothing writes nothing again, so git sees no diff.
    let written = std::fs::metadata(&asset).unwrap().modified().unwrap();
    let mut harness = harness.reopen();
    harness.play(1024);
    assert_eq!(harness.plugins.close(&mut harness.project), []);
    assert_eq!(
        std::fs::metadata(&asset).unwrap().modified().unwrap(),
        written
    );
}

#[test]
fn undo_and_redo_do_not_touch_plugin_state() {
    for format in FORMATS {
        undo_leaves_state_alone(format);
    }
}

fn undo_leaves_state_alone(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), change_the_state_and_play());
    harness.play(2048);
    let asset = harness.project.assets().path(&state_asset("piano"));
    let saved = std::fs::read(&asset).unwrap();

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
    for format in FORMATS {
        read_only_writes_no_state(format);
    }
}

fn read_only_writes_no_state(format: PluginFormat) {
    let folder = tempfile::tempdir().unwrap();
    let mut harness = Harness::open(folder, true);
    harness.add_track(record(format, "piano"), change_the_state_and_play());
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

/// The app quits without unwinding and GPUI drops the views before any quit handler, so
/// dropping the project is the one moment that always happens.
#[test]
fn dropping_the_project_saves_the_state_of_every_plugin() {
    for format in FORMATS {
        dropping_saves_every_plugin(format);
    }
}

fn dropping_saves_every_plugin(format: PluginFormat) {
    let folder = tempfile::tempdir().unwrap();
    let mut harness = Harness::open(folder, true);
    harness.add_track(record(format, "piano"), change_the_state_and_play());
    harness.play(2048);
    let asset = harness.project.assets().path(&state_asset("piano"));
    std::fs::remove_file(&asset).unwrap();

    // No `close`.
    let Harness {
        project,
        plugins,
        folder,
        ..
    } = harness;
    drop((project, plugins));
    assert!(asset.exists(), "dropping the project wrote no state");
    let saved = std::fs::read(&asset).unwrap();
    assert_eq!(saved_transpose(format, &saved), 36, "{format:?}");
    drop(folder);
}

/// So undo of a delete brings the plugin back as it sounded.
#[test]
fn a_plugin_whose_record_goes_is_saved_on_the_way_out() {
    for format in FORMATS {
        saved_on_the_way_out(format);
    }
}

fn saved_on_the_way_out(format: PluginFormat) {
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), change_the_state_and_play());
    harness.play(2048);
    let asset = harness.project.assets().path(&state_asset("piano"));
    std::fs::remove_file(&asset).unwrap();

    let mut changes = sound_core::Changes::new();
    changes.delete(&id("track/instrument"));
    harness
        .project
        .commit("Delete the plugin", changes)
        .unwrap();
    harness.plugins.poll(&harness.project);
    let saved = std::fs::read(&asset).expect("the plugin was saved when its record went");
    assert_eq!(saved_transpose(format, &saved), 36, "{format:?}");

    assert!(harness.project.undo().unwrap().is_some());
    assert_eq!(harness.problems(), Vec::<String>::new());
}

/// A plugin that is one object for both VST 3 halves may still keep something of its own in
/// each, so the host asks the controller interface whatever the object is.
#[test]
fn the_controller_state_of_a_one_object_plugin_is_saved_and_comes_back() {
    tell_the_plugin_to_keep_a_controller_state();
    let format = PluginFormat::Vst3;
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), change_the_state_and_play());
    let before = harness.play(2048);
    // The pedal halves the level of its controller state. A note at velocity 100 is a cosine
    // of that amplitude.
    let loud = 100.0 / 127.0;
    assert!(
        peak(&before.left()) < loud * 0.6,
        "{}",
        peak(&before.left())
    );

    let asset = harness.project.assets().path(&state_asset("piano"));
    let saved = std::fs::read(&asset).unwrap();
    assert_eq!(saved_transpose(format, &saved), 36);
    assert_eq!(saved_controller_level(&saved), Some(50), "{saved:?}");

    // Without the pedal, the level can only come from the controller part of the asset.
    let mut harness = harness.reopen();
    harness.write_and_apply(
        "state/track/keys.json",
        r#"{"tool": "test.keys", "state": {"played": [{"kind": "on", "frame": 128, "pitch": 60, "velocity": 100}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let after = harness.play(2048);
    assert_eq!(after.left(), before.left());
}

/// A plugin half that cannot give its state is not an empty state: the file is the composer's
/// sound.
#[test]
fn a_controller_that_cannot_give_its_state_leaves_the_file_that_is_there_alone() {
    tell_the_plugin_that_its_controller_fails();
    let format = PluginFormat::Vst3;
    let mut harness = Harness::new();
    let asset = harness.project.assets().path(&state_asset("piano"));
    std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
    let held = vst3_state(
        &test_plugin_support::save_state(test_plugin_support::SavedState {
            semitones: 7,
            ..Default::default()
        }),
        b"",
    );
    std::fs::write(&asset, &held).unwrap();

    harness.add_track(record(format, "piano"), change_the_state_and_play());
    // Polled once by hand, so the problem of that one save is the one read here.
    harness.render_without_polling(2048);
    let problems = harness.plugins.poll(&harness.project);
    let problems: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(
        problems.iter().any(|problem| problem.contains("getState")),
        "{problems:?}"
    );
    assert_eq!(
        std::fs::read(&asset).unwrap(),
        held,
        "the state was damaged"
    );

    assert_eq!(harness.plugins.close(&mut harness.project).len(), 1);
    assert_eq!(
        std::fs::read(&asset).unwrap(),
        held,
        "the state was damaged"
    );
}

/// The plugin leaves room for its header, writes the payload and seeks back. A host stream that
/// would not let it seek past what it had written turns that into a state that is not its own.
#[test]
fn a_plugin_that_writes_its_header_last_is_saved_as_it_meant_it() {
    tell_the_plugin_to_write_its_header_last();
    let format = PluginFormat::Vst3;
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), change_the_state_and_play());
    let before = harness.play(2048);

    let asset = harness.project.assets().path(&state_asset("piano"));
    let saved = std::fs::read(&asset).unwrap();
    assert_eq!(saved_transpose(format, &saved), 36);

    let mut harness = harness.reopen();
    harness.write_and_apply(
        "state/track/keys.json",
        r#"{"tool": "test.keys", "state": {"played": [{"kind": "on", "frame": 128, "pitch": 60, "velocity": 100}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let after = harness.play(2048);
    assert_eq!(after.left(), before.left());
}

/// A plugin that says its state changed on every step of a knob drag would have the host write
/// it sixty times a second, and a sampler's state is not small.
#[test]
fn a_plugin_that_keeps_changing_is_written_at_most_once_a_second() {
    for format in FORMATS {
        written_at_most_once_a_second(format);
    }
}

fn written_at_most_once_a_second(format: PluginFormat) {
    let mut harness = Harness::new();
    // Each pedal move changes the plugin's own state.
    let played = (0..16)
        .map(|index| Played::Pedal {
            frame: index * 512,
            value: 70 + index as u8,
        })
        .collect();
    harness.add_track(record(format, "piano"), played);
    let asset = harness.project.assets().path(&state_asset("piano"));

    let saved = |asset: &std::path::Path| {
        let bytes = std::fs::read(asset).unwrap_or_default();
        saved_transpose(format, &bytes)
    };

    let start = std::time::Instant::now();
    harness.render_without_polling(1024);
    harness.plugins.poll_at(&harness.project, start);
    assert_eq!(saved(&asset), 70 + 1 - 64);

    for step in 1..8 {
        harness.render_without_polling(1024);
        harness
            .plugins
            .poll_at(&harness.project, start + Duration::from_millis(100 * step));
    }
    assert_eq!(
        saved(&asset),
        70 + 1 - 64,
        "the state was written again too soon"
    );

    harness
        .plugins
        .poll_at(&harness.project, start + Duration::from_millis(1100));
    let after_a_second = saved(&asset);
    assert!(
        after_a_second > 70 + 1 - 64,
        "nothing was written after the second"
    );

    // Closing writes what still waits.
    harness.render_without_polling(1024);
    assert_eq!(harness.plugins.close(&mut harness.project), []);
    assert!(saved(&asset) >= after_a_second);
}
