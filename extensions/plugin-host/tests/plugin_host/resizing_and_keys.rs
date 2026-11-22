//! A plugin's window that the composer resizes by dragging its edge, and keys typed into it.

use std::path::Path;

use plugin_host::{Key, KeyDirection, Keystroke, Modifiers, PluginFormat};
use test_plugin_support::{
    NO_ADJUST_VARIABLE, RESIZABLE_VARIABLE, SMALLEST_WINDOW, WINDOW_HEIGHT, WINDOW_WIDTH,
};

use crate::desktop::Desktop;
use crate::support::{
    FORMATS, Harness, id, lifecycle, record, tell_the_plugin,
    tell_the_plugin_to_ask_again_from_inside_the_answer,
};

const SLOT: &str = "track/instrument";

fn tell_the_plugin_it_may_be_resized(may: bool) {
    // SAFETY: as in `tell_the_plugin`.
    unsafe {
        match may {
            true => std::env::set_var(RESIZABLE_VARIABLE, "1"),
            false => std::env::remove_var(RESIZABLE_VARIABLE),
        }
    }
}

fn window_calls(log: &Path) -> Vec<String> {
    lifecycle(log)
        .into_iter()
        .map(|call| call.call)
        .filter(|call| call.starts_with("gui_"))
        .collect()
}

fn open(format: PluginFormat, log: &Path, desktop: &mut Desktop) -> Harness {
    tell_the_plugin(Some(log), None);
    let mut harness = Harness::in_process();
    harness.add_track(record(format, "piano"), Vec::new());
    harness.play(512);
    desktop.open(&harness.plugins, &id(SLOT));
    harness
}

/// The plugin adjusts the size (CLAP's `adjust_size`, VST 3's `checkSizeConstraint`) and takes
/// it (`set_size`, `onSize`). On a display of twice the pixels, so the plugin is told points.
#[test]
fn a_window_the_plugin_allows_to_resize_follows_a_drag_within_what_the_plugin_takes() {
    tell_the_plugin_it_may_be_resized(true);
    for format in FORMATS {
        resizing_within_limits(format);
    }
    tell_the_plugin_it_may_be_resized(false);
}

fn resizing_within_limits(format: PluginFormat) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut desktop = Desktop::new(2.0);
    let harness = open(format, &log, &mut desktop);
    assert!(desktop.window().resizable);
    assert_eq!(desktop.content(), (WINDOW_WIDTH, WINDOW_HEIGHT));
    let before = window_calls(&log).len();

    desktop.drag_edge(&harness.plugins, (640, 480));
    let calls = window_calls(&log).split_off(before);
    assert_eq!(calls, ["gui_adjust_size", "gui_on_size"], "{format:?}");
    desktop.settle(&harness.plugins);
    assert_eq!(desktop.content(), (640, 480), "{format:?}");

    desktop.drag_edge(&harness.plugins, (100, 100));
    desktop.settle(&harness.plugins);
    assert_eq!(desktop.content(), SMALLEST_WINDOW, "{format:?}");
    // The size the host gave is not told to the plugin again.
    let told = window_calls(&log);
    desktop.settle(&harness.plugins);
    assert_eq!(window_calls(&log), told);

    harness.plugins.close_window(&id(SLOT));
}

fn tell_the_plugin_not_to_adjust(not: bool) {
    // SAFETY: as in `tell_the_plugin`.
    unsafe {
        match not {
            true => std::env::set_var(NO_ADJUST_VARIABLE, "1"),
            false => std::env::remove_var(NO_ADJUST_VARIABLE),
        }
    }
}

/// CLAP's `adjust_size` says nothing and VST 3's `checkSizeConstraint` leaves the rectangle.
#[test]
fn a_plugin_that_does_not_adjust_a_size_takes_it_as_it_was_dragged() {
    tell_the_plugin_it_may_be_resized(true);
    tell_the_plugin_not_to_adjust(true);
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        let mut desktop = Desktop::new(1.0);
        let harness = open(format, &log, &mut desktop);
        let before = window_calls(&log).len();
        desktop.drag_edge(&harness.plugins, (100, 90));
        let calls = window_calls(&log).split_off(before);
        assert_eq!(calls, ["gui_adjust_size", "gui_on_size"], "{format:?}");
        desktop.settle(&harness.plugins);
        assert_eq!(desktop.content(), (100, 90), "{format:?}");
        harness.plugins.close_window(&id(SLOT));
    }
    tell_the_plugin_not_to_adjust(false);
    tell_the_plugin_it_may_be_resized(false);
}

