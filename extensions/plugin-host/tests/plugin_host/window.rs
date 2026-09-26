//! The plugin's own window: which calls of the plugin's window interface the host makes, in
//! what order, on which thread, and that a window goes whenever its plugin does.
//!
//! No display is needed. GPUI has a platform for tests whose windows are not real ones, so a
//! window opens and closes here with nothing on screen and the plugin gets no view to draw in.
//! Neither test plugin draws anything anyway: each answers the calls of its format's window
//! interface and writes every one down with the thread it arrived on, which is what a real
//! plugin would assert. What only a real window can show is checked by hand, see the pull
//! request.
//!
//! Every check that is about the host and not about one format runs for both. The two formats
//! do not make the same calls, so the one name each writes down for making a window and the one
//! for letting it go are `MADE_A_WINDOW` and `LET_GO_OF_ITS_WINDOW`, and the whole sequence of
//! each format is in the first test of this file.

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

/// The call each format writes down when it makes what it needs for a window: CLAP's
/// `gui_create`, and the VST 3 controller making an `IPlugView`.
const MADE_A_WINDOW: &str = "gui_create";

/// The call each format writes down when it has let go of everything it held for a window:
/// CLAP's `gui_destroy`, and the VST 3 view being released.
const LET_GO_OF_ITS_WINDOW: &str = "gui_destroy";

/// The window calls the plugin wrote down, in order.
fn window_calls(log: &Path) -> Vec<String> {
    let calls: Vec<LoggedCall> = lifecycle(log);
    calls
        .into_iter()
        .filter(|call| call.call.starts_with("gui_") || call.call == "closed")
        .map(|call| call.call)
        .collect()
}

/// How often `call` is in the log.
fn times(log: &Path, call: &str) -> usize {
    window_calls(log)
        .iter()
        .filter(|line| *line == call)
        .count()
}

/// A project with the test plugin of `format` on a track, a log of every call it gets, and a
/// few blocks played so that it is really running.
fn open(format: PluginFormat, log: &Path) -> Harness {
    tell_the_plugin(Some(log), None);
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), Vec::new());
    harness.play(512);
    harness
}

/// A folder and a log path of its own for one run, so that two formats in one test never read
/// each other's lines.
fn log_folder() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temporary folder")
}

/// Opens the window of the one plugin of `harness`, through the application.
fn open_window(harness: &Harness, cx: &mut TestAppContext) {
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .expect("the window opens");
}

fn close_window(harness: &Harness, cx: &mut TestAppContext) {
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
}

/// How many windows the application has. A window that is taken down leaves none behind.
fn windows(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.windows().len())
}

/// How big the one window of the application is.
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

/// Where the one window of the application is.
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

/// Takes the window down the way anything but this host would: GPUI's own `remove_window`,
/// which is what the window's close control ends in.
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
    // What each format costs to find out whether the plugin has a window at all. CLAP asks,
    // which is one cheap call. VST 3 has no way of asking but to build the plugin's whole
    // interface, so it asks nothing and offers the window; see ARCHITECTURE.md.
    let asked = window_calls(&log);
    let expected: &[&str] = match format {
        PluginFormat::Clap => &["gui_is_api_supported"],
        PluginFormat::Vst3 => &[],
    };
    assert_eq!(asked, expected, "{format:?}");

    let made_before = times(&log, MADE_A_WINDOW);
    open_window(&harness, cx);
    assert!(harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_window_change());
    assert_eq!(windows(cx), 1);
    // There is no call that gives the plugin a parent: a window of the test platform has no
    // view of its own, so the host has nothing to give it.
    let opening: Vec<String> = window_calls(&log).split_off(asked.len());
    let expected: &[&str] = match format {
        // CLAP: the negotiation right before `create`, which is its order for an embedded
        // window, whether the composer may resize it, and then the plugin is shown.
        PluginFormat::Clap => &[
            "gui_is_api_supported",
            "gui_create",
            "gui_can_resize",
            "gui_show",
        ],
        // VST 3: the controller makes a view, the host checks that a Cocoa view of ours suits
        // it, the frame goes in before the view can have a parent, and the host asks whether
        // the composer may resize it. There is no separate show in this format.
        PluginFormat::Vst3 => &[
            "gui_create",
            "gui_is_api_supported",
            "gui_set_frame",
            "gui_can_resize",
        ],
    };
    assert_eq!(opening, expected, "{format:?}");
    // The window is as big as the plugin said.
    assert_eq!(
        window_size(cx),
        (
            test_plugin_support::WINDOW_WIDTH,
            test_plugin_support::WINDOW_HEIGHT
        )
    );

    // Opening again brings the one window forward: nothing is made again and there is no
    // second window.
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

    // The plugin is still there and still plays: a window is not the plugin.
    harness.write_and_apply(
        "state/track/keys.json",
        r#"{"tool": "test.keys", "state": {"played": [{"kind": "on", "frame": 0, "pitch": 60, "velocity": 100}]}}"#,
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    let render = harness.play(1024);
    assert_eq!(render.first_sound(), Some(0));

    // And it can be opened again, which makes it anew.
    open_window(&harness, cx);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(times(&log, MADE_A_WINDOW) - made_before, 1, "{format:?}");
    close_window(&harness, cx);
    assert_eq!(windows(cx), 0);
}

