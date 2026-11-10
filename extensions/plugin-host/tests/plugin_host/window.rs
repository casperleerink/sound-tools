//! The plugin's own window: which window calls the host makes, in what order, on which thread,
//! and that a window goes whenever its plugin does.
//!
//! GPUI's test platform has no real windows, so the plugin gets no view to draw in. Each test
//! plugin writes down every window call with the thread it arrived on.

use std::path::Path;

use gpui::TestAppContext;
use sound_core::Changes;

use plugin_host::PluginFormat;

use crate::support::{
    FORMATS, Harness, LoggedCall, id, lifecycle, record, tell_the_plugin,
    tell_the_plugin_to_ask_again_from_inside_the_answer, tell_the_plugin_to_ask_for_a_window_size,
    tell_the_plugin_to_close_its_window, tell_the_plugin_to_have_no_window,
};

const SLOT: &str = "track/instrument";

/// What each format writes down for CLAP's `gui_create` and the VST 3 controller making an
/// `IPlugView`.
const MADE_A_WINDOW: &str = "gui_create";

/// What each format writes down for CLAP's `gui_destroy` and the VST 3 view being released.
const LET_GO_OF_ITS_WINDOW: &str = "gui_destroy";

fn window_calls(log: &Path) -> Vec<String> {
    let calls: Vec<LoggedCall> = lifecycle(log);
    calls
        .into_iter()
        .filter(|call| call.call.starts_with("gui_") || call.call == "closed")
        .map(|call| call.call)
        .collect()
}

fn times(log: &Path, call: &str) -> usize {
    window_calls(log)
        .iter()
        .filter(|line| *line == call)
        .count()
}

fn open(format: PluginFormat, log: &Path) -> Harness {
    tell_the_plugin(Some(log), None);
    let mut harness = Harness::in_process();
    harness.add_track(record(format, "piano"), Vec::new());
    harness.play(512);
    harness
}

/// One per format, so two formats in one test never read each other's lines.
fn log_folder() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary folder")
}

fn open_window(harness: &Harness, cx: &mut TestAppContext) {
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .expect("the window opens");
}

fn close_window(harness: &Harness, cx: &mut TestAppContext) {
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
}

fn windows(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.windows().len())
}

fn window_size(cx: &mut TestAppContext) -> (u32, u32) {
    let handle = cx.update(|cx| cx.windows().first().copied()).unwrap();
    let bounds = cx
        .update(|cx| handle.update(cx, |_, window, _| window.bounds()))
        .expect("the window is there");
    (
        f32::from(bounds.size.width) as u32,
        f32::from(bounds.size.height) as u32,
    )
}

fn window_origin(cx: &mut TestAppContext) -> (i32, i32) {
    let handle = cx.update(|cx| cx.windows().first().copied()).unwrap();
    let bounds = cx
        .update(|cx| handle.update(cx, |_, window, _| window.bounds()))
        .expect("the window is there");
    (
        f32::from(bounds.origin.x) as i32,
        f32::from(bounds.origin.y) as i32,
    )
}

/// What the window's own close control ends in.
fn remove_the_window(cx: &mut TestAppContext) {
    let handle = cx.update(|cx| cx.windows().first().copied()).unwrap();
    cx.update(|cx| {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .ok()
    });
}

#[gpui::test]
fn opening_the_window_makes_it_once_and_a_second_open_only_brings_it_forward(
    cx: &mut TestAppContext,
) {
    for format in FORMATS {
        a_window_is_made_once(format, cx);
    }
}

