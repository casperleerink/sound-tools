//! The plugin's own window: which window calls the host makes, in what order, on which thread,
//! and that a window goes whenever its plugin does. Each test plugin writes down every window
//! call with the thread it arrived on.

use std::path::Path;

use sound_core::Changes;

use plugin_host::{PluginFormat, WindowCommand};

use crate::desktop::Desktop;
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

fn last_call(log: &Path) -> Option<String> {
    window_calls(log).pop()
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

/// As the card's Close window sends it.
fn close_window(harness: &mut Harness, desktop: &mut Desktop) {
    let closed = harness.project.command(&id(SLOT), "close_window", None);
    assert_eq!(closed.ok(), Some(true));
    desktop.settle(&harness.plugins);
}

#[test]
fn opening_the_window_makes_it_once_and_a_second_open_only_brings_it_forward() {
    for format in FORMATS {
        a_window_is_made_once(format);
    }
}

fn a_window_is_made_once(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
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
    desktop.open(&harness.plugins, &slot);
    assert!(harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_card_change());
    let opening: Vec<String> = window_calls(&log).split_off(asked.len());
    let expected: &[&str] = match format {
        // CLAP's order for an embedded window asks the API right before `create`.
        PluginFormat::Clap => &[
            "gui_is_api_supported",
            "gui_create",
            "gui_can_resize",
            "gui_set_parent",
            "gui_show",
        ],
        // VST 3: the frame goes in before the view can have a parent, and there is no show.
        PluginFormat::Vst3 => &[
            "gui_create",
            "gui_is_api_supported",
            "gui_set_frame",
            "gui_can_resize",
            "gui_set_parent",
        ],
    };
    assert_eq!(opening, expected, "{format:?}");
    let shown = desktop.window().clone();
    assert_eq!(
        desktop.content(),
        (
            test_plugin_support::WINDOW_WIDTH,
            test_plugin_support::WINDOW_HEIGHT
        )
    );
    assert!(shown.focus);
    assert!(!shown.resizable);
    // The plugin's name, and the folder of the piece.
    let folder = harness.folder.path().file_name().unwrap().to_string_lossy();
    assert_eq!(
        shown.title,
        format!("{} — {folder}", test_vst3_plugin::PLUGIN_NAME),
        "{format:?}"
    );

    let window = desktop.only();
    desktop.open(&harness.plugins, &slot);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(desktop.shown.len(), 1);
    assert_eq!(
        desktop.commands.last(),
        Some(&WindowCommand::Focus { window })
    );
    assert_eq!(times(&log, MADE_A_WINDOW) - made_before, 1, "{format:?}");

    close_window(&mut harness, &mut desktop);
    assert!(desktop.shown.is_empty());
}

#[test]
fn the_composer_closes_the_window_and_the_plugin_goes_on_playing() {
    for format in FORMATS {
        closing_the_window_leaves_the_plugin(format);
    }
}

fn closing_the_window_leaves_the_plugin(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
    let slot = id(SLOT);
    desktop.open(&harness.plugins, &slot);
    let made_before = times(&log, MADE_A_WINDOW);
    close_window(&mut harness, &mut desktop);
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(desktop.shown.is_empty());
    assert_eq!(
        last_call(&log).as_deref(),
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

    desktop.open(&harness.plugins, &slot);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(times(&log, MADE_A_WINDOW) - made_before, 1, "{format:?}");
    close_window(&mut harness, &mut desktop);
    assert!(desktop.shown.is_empty());
}

/// VST 3 says the host answers `IPlugView::onSize` in the same callstack as the request.
#[test]
fn a_plugin_that_asks_for_another_size_gets_it_at_the_next_poll() {
    for format in FORMATS {
        a_plugin_sizes_its_own_window(format);
    }
}

fn a_plugin_sizes_its_own_window(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_ask_for_a_window_size(640, 480);
    let mut harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
    desktop.open(&harness.plugins, &id(SLOT));
    // A CLAP plugin asks from `show`, after the window was made, so the window is still the
    // first size. A VST 3 plugin asks from `setFrame`, before the host reads its size.
    if format == PluginFormat::Clap {
        assert_eq!(
            desktop.content(),
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
    desktop.settle(&harness.plugins);
    assert_eq!(
        desktop.content(),
        (640, 480),
        "{format:?} {:?}",
        window_calls(&log)
    );

    close_window(&mut harness, &mut desktop);
    assert!(desktop.shown.is_empty());
    tell_the_plugin_to_ask_for_a_window_size(0, 0);
}

/// CLAP is asked while the plugin loads, so the card never offers the window. VST 3 cannot be
/// asked without building the plugin's interface, so the card offers one until the first try.
#[test]
fn a_plugin_with_no_window_of_its_own_is_offered_none() {
    for format in FORMATS {
        no_window_is_offered(format);
    }
}

fn no_window_is_offered(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_have_no_window(true);
    let harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
    let slot = id(SLOT);
    let offered_before = match format {
        PluginFormat::Clap => Some(false),
        PluginFormat::Vst3 => Some(true),
    };
    assert_eq!(harness.plugins.window_offered(&slot), offered_before);

    let problem = harness
        .plugins
        .open_window(&slot)
        .expect_err("a plugin with no window opens none");
    assert!(
        problem.to_string().contains("no window of its own"),
        "{problem}"
    );
    desktop.settle(&harness.plugins);
    assert!(desktop.commands.is_empty(), "{:?}", desktop.commands);
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
#[test]
fn a_resize_asked_for_from_inside_the_answer_is_refused_and_the_newest_size_wins() {
    // The same size as the outer request never ends without a guard.
    nested_resize_ends_on((640, 480), (640, 480));
    // Another size, which the view really takes, so the window has to follow it.
    nested_resize_ends_on((640, 480), (800, 600));
}

fn nested_resize_ends_on(asked: (u32, u32), from_inside: (u32, u32)) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_ask_for_a_window_size(asked.0, asked.1);
    tell_the_plugin_to_ask_again_from_inside_the_answer(from_inside.0, from_inside.1);
    let mut harness = open(PluginFormat::Vst3, &log);
    let mut desktop = Desktop::new(1.0);
    desktop.open(&harness.plugins, &id(SLOT));
    harness.plugins.poll(&harness.project);
    desktop.settle(&harness.plugins);

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
    assert_eq!(desktop.content(), from_inside, "{calls:?}");

    close_window(&mut harness, &mut desktop);
    assert!(desktop.shown.is_empty());
    tell_the_plugin_to_ask_for_a_window_size(0, 0);
    tell_the_plugin_to_ask_again_from_inside_the_answer(0, 0);
}

/// CLAP only: VST 3 has no `clap_host_gui.closed`, because the host owns the window there.
#[test]
fn a_window_the_plugin_closes_itself_is_freed_at_the_next_poll() {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_close_its_window();
    let harness = open(PluginFormat::Clap, &log);
    let mut desktop = Desktop::new(1.0);
    let slot = id(SLOT);
    desktop.open(&harness.plugins, &slot);
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
    // The window itself is taken down by the app.
    assert_eq!(desktop.shown.len(), 1);
    desktop.settle(&harness.plugins);
    assert!(desktop.shown.is_empty());
}

/// A VST 3 plugin that asks to be loaded again goes the same way, see `remembered.rs`.
#[test]
fn the_same_plugin_loading_again_gets_its_window_back_and_another_one_does_not() {
    for format in FORMATS {
        another_state_brings_the_window_back(format);
    }
}

fn another_state_brings_the_window_back(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
    let slot = id(SLOT);
    desktop.open(&harness.plugins, &slot);
    let before = desktop.window().position;
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
    desktop.settle(&harness.plugins);
    assert_eq!(desktop.shown.len(), 1);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(desktop.window().position, before);
    // It came back by itself, so it leaves the keyboard where it was.
    assert!(!desktop.window().focus);

    let other = match format {
        PluginFormat::Clap => PluginFormat::Vst3,
        PluginFormat::Vst3 => PluginFormat::Clap,
    };
    let mut changes = Changes::new();
    changes.create(slot.clone(), record(other, "organ"));
    harness.project.commit("Choose another", changes).unwrap();
    assert!(!harness.plugins.window_is_open(&slot));
    desktop.settle(&harness.plugins);
    assert!(desktop.shown.is_empty());
    assert!(!harness.plugins.window_is_open(&slot));
}

#[test]
fn the_window_goes_when_the_record_is_deleted_and_when_the_project_closes() {
    for format in FORMATS {
        a_deleted_record_takes_the_window(format);
    }
}

fn a_deleted_record_takes_the_window(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
    let slot = id(SLOT);
    desktop.open(&harness.plugins, &slot);
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
        last_call(&log).as_deref(),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    desktop.settle(&harness.plugins);
    assert!(desktop.shown.is_empty());

    // Opening a window is not an edit, so undo does not bring it back.
    harness.project.undo().unwrap();
    harness.plugins.poll(&harness.project);
    desktop.settle(&harness.plugins);
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(desktop.shown.is_empty());
    assert_eq!(harness.problems(), Vec::<String>::new());

    desktop.open(&harness.plugins, &slot);
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.close(&mut harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert_eq!(
        last_call(&log).as_deref(),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    desktop.settle(&harness.plugins);
    assert!(desktop.shown.is_empty());
}

#[test]
fn dropping_the_host_frees_the_view_of_a_window_that_is_still_open() {
    for format in FORMATS {
        dropping_the_host_frees_the_view(format);
    }
}

fn dropping_the_host_frees_the_view(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
    desktop.open(&harness.plugins, &id(SLOT));
    let before = times(&log, LET_GO_OF_ITS_WINDOW);

    // Nothing is left to close the window, so the drop of the host frees the view. The window
    // itself goes with the app.
    let Harness {
        project,
        engine,
        plugins,
        ..
    } = harness;
    drop((project, engine, plugins));
    assert_eq!(
        last_call(&log).as_deref(),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    assert_eq!(times(&log, LET_GO_OF_ITS_WINDOW) - before, 1, "{format:?}");
}

/// Both formats put every window call on the main thread.
#[test]
fn the_calls_of_a_plugins_window_are_on_the_main_thread_and_never_on_the_one_that_processes() {
    for format in FORMATS {
        window_calls_are_on_the_main_thread(format);
    }
}

fn window_calls_are_on_the_main_thread(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin(Some(&log), None);
    let mut harness = Harness::in_process();
    let mut desktop = Desktop::new(1.0);
    harness.add_track(record(format, "piano"), Vec::new());
    harness.project.engine().play();

    // Blocks on a thread of their own, as the device does. The window opens and closes between
    // two stretches of playing.
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
    desktop.open(&harness.plugins, &id(SLOT));
    play(2, &mut harness);
    close_window(&mut harness, &mut desktop);
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
#[test]
fn the_plugin_lets_go_of_its_view_before_the_window_it_is_in_goes() {
    for format in FORMATS {
        the_view_goes_before_its_parent(format);
    }
}

fn the_view_goes_before_its_parent(format: PluginFormat) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let harness = open(format, &log);
    let mut desktop = Desktop::new(1.0);
    let slot = id(SLOT);
    let let_go_before = times(&log, LET_GO_OF_ITS_WINDOW);
    let plugins = &harness.plugins;

    // The window's own close control: the app tells the host before the view goes.
    desktop.open(plugins, &slot);
    desktop.close_by_its_title_bar(plugins);
    assert!(!plugins.window_is_open(&slot));
    assert_eq!(
        last_call(&log).as_deref(),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    // Nothing is left for the app to close.
    desktop.settle(plugins);
    assert!(
        !matches!(desktop.commands.last(), Some(WindowCommand::Close { .. })),
        "{:?}",
        desktop.commands
    );

    // The card's button: the view is freed before the app hears of it.
    desktop.open(plugins, &slot);
    plugins.close_window(&slot);
    assert_eq!(
        last_call(&log).as_deref(),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    desktop.settle(plugins);
    assert!(desktop.shown.is_empty());

    // Quitting.
    desktop.open(plugins, &slot);
    desktop.quit(plugins);
    assert!(!plugins.window_is_open(&slot));
    assert!(desktop.shown.is_empty());
    assert_eq!(
        times(&log, LET_GO_OF_ITS_WINDOW) - let_go_before,
        3,
        "{format:?}"
    );
}

/// The app may make a window after its plugin went, or a plugin may refuse the window. Either
/// way nothing is left open.
#[test]
fn a_window_whose_plugin_went_while_it_was_made_is_closed_and_a_refused_one_is_said() {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    let mut harness = open(PluginFormat::Vst3, &log);
    let slot = id(SLOT);
    harness.plugins.open_window(&slot).unwrap();
    let mut changes = Changes::new();
    changes.delete(&slot);
    harness.project.commit("Delete", changes).unwrap();
    harness.plugins.poll(&harness.project);
    let mut desktop = Desktop::new(1.0);
    desktop.settle(&harness.plugins);
    assert!(desktop.shown.is_empty(), "{:?}", desktop.commands);
    harness.project.undo().unwrap();
    harness.plugins.poll(&harness.project);

    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::ATTACH_FAILS_VARIABLE, "1") };
    desktop.open(&harness.plugins, &slot);
    assert!(!harness.plugins.window_is_open(&slot));
    desktop.settle(&harness.plugins);
    assert!(desktop.shown.is_empty(), "{:?}", desktop.commands);
    let problems = harness.plugins.poll(&harness.project);
    assert!(
        problems
            .iter()
            .any(|problem| problem.to_string().contains("did not open")),
        "{problems:?}"
    );
    let calls = window_calls(&log);
    assert!(!calls.contains(&"gui_removed".to_string()), "{calls:?}");
}

/// Every plugin made with nih-plug refuses `show`, with its view already on screen.
#[test]
fn a_clap_plugin_that_refuses_show_keeps_its_window() {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    // SAFETY: as in `tell_the_plugin`.
    unsafe { std::env::set_var(test_plugin_support::REFUSES_SHOW_VARIABLE, "1") };
    let harness = open(PluginFormat::Clap, &log);
    let slot = id(SLOT);
    let mut desktop = Desktop::new(1.0);
    desktop.open(&harness.plugins, &slot);
    assert_eq!(last_call(&log).as_deref(), Some("gui_show"));
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(desktop.shown.len(), 1, "{:?}", desktop.commands);
    let problems = harness.plugins.poll(&harness.project);
    assert!(problems.is_empty(), "{problems:?}");
}
