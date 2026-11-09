//! A plugin's window that the composer resizes by dragging its edge, and keys typed into it.
//! GPUI's test platform resizes a window as a drag of its edge would.

use std::path::Path;

use gpui::{AnyWindowHandle, KeyUpEvent, Keystroke, PlatformInput, TestAppContext, px, size};
use plugin_host::PluginFormat;
use test_plugin_support::{
    NO_ADJUST_VARIABLE, RESIZABLE_VARIABLE, SMALLEST_WINDOW, WINDOW_HEIGHT, WINDOW_WIDTH,
};

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

fn open(format: PluginFormat, log: &Path, cx: &mut TestAppContext) -> (Harness, AnyWindowHandle) {
    tell_the_plugin(Some(log), None);
    let mut harness = Harness::in_process();
    harness.add_track(record(format, "piano"), Vec::new());
    harness.play(512);
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .expect("the window opens");
    let handle = cx.update(|cx| cx.windows().first().copied()).unwrap();
    (harness, handle)
}

/// A test platform window has no title bar, so its bounds are its content.
fn content_size(handle: AnyWindowHandle, cx: &mut TestAppContext) -> (u32, u32) {
    let content = cx
        .update(|cx| handle.update(cx, |_, window, _| window.bounds().size))
        .expect("the window is there");
    (
        f32::from(content.width) as u32,
        f32::from(content.height) as u32,
    )
}

/// The plugin adjusts the size (CLAP's `adjust_size`, VST 3's `checkSizeConstraint`) and takes
/// it (`set_size`, `onSize`).
#[gpui::test]
fn a_window_the_plugin_allows_to_resize_follows_a_drag_within_what_the_plugin_takes(
    cx: &mut TestAppContext,
) {
    tell_the_plugin_it_may_be_resized(true);
    for format in FORMATS {
        resizing_within_limits(format, cx);
    }
    tell_the_plugin_it_may_be_resized(false);
}

fn resizing_within_limits(format: PluginFormat, cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let (harness, handle) = open(format, &log, cx);
    assert_eq!(content_size(handle, cx), (WINDOW_WIDTH, WINDOW_HEIGHT));
    let before = window_calls(&log).len();

    cx.simulate_window_resize(handle, size(px(640.), px(480.)));
    let calls = window_calls(&log).split_off(before);
    assert_eq!(calls, ["gui_adjust_size", "gui_on_size"], "{format:?}");
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(content_size(handle, cx), (640, 480), "{format:?}");

    cx.simulate_window_resize(handle, size(px(100.), px(100.)));
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(content_size(handle, cx), SMALLEST_WINDOW, "{format:?}");
    // The size the host gave is not told to the plugin again.
    let told = window_calls(&log);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(window_calls(&log), told);

    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
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
#[gpui::test]
fn a_plugin_that_does_not_adjust_a_size_takes_it_as_it_was_dragged(cx: &mut TestAppContext) {
    tell_the_plugin_it_may_be_resized(true);
    tell_the_plugin_not_to_adjust(true);
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        let (harness, handle) = open(format, &log, cx);
        let before = window_calls(&log).len();
        cx.simulate_window_resize(handle, size(px(100.), px(90.)));
        let calls = window_calls(&log).split_off(before);
        assert_eq!(calls, ["gui_adjust_size", "gui_on_size"], "{format:?}");
        cx.update(|cx| harness.plugins.settle_windows(cx));
        assert_eq!(content_size(handle, cx), (100, 90), "{format:?}");
        cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    }
    tell_the_plugin_not_to_adjust(false);
    tell_the_plugin_it_may_be_resized(false);
}

/// `onSize` is never called inside `onSize`. The window ends on the size the view then says it
/// has.
#[gpui::test]
fn a_view_that_asks_for_a_size_inside_the_answer_to_a_drag_is_not_answered_inside_it(
    cx: &mut TestAppContext,
) {
    tell_the_plugin_it_may_be_resized(true);
    tell_the_plugin_to_ask_again_from_inside_the_answer(500, 400);
    let folder = tempfile::tempdir().unwrap();
    let log = folder.path().join("calls.txt");
    let (harness, handle) = open(PluginFormat::Vst3, &log, cx);
    let before = window_calls(&log).len();
    cx.simulate_window_resize(handle, size(px(640.), px(480.)));
    let calls = window_calls(&log).split_off(before);
    assert_eq!(
        calls,
        ["gui_adjust_size", "gui_on_size", "gui_request_resize"],
        "{calls:?}"
    );
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(content_size(handle, cx), (500, 400));
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    tell_the_plugin_to_ask_again_from_inside_the_answer(0, 0);
    tell_the_plugin_it_may_be_resized(false);
}

#[gpui::test]
fn a_window_the_plugin_does_not_allow_to_resize_asks_it_nothing(cx: &mut TestAppContext) {
    tell_the_plugin_it_may_be_resized(false);
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        let (harness, handle) = open(format, &log, cx);
        cx.simulate_window_resize(handle, size(px(640.), px(480.)));
        cx.update(|cx| harness.plugins.settle_windows(cx));
        let calls = window_calls(&log);
        assert!(!calls.contains(&"gui_adjust_size".to_string()), "{calls:?}");
        assert!(!calls.contains(&"gui_on_size".to_string()), "{calls:?}");
        cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    }
}

/// A key the plugin's own view does not take reaches the window, and the host passes it on with
/// `IPlugView::onKeyDown` and `onKeyUp`. CLAP has no such call.
#[gpui::test]
fn a_key_typed_in_a_plugins_window_reaches_a_vst3_view(cx: &mut TestAppContext) {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("calls.txt");
        let (harness, handle) = open(format, &log, cx);
        // `secondary` is command on macOS and control on Windows, which VST 3 both calls command.
        cx.simulate_keystrokes(handle, "a shift-b enter secondary-c");
        let up = PlatformInput::KeyUp(KeyUpEvent {
            keystroke: Keystroke::parse("a").unwrap(),
        });
        cx.update(|cx| handle.update(cx, |_, window, cx| window.dispatch_event(up, cx)))
            .unwrap();
        let keys: Vec<String> = window_calls(&log)
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
        assert_eq!(keys, expected, "{format:?}");
        cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    }
}