fn a_window_is_made_once(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let harness = open(format, &log);
    let slot = id(SLOT);
    assert_eq!(harness.plugins.window_offered(&slot), Some(true));
    assert!(!harness.plugins.window_is_open(&slot));
    // VST 3 cannot ask whether there is a window without building the plugin's whole
    // interface, so it asks nothing and offers one.
    let asked = window_calls(&log);
    let expected: &[&str] = match format {
        PluginFormat::Clap => &["gui_is_api_supported"],
        PluginFormat::Vst3 => &[],
    };
    assert_eq!(asked, expected, "{format:?}");

    let made_before = times(&log, MADE_A_WINDOW);
    open_window(&harness, cx);
    assert!(harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_card_change());
    assert_eq!(windows(cx), 1);
    // No call gives the plugin a parent: a test platform window has no view of its own.
    let opening: Vec<String> = window_calls(&log).split_off(asked.len());
    let expected: &[&str] = match format {
        // CLAP's order for an embedded window asks the API right before `create`.
        PluginFormat::Clap => &[
            "gui_is_api_supported",
            "gui_create",
            "gui_can_resize",
            "gui_show",
        ],
        // VST 3: the frame goes in before the view can have a parent, and there is no show.
        PluginFormat::Vst3 => &[
            "gui_create",
            "gui_is_api_supported",
            "gui_set_frame",
            "gui_can_resize",
        ],
    };
    assert_eq!(opening, expected, "{format:?}");
    assert_eq!(
        window_size(cx),
        (
            test_plugin_support::WINDOW_WIDTH,
            test_plugin_support::WINDOW_HEIGHT
        )
    );

    open_window(&harness, cx);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(windows(cx), 1);
    assert_eq!(times(&log, MADE_A_WINDOW) - made_before, 1, "{format:?}");

    close_window(&harness, cx);
    assert_eq!(windows(cx), 0);
}

#[gpui::test]
fn the_composer_closes_the_window_and_the_plugin_goes_on_playing(cx: &mut TestAppContext) {
    for format in FORMATS {
        closing_the_window_leaves_the_plugin(format, cx);
    }
}

fn closing_the_window_leaves_the_plugin(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(format, &log);
    let slot = id(SLOT);
    open_window(&harness, cx);
    let made_before = times(&log, MADE_A_WINDOW);
    close_window(&harness, cx);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(windows(cx), 0);
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );

    harness.write_and_apply(
        "state/track/keys.json",
        r#"{"tool": "test.keys", "state": {"played": [{"kind": "on", "frame": 0, "pitch": 60, "velocity": 100}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let render = harness.play(1024);
    assert_eq!(render.first_sound(), Some(0));

    open_window(&harness, cx);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(times(&log, MADE_A_WINDOW) - made_before, 1, "{format:?}");
    close_window(&harness, cx);
    assert_eq!(windows(cx), 0);
}

/// VST 3 says the host answers `IPlugView::onSize` in the same callstack as the request.
#[gpui::test]
fn a_plugin_that_asks_for_another_size_gets_it_at_the_next_poll(cx: &mut TestAppContext) {
    for format in FORMATS {
        a_plugin_sizes_its_own_window(format, cx);
    }
}

fn a_plugin_sizes_its_own_window(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_ask_for_a_window_size(640, 480);
    let harness = open(format, &log);
    open_window(&harness, cx);
    // A CLAP plugin asks from `show`, after the window was made, so the window is still the
    // first size. A VST 3 plugin asks from `setFrame`, before the host reads its size.
    if format == PluginFormat::Clap {
        assert_eq!(
            window_size(cx),
            (
                test_plugin_support::WINDOW_WIDTH,
                test_plugin_support::WINDOW_HEIGHT
            ),
            "{format:?}"
        );
    }
    let calls = window_calls(&log);
    assert!(
        calls.contains(&"gui_request_resize".to_string()),
        "{format:?} {calls:?}"
    );
    if format == PluginFormat::Vst3 {
        let asked = calls
            .iter()
            .position(|call| call == "gui_request_resize")
            .expect("the plugin asked");
        assert_eq!(
            calls.get(asked + 1).map(String::as_str),
            Some("gui_on_size")
        );
    }

    harness.plugins.poll(&harness.project);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(
        window_size(cx),
        (640, 480),
        "{format:?} {:?}",
        window_calls(&log)
    );

    close_window(&harness, cx);
    assert_eq!(windows(cx), 0);
    tell_the_plugin_to_ask_for_a_window_size(0, 0);
}

/// CLAP is asked while the plugin loads, so the card never offers the window. VST 3 cannot be
/// asked without building the plugin's interface, so the card offers one until the first try.
#[gpui::test]
fn a_plugin_with_no_window_of_its_own_is_offered_none(cx: &mut TestAppContext) {
    for format in FORMATS {
        no_window_is_offered(format, cx);
    }
}

