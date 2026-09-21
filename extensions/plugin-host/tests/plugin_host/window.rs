//! The plugin's own window: which GUI calls the host makes, in what order, on which thread,
//! and that a window goes whenever its plugin does.
//!
//! No display is needed. The test plugin makes no real window: it answers the calls of the
//! CLAP GUI extension and writes each one down with the thread it arrived on, which is what a
//! real plugin would assert. Which thread those calls belong to is in `lifecycle.rs`, where a
//! real audio thread runs beside the main one.

use std::path::Path;

use sound_core::Changes;

use crate::support::{
    Harness, LoggedCall, id, lifecycle, record, tell_the_plugin,
    tell_the_plugin_to_close_its_window,
};

const SLOT: &str = "track/instrument";

/// The GUI calls the plugin wrote down, in order.
fn window_calls(log: &Path) -> Vec<String> {
    let calls: Vec<LoggedCall> = lifecycle(log);
    calls
        .into_iter()
        .filter(|call| call.call.starts_with("gui_") || call.call == "closed")
        .map(|call| call.call)
        .collect()
}

/// A project with the test plugin on a track, a log of every call it gets, and a few blocks
/// played so that it is really running.
fn open(log: &Path) -> Harness {
    tell_the_plugin(Some(log), None);
    let mut harness = Harness::new();
    harness.add_track(record("piano"), Vec::new());
    harness.play(512);
    harness
}

#[test]
fn opening_the_window_creates_it_once_shows_it_and_a_second_open_only_shows_it_again() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let harness = open(&log);
    let slot = id(SLOT);
    assert!(harness.plugins.has_window(&slot));
    assert!(!harness.plugins.window_is_open(&slot));

    harness.plugins.open_window(&slot, "Piano — Night").unwrap();
    assert!(harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_window_change());
    assert_eq!(
        window_calls(&log),
        [
            // `has_window` asked first, then CLAP's order for a floating window.
            "gui_is_api_supported",
            "gui_is_api_supported",
            "gui_create",
            "gui_suggest_title[Piano_—_Night]",
            "gui_show",
        ]
    );

    // Opening again brings the one window forward: no second create.
    harness.plugins.open_window(&slot, "Piano — Night").unwrap();
    assert!(harness.plugins.window_is_open(&slot));
    let calls = window_calls(&log);
    assert_eq!(calls.last().map(String::as_str), Some("gui_show"));
    assert_eq!(
        calls.iter().filter(|call| *call == "gui_create").count(),
        1,
        "{calls:?}"
    );
}

#[test]
fn the_composer_closes_the_window_and_the_plugin_goes_on_playing() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut harness = open(&log);
    let slot = id(SLOT);
    harness.plugins.open_window(&slot, "Piano").unwrap();
    harness.plugins.close_window(&slot);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some("gui_destroy")
    );

    // The plugin is still there and still plays: a window is not the plugin.
    harness.write_and_apply(
        "state/track/keys.json",
        r#"{"tool": "test.keys", "state": {"played": [{"kind": "on", "frame": 0, "pitch": 60, "velocity": 100}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let render = harness.play(1024);
    assert_eq!(render.first_sound(), Some(0));

    // And it can be opened again, which creates it anew.
    harness.plugins.open_window(&slot, "Piano").unwrap();
    assert!(harness.plugins.window_is_open(&slot));
    let calls = window_calls(&log);
    assert_eq!(
        calls.iter().filter(|call| *call == "gui_create").count(),
        2,
        "{calls:?}"
    );
}

#[test]
fn a_window_the_plugin_closes_itself_is_freed_at_the_next_poll() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_close_its_window();
    let harness = open(&log);
    let slot = id(SLOT);
    harness.plugins.open_window(&slot, "Piano").unwrap();
    // The plugin asked for a call on the main thread; until the host makes it nothing changed.
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.take_window_change();

    harness.plugins.poll(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_window_change());
    let calls = window_calls(&log);
    assert_eq!(
        &calls[calls.len() - 2..],
        ["closed", "gui_destroy"],
        "{calls:?}"
    );
}

#[test]
fn the_window_goes_when_the_record_names_another_plugin_state() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut harness = open(&log);
    let slot = id(SLOT);
    harness.plugins.open_window(&slot, "Piano").unwrap();
    harness.plugins.take_window_change();

    // Another state file is another plugin as far as the host is concerned: it loads again.
    let mut changes = Changes::new();
    changes.create(slot.clone(), record("organ"));
    harness.project.commit("Choose organ", changes).unwrap();
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_window_change());
    let calls = window_calls(&log);
    assert!(calls.contains(&"gui_destroy".to_string()), "{calls:?}");
    assert_eq!(harness.problems(), Vec::<String>::new());
}

#[test]
fn the_window_goes_when_the_record_is_deleted_and_when_the_project_closes() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut harness = open(&log);
    let slot = id(SLOT);
    harness.plugins.open_window(&slot, "Piano").unwrap();
    harness.plugins.take_window_change();

    // Deleted, from the window or from a file: the host lets go of it at the next poll.
    let mut changes = Changes::new();
    changes.delete(&slot);
    harness
        .project
        .commit("Delete instrument", changes)
        .unwrap();
    harness.plugins.poll(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some("gui_destroy")
    );

    // Undo brings the plugin back. Its window does not come with it: opening one is not an
    // edit, so there is nothing to undo.
    harness.project.undo().unwrap();
    harness.plugins.poll(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(harness.problems(), Vec::<String>::new());

    // The project closing frees every window that is still open.
    harness.plugins.open_window(&slot, "Piano").unwrap();
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.close(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    let calls = window_calls(&log);
    assert_eq!(
        calls.iter().filter(|call| *call == "gui_destroy").count(),
        2,
        "{calls:?}"
    );
}

#[test]
fn dropping_the_host_frees_a_window_that_is_still_open() {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let harness = open(&log);
    let slot = id(SLOT);
    harness.plugins.open_window(&slot, "Piano").unwrap();
    let before = window_calls(&log);
    assert!(!before.contains(&"gui_destroy".to_string()), "{before:?}");

    // What quitting does: the project goes, and with it the registry, the behaviour and the
    // host. Nothing is left to close a window, so the drop of the host does it.
    let Harness {
        project,
        engine,
        plugins,
        ..
    } = harness;
    drop((project, engine, plugins));
    let calls = window_calls(&log);
    assert_eq!(calls.last().map(String::as_str), Some("gui_destroy"));
}