/// `onSize` is never called inside `onSize`. The window ends on the size the view then says it
/// has.
#[test]
fn a_view_that_asks_for_a_size_inside_the_answer_to_a_drag_is_not_answered_inside_it() {
    tell_the_plugin_it_may_be_resized(true);
    tell_the_plugin_to_ask_again_from_inside_the_answer(500, 400);
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut desktop = Desktop::new(1.0);
    let harness = open(PluginFormat::Vst3, &log, &mut desktop);
    let before = window_calls(&log).len();
    desktop.drag_edge(&harness.plugins, (640, 480));
    let calls = window_calls(&log).split_off(before);
    assert_eq!(
        calls,
        ["gui_adjust_size", "gui_on_size", "gui_request_resize"],
        "{calls:?}"
    );
    desktop.settle(&harness.plugins);
    assert_eq!(desktop.content(), (500, 400));
    harness.plugins.close_window(&id(SLOT));
    tell_the_plugin_to_ask_again_from_inside_the_answer(0, 0);
    tell_the_plugin_it_may_be_resized(false);
}

#[test]
fn a_window_the_plugin_does_not_allow_to_resize_asks_it_nothing() {
    tell_the_plugin_it_may_be_resized(false);
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        let mut desktop = Desktop::new(1.0);
        let harness = open(format, &log, &mut desktop);
        assert!(!desktop.window().resizable);
        desktop.drag_edge(&harness.plugins, (640, 480));
        desktop.settle(&harness.plugins);
        let calls = window_calls(&log);
        assert!(!calls.contains(&"gui_adjust_size".to_string()), "{calls:?}");
        assert!(!calls.contains(&"gui_on_size".to_string()), "{calls:?}");
        harness.plugins.close_window(&id(SLOT));
    }
}

/// Both formats count physical pixels in a Win32 window, and the plugin is told the scale
/// before it is shown. The CLAP test plugin grows with it.
#[test]
fn a_plugin_in_a_win32_window_is_told_the_scale_first_and_counts_physical_pixels() {
    tell_the_plugin_it_may_be_resized(true);
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let mut desktop = Desktop::new(1.5).with_win32_views();
    let harness = open(PluginFormat::Clap, &log, &mut desktop);
    let calls = window_calls(&log);
    let scaled = calls.iter().position(|call| call == "gui_set_scale[1.5]");
    let parent = calls.iter().position(|call| call == "gui_set_parent");
    assert!(scaled.is_some() && scaled < parent, "{calls:?}");
    // 320 by 240 at one and a half.
    desktop.settle(&harness.plugins);
    assert_eq!(desktop.window().content, (480, 360), "{calls:?}");
    // Past the largest the plugin takes, 1200 by 900 of its pixels, which a plugin told points
    // would take as it is.
    desktop.drag_edge(&harness.plugins, (1500, 1200));
    desktop.settle(&harness.plugins);
    assert_eq!(desktop.window().content, (1200, 900));
    harness.plugins.close_window(&id(SLOT));
    tell_the_plugin_it_may_be_resized(false);
}

fn typed(text: &str, modifiers: Modifiers) -> Keystroke {
    Keystroke {
        key: Key::Text(text.to_string()),
        modifiers,
    }
}

/// A key the plugin's own view does not take reaches the window, and the host passes it on with
/// `IPlugView::onKeyDown` and `onKeyUp`. CLAP has no such call, so the app plays it.
#[test]
fn a_key_typed_in_a_plugins_window_reaches_a_vst3_view() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        let mut desktop = Desktop::new(1.0);
        let harness = open(format, &log, &mut desktop);
        let none = Modifiers::default();
        let keys = [
            (typed("a", none), KeyDirection::Down),
            (
                typed(
                    "B",
                    Modifiers {
                        shift: true,
                        ..none
                    },
                ),
                KeyDirection::Down,
            ),
            (
                Keystroke {
                    key: Key::Enter,
                    modifiers: none,
                },
                KeyDirection::Down,
            ),
            (
                typed(
                    "c",
                    Modifiers {
                        command: true,
                        ..none
                    },
                ),
                KeyDirection::Down,
            ),
            (typed("a", none), KeyDirection::Up),
        ];
        let used: Vec<bool> = keys
            .iter()
            .map(|(key, direction)| desktop.type_key(&harness.plugins, key, *direction))
            .collect();
        let logged: Vec<String> = window_calls(&log)
            .into_iter()
            .filter(|call| call.starts_with("gui_key"))
            .collect();
        let expected: &[&str] = match format {
            PluginFormat::Clap => &[],
            // The character, the virtual key code of a key that types none, and the modifiers
            // as `keycodes.h` has them: shift 1, command 4.
            PluginFormat::Vst3 => &[
                "gui_key_down[97,0,0]",
                "gui_key_down[66,0,1]",
                "gui_key_down[0,4,0]",
                "gui_key_down[99,0,4]",
                "gui_key_up[97,0,0]",
            ],
        };
        assert_eq!(logged, expected, "{format:?}");
        assert_eq!(used, [format == PluginFormat::Vst3; 5], "{format:?}");

        // A window that went hands on nothing.
        let window = desktop.only();
        harness.plugins.close_window(&id(SLOT));
        let late = harness
            .plugins
            .window_key(window, &typed("a", none), KeyDirection::Down);
        assert!(!late);
        assert_eq!(
            window_calls(&log)
                .iter()
                .filter(|call| call.starts_with("gui_key"))
                .count(),
            expected.len()
        );
    }
}