fn no_window_is_offered(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_have_no_window(true);
    let harness = open(format, &log);
    let slot = id(SLOT);
    let offered_before = match format {
        PluginFormat::Clap => Some(false),
        PluginFormat::Vst3 => Some(true),
    };
    assert_eq!(harness.plugins.window_offered(&slot), offered_before);

    let problem = cx
        .update(|cx| harness.plugins.open_window(&slot, cx))
        .expect_err("a plugin with no window opens none");
    assert!(
        problem.to_string().contains("no window of its own"),
        "{problem}"
    );
    assert_eq!(windows(cx), 0);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(harness.plugins.window_offered(&slot), Some(false));
    tell_the_plugin_to_have_no_window(false);
}

/// Building a plugin's interface to find out whether it has a window costs up to a second of
/// the thread that draws, and `--render`, `--inspect` and `--headless` open no window at all.
#[test]
fn loading_a_vst3_plugin_makes_no_view_call_at_all() {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin(Some(&log), None);
    let mut harness = Harness::in_process();
    harness.add_track(record(PluginFormat::Vst3, "piano"), Vec::new());
    harness.play(1024);
    harness.plugins.close(&mut harness.project);
    let calls = window_calls(&log);
    assert_eq!(calls, Vec::<String>::new(), "{calls:?}");
}

/// `editorhost.cpp` refuses the nested request; without that a plugin that answers `onSize`
/// with the same request runs the host out of stack.
#[gpui::test]
fn a_resize_asked_for_from_inside_the_answer_is_refused_and_the_newest_size_wins(
    cx: &mut TestAppContext,
) {
    // The same size as the outer request never ends without a guard.
    nested_resize_ends_on(cx, (640, 480), (640, 480));
    // Another size, which the view really takes, so the window has to follow it.
    nested_resize_ends_on(cx, (640, 480), (800, 600));
}

fn nested_resize_ends_on(cx: &mut TestAppContext, asked: (u32, u32), from_inside: (u32, u32)) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_ask_for_a_window_size(asked.0, asked.1);
    tell_the_plugin_to_ask_again_from_inside_the_answer(from_inside.0, from_inside.1);
    let harness = open(PluginFormat::Vst3, &log);
    open_window(&harness, cx);
    harness.plugins.poll(&harness.project);
    cx.update(|cx| harness.plugins.settle_windows(cx));

    let calls = window_calls(&log);
    assert_eq!(
        calls.iter().filter(|call| *call == "gui_on_size").count(),
        1,
        "{calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| *call == "gui_request_resize")
            .count(),
        2,
        "{calls:?}"
    );
    assert_eq!(window_size(cx), from_inside, "{calls:?}");

    close_window(&harness, cx);
    assert_eq!(windows(cx), 0);
    tell_the_plugin_to_ask_for_a_window_size(0, 0);
    tell_the_plugin_to_ask_again_from_inside_the_answer(0, 0);
}

/// CLAP only: VST 3 has no `clap_host_gui.closed`, because the host owns the window there.
#[gpui::test]
fn a_window_the_plugin_closes_itself_is_freed_at_the_next_poll(cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_close_its_window();
    let harness = open(PluginFormat::Clap, &log);
    let slot = id(SLOT);
    open_window(&harness, cx);
    // Nothing changes until the next poll.
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.take_card_change();

    harness.plugins.poll(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_card_change());
    let calls = window_calls(&log);
    assert_eq!(
        &calls[calls.len() - 2..],
        ["closed", LET_GO_OF_ITS_WINDOW],
        "{calls:?}"
    );
    // The window itself is taken down by whoever has the application.
    assert_eq!(windows(cx), 1);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
}

/// A VST 3 plugin that asks to be loaded again goes the same way, see `remembered.rs`.
#[gpui::test]
fn the_same_plugin_loading_again_gets_its_window_back_and_another_one_does_not(
    cx: &mut TestAppContext,
) {
    for format in FORMATS {
        another_state_brings_the_window_back(format, cx);
    }
}