/// A plugin that sizes itself as it opens, which is what a real one does when its interface is
/// bigger than the size it first reported. Both formats have a call for it, and both give the
/// size to the next poll: [`plugin_host::Plugins::settle_windows`] is what has the application.
///
/// VST 3 asks more of its host than CLAP does: the format says the host has to answer
/// `IPlugView::onSize` in the same callstack as the request, so that the plugin resizes the
/// view it made. The log says that it did.
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
    // Where the window is born differs, because the two formats ask at different moments. A
    // CLAP plugin asks from `show`, which is after the window was made, so the window is still
    // the size the plugin first reported. A VST 3 plugin asks from `setFrame`, which is before
    // the host reads its size, so the window is born the size it ended up asking for. Nothing
    // of GPUI may run while the table of plugins is borrowed either way.
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
        // The format's own rule: the view is told its new size inside the request.
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

/// A plugin with no window at all. Asking for one is a reported problem and not a window that
/// never fills, and the card stops offering one for the rest of the session.
///
/// When the card knows differs by format, and that is the one place where the two do. CLAP is
/// asked while the plugin loads, so the card never offers a window it cannot open. VST 3 has no
/// way of being asked but to build the plugin's whole interface, so the card offers one and the
/// answer arrives the first time the composer asks for it. See ARCHITECTURE.md.
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
    // Whatever it said before, the card offers nothing now.
    assert_eq!(harness.plugins.window_offered(&slot), Some(false));
    tell_the_plugin_to_have_no_window(false);
}

/// Loading a VST 3 plugin asks it nothing about a window, in any mode. Building a plugin's
/// interface to find out whether it has one costs up to a second of the thread that draws, and
/// `--render`, `--inspect` and `--headless` can open no window at all. See ARCHITECTURE.md.
#[test]
fn loading_a_vst3_plugin_makes_no_view_call_at_all() {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin(Some(&log), None);
    let mut harness = Harness::new();
    harness.add_track(record(PluginFormat::Vst3, "piano"), Vec::new());
    harness.play(1024);
    // The plugin is loaded, activated and playing, and its state has been read and saved.
    harness.plugins.close(&harness.project);
    let calls = window_calls(&log);
    assert_eq!(calls, Vec::<String>::new(), "{calls:?}");
}

