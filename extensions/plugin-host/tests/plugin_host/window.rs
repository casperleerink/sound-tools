//! The plugin's own window: which calls of the GUI extension the host makes, in what order,
//! on which thread, and that a window goes whenever its plugin does.
//!
//! No display is needed. GPUI has a platform for tests whose windows are not real ones, so a
//! window opens and closes here with nothing on screen and the plugin gets no view to draw in.
//! The test plugin draws nothing anyway: it answers the calls of the GUI extension and writes
//! each one down with the thread it arrived on, which is what a real plugin would assert. What
//! only a real window can show is checked by hand, see the pull request.

use std::path::Path;

use gpui::TestAppContext;
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

/// Opens the window of the one plugin of `harness`, through the application.
fn open_window(harness: &Harness, title: &str, cx: &mut TestAppContext) {
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), title, cx))
        .expect("the window opens");
}

fn close_window(harness: &Harness, cx: &mut TestAppContext) {
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
}

/// How many windows the application has. A window that is taken down leaves none behind.
fn windows(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.windows().len())
}

#[gpui::test]
fn opening_the_window_creates_it_once_shows_it_and_a_second_open_only_brings_it_forward(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let harness = open(&log);
    let slot = id(SLOT);
    assert_eq!(harness.plugins.window_offered(&slot), Some(true));
    assert!(!harness.plugins.window_is_open(&slot));

    open_window(&harness, "Piano — Night", cx);
    assert!(harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_window_change());
    assert_eq!(windows(cx), 1);
    assert_eq!(
        window_calls(&log),
        [
            // Once while the plugin loaded, so that drawing a card calls into no plugin, and
            // once as the negotiation right before `create`, which is CLAP's order for an
            // embedded window. There is no `gui_set_parent`: a window of the test platform has
            // no view of its own.
            "gui_is_api_supported",
            "gui_is_api_supported",
            "gui_create",
            "gui_show",
        ]
    );

    // Opening again brings the one window forward: no second create and no second window.
    open_window(&harness, "Piano — Night", cx);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(windows(cx), 1);
    let calls = window_calls(&log);
    assert_eq!(
        calls.iter().filter(|call| *call == "gui_create").count(),
        1,
        "{calls:?}"
    );
}

#[gpui::test]
fn the_composer_closes_the_window_and_the_plugin_goes_on_playing(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut harness = open(&log);
    let slot = id(SLOT);
    open_window(&harness, "Piano", cx);
    close_window(&harness, cx);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(windows(cx), 0);
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
    open_window(&harness, "Piano", cx);
    assert!(harness.plugins.window_is_open(&slot));
    let calls = window_calls(&log);
    assert_eq!(
        calls.iter().filter(|call| *call == "gui_create").count(),
        2,
        "{calls:?}"
    );
}

#[gpui::test]
fn a_window_the_plugin_closes_itself_is_freed_at_the_next_poll(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_close_its_window();
    let harness = open(&log);
    let slot = id(SLOT);
    open_window(&harness, "Piano", cx);
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
    // The window itself is taken down by whoever polls, which has the application.
    assert_eq!(windows(cx), 1);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
}

#[gpui::test]
fn the_window_goes_when_the_record_names_another_plugin_state(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut harness = open(&log);
    let slot = id(SLOT);
    open_window(&harness, "Piano", cx);
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
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
}

#[gpui::test]
fn the_window_goes_when_the_record_is_deleted_and_when_the_project_closes(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut harness = open(&log);
    let slot = id(SLOT);
    open_window(&harness, "Piano", cx);
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
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);

    // Undo brings the plugin back. Its window does not come with it: opening one is not an
    // edit, so there is nothing to undo.
    harness.project.undo().unwrap();
    harness.plugins.poll(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(harness.problems(), Vec::<String>::new());

    // The project closing frees every window that is still open.
    open_window(&harness, "Piano", cx);
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.close(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    let calls = window_calls(&log);
    assert_eq!(
        calls.iter().filter(|call| *call == "gui_destroy").count(),
        2,
        "{calls:?}"
    );
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
}

#[gpui::test]
fn dropping_the_host_frees_the_view_of_a_window_that_is_still_open(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let harness = open(&log);
    open_window(&harness, "Piano", cx);
    let before = window_calls(&log);
    assert!(!before.contains(&"gui_destroy".to_string()), "{before:?}");

    // What quitting does: the project goes, and with it the registry, the behaviour and the
    // host. Nothing is left to close a window, so the drop of the host frees what the plugin
    // holds for it. The window itself goes with the application.
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

/// CLAP puts every call of the GUI extension on the main thread, so none of them may arrive
/// on the thread that processes. The window is opened and closed while a real audio thread
/// plays the plugin.
///
/// The GUI calls are written down with plugin 0: they are about the plugin itself and not about
/// one of its audio processors, which are what the numbers count.
#[gpui::test]
fn the_calls_of_a_plugins_window_are_on_the_main_thread_and_never_on_the_one_that_processes(
    cx: &mut TestAppContext,
) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    tell_the_plugin(Some(&log), None);
    let mut harness = Harness::new();
    harness.add_track(record("piano"), Vec::new());
    harness.project.engine().play();

    // Blocks on a thread of their own, as the device does. The application belongs to this
    // thread, so the window is opened and closed between two stretches of playing and not
    // inside one: `TestAppContext` is not another thread's to touch.
    let play = |blocks: usize, harness: &mut Harness| {
        let engine = &mut harness.engine;
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let mut buffer = vec![0.0_f32; 512 * 2];
                for _ in 0..blocks {
                    engine.process_block(&mut buffer);
                }
            });
        });
        harness.project.engine().poll().expect("the engine polls");
        harness.plugins.poll(&harness.project);
    };
    play(2, &mut harness);
    open_window(&harness, "Piano", cx);
    play(2, &mut harness);
    close_window(&harness, cx);
    play(2, &mut harness);

    let calls = lifecycle(&log);
    let names: Vec<&str> = calls.iter().map(|call| call.call.as_str()).collect();
    let thread_of = |name: &str| {
        let found = calls.iter().find(|call| call.call == name);
        found
            .unwrap_or_else(|| panic!("no {name} in {names:?}"))
            .thread
            .clone()
    };
    let (audio_thread, main_thread) = (thread_of("process"), thread_of("activate"));
    assert_ne!(audio_thread, main_thread, "{names:?}");
    for call in [
        "gui_is_api_supported",
        "gui_create",
        "gui_show",
        "gui_destroy",
    ] {
        assert_eq!(thread_of(call), main_thread, "{call}: {names:?}");
    }
}