fn another_state_brings_the_window_back(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(format, &log);
    let slot = id(SLOT);
    open_window(&harness, cx);
    let before = window_origin(cx);
    harness.plugins.take_card_change();

    // Another state file is another plugin load.
    let mut changes = Changes::new();
    changes.create(slot.clone(), record(format, "organ"));
    harness.project.commit("Choose organ", changes).unwrap();
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_card_change());
    let calls = window_calls(&log);
    assert!(
        calls.contains(&LET_GO_OF_ITS_WINDOW.to_string()),
        "{calls:?}"
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 1);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(window_origin(cx), before);

    let other = match format {
        PluginFormat::Clap => PluginFormat::Vst3,
        PluginFormat::Vst3 => PluginFormat::Clap,
    };
    let mut changes = Changes::new();
    changes.create(slot.clone(), record(other, "organ"));
    harness.project.commit("Choose another", changes).unwrap();
    assert!(!harness.plugins.window_is_open(&slot));
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
    assert!(!harness.plugins.window_is_open(&slot));
}

#[gpui::test]
fn the_window_goes_when_the_record_is_deleted_and_when_the_project_closes(cx: &mut TestAppContext) {
    for format in FORMATS {
        a_deleted_record_takes_the_window(format, cx);
    }
}

fn a_deleted_record_takes_the_window(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(format, &log);
    let slot = id(SLOT);
    open_window(&harness, cx);
    harness.plugins.take_card_change();

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
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);

    // Opening a window is not an edit, so undo does not bring it back.
    harness.project.undo().unwrap();
    harness.plugins.poll(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(harness.problems(), Vec::<String>::new());

    open_window(&harness, cx);
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.close(&mut harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
}

#[gpui::test]
fn dropping_the_host_frees_the_view_of_a_window_that_is_still_open(cx: &mut TestAppContext) {
    for format in FORMATS {
        dropping_the_host_frees_the_view(format, cx);
    }
}

fn dropping_the_host_frees_the_view(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let harness = open(format, &log);
    open_window(&harness, cx);
    let before = times(&log, LET_GO_OF_ITS_WINDOW);

    // What quitting does: nothing is left to close the window, so the drop of the host frees
    // the view. The window itself goes with the application.
    let Harness {
        project,
        engine,
        plugins,
        ..
    } = harness;
    drop((project, engine, plugins));
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    assert_eq!(times(&log, LET_GO_OF_ITS_WINDOW) - before, 1, "{format:?}");
    // So the next format starts clean.
    remove_the_window(cx);
}

/// Both formats put every window call on the main thread.
#[gpui::test]
fn the_calls_of_a_plugins_window_are_on_the_main_thread_and_never_on_the_one_that_processes(
    cx: &mut TestAppContext,
) {
    for format in FORMATS {
        window_calls_are_on_the_main_thread(format, cx);
    }
}

fn window_calls_are_on_the_main_thread(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin(Some(&log), None);
    let mut harness = Harness::in_process();
    harness.add_track(record(format, "piano"), Vec::new());
    harness.project.engine().play();

    // Blocks on a thread of their own, as the device does. The window opens and closes between
    // two stretches of playing, as `TestAppContext` belongs to this thread.
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
    open_window(&harness, cx);
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
    let window_calls: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| name.starts_with("gui_"))
        .collect();
    assert!(window_calls.len() >= 4, "{names:?}");
    for call in window_calls {
        assert_eq!(thread_of(call), main_thread, "{call}: {names:?}");
    }
}

/// On every path a window can go by. Else the plugin would be left holding a freed `NSView`.
#[gpui::test]
fn the_plugin_lets_go_of_its_view_before_the_window_it_is_in_goes(cx: &mut TestAppContext) {
    for format in FORMATS {
        the_view_goes_before_its_parent(format, cx);
    }
}

fn the_view_goes_before_its_parent(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let harness = open(format, &log);
    let slot = id(SLOT);
    let let_go_before = times(&log, LET_GO_OF_ITS_WINDOW);

    // The window's own close control.
    open_window(&harness, cx);
    assert_eq!(windows(cx), 1);
    remove_the_window(cx);
    assert_eq!(windows(cx), 0);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );

    // The card's button.
    open_window(&harness, cx);
    close_window(&harness, cx);
    assert_eq!(windows(cx), 0);
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );

    // Quitting.
    open_window(&harness, cx);
    assert_eq!(windows(cx), 1);
    cx.update(|cx| harness.plugins.close_all_windows(cx));
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(windows(cx), 0);
    assert_eq!(
        times(&log, LET_GO_OF_ITS_WINDOW) - let_go_before,
        3,
        "{format:?}"
    );
}