/// A plugin that asks to be resized from inside the host's answer to another request of its
/// own. `editorhost.cpp` refuses the nested one; without that a plugin that answers `onSize`
/// with the same request runs the host out of stack. Whatever it asks for, the window ends on
/// the size the view really is.
#[gpui::test]
fn a_resize_asked_for_from_inside_the_answer_is_refused_and_the_newest_size_wins(
    cx: &mut TestAppContext,
) {
    // The same size as the outer request, which is the one that never ends without a guard.
    nested_resize_ends_on(cx, (640, 480), (640, 480));
    // And another size, which the view really takes, so the window has to follow it.
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
    // Two requests, one answer: the nested one was refused instead of being let in again.
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

/// CLAP only: a plugin may close the window it was given, and says so with
/// `clap_host_gui.closed`. VST 3 has no such call, because the host owns the window there and
/// the plugin only fills it.
#[gpui::test]
fn a_window_the_plugin_closes_itself_is_freed_at_the_next_poll(cx: &mut TestAppContext) {
    let folder = log_folder();
    let log = folder.path().join("calls.txt");
    tell_the_plugin_to_close_its_window();
    let harness = open(PluginFormat::Clap, &log);
    let slot = id(SLOT);
    open_window(&harness, cx);
    // The plugin asked for a call on the main thread; until the host makes it nothing changed.
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.take_window_change();

    harness.plugins.poll(&harness.project);
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_window_change());
    let calls = window_calls(&log);
    assert_eq!(
        &calls[calls.len() - 2..],
        ["closed", LET_GO_OF_ITS_WINDOW],
        "{calls:?}"
    );
    // The window itself is taken down by whoever polls, which has the application.
    assert_eq!(windows(cx), 1);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
}

/// The same plugin loading again in the same record brings its window back, where it was: a
/// record that names another state file, and a VST 3 plugin that asks to be loaded again
/// (`kReloadComponent`), which goes the same way, see `restarts.rs`. Another plugin in the
/// record does not get the window of the one it replaced.
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
    harness.plugins.take_window_change();

    // Another state file is another plugin load as far as the host is concerned: the window
    // of the one that goes goes with it.
    let mut changes = Changes::new();
    changes.create(slot.clone(), record(format, "organ"));
    harness.project.commit("Choose organ", changes).unwrap();
    assert!(!harness.plugins.window_is_open(&slot));
    assert!(harness.plugins.take_window_change());
    let calls = window_calls(&log);
    assert!(
        calls.contains(&LET_GO_OF_ITS_WINDOW.to_string()),
        "{calls:?}"
    );
    assert_eq!(harness.problems(), Vec::<String>::new());
    // It is the same plugin, so the window comes back where it was.
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 1);
    assert!(harness.plugins.window_is_open(&slot));
    assert_eq!(window_origin(cx), before);

    // Another plugin in the record: its window stays closed.
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
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
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
    open_window(&harness, cx);
    assert!(harness.plugins.window_is_open(&slot));
    harness.plugins.close(&harness.project);
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
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );
    assert_eq!(times(&log, LET_GO_OF_ITS_WINDOW) - before, 1, "{format:?}");
    // The window of a host that is gone is taken down here, so the next format starts clean.
    remove_the_window(cx);
}

/// Both formats put every call of a plugin's window on the main thread, so none of them may
/// arrive on the thread that processes. The window is opened and closed while a real audio
/// thread plays the plugin.
///
/// The window calls are written down with plugin 0: they are about the plugin itself and not
/// about one of its audio processors, which are what the numbers count.
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
    let mut harness = Harness::new();
    harness.add_track(record(format, "piano"), Vec::new());
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
    // Every window call the plugin wrote down, whatever the format calls it.
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

/// The plugin lets go of the view it is in before that view is released, on every path a
/// window can go by. GPUI tells the observers of a window that closes while it still holds the
/// window, so the plugin has let go before the window is one the application no longer has.
/// Without that the plugin would be left holding a freed `NSView`.
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

    // The way the window's own close control goes: GPUI removes the window, and this host is
    // told while the window is still there.
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

    // The way the card goes: this host frees the view and then takes the window down.
    open_window(&harness, cx);
    close_window(&harness, cx);
    assert_eq!(windows(cx), 0);
    assert_eq!(
        window_calls(&log).last().map(String::as_str),
        Some(LET_GO_OF_ITS_WINDOW),
        "{format:?}"
    );

    // The way quitting goes: every window of every plugin, before anything is torn down.
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
