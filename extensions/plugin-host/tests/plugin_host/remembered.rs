//! Where a plugin's window was and whether it was open, kept in `workspace.json` and given
//! back when the project opens, and when the plugin reloads.
//!
//! No display is needed: GPUI's platform for tests places a window where it was asked to, on
//! one display of 1920 by 1080. What it cannot do is let a person drag a window, so a window
//! is placed by the file here, and a real drag is checked by hand.

use gpui::TestAppContext;
use plugin_host::PluginFormat;
use test_plugin_support::RELOAD_KEY;

use crate::support::{FORMATS, Harness, Played, id, record, tell_the_plugin};

const SLOT: &str = "track/instrument";

/// A window of the test plugin that was open at 100, 60 on the display.
const OPEN_AT_100_60: &str =
    r#"{"plugin_windows": {"track/instrument": {"open": true, "x": 100, "y": 60}}}"#;

fn windows(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.windows().len())
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

/// What `workspace.json` says about the window of the plugin of the track.
fn remembered(harness: &Harness) -> serde_json::Value {
    let text = std::fs::read_to_string(harness.path("workspace.json")).expect("the file");
    let whole: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    whole["plugin_windows"][SLOT].clone()
}

/// The project on `harness`'s folder, closed and opened again as quitting and starting do:
/// every window taken down by the host, the project closed, and a new host on the same folder.
fn close_and_open_again(harness: Harness, cx: &mut TestAppContext) -> Harness {
    cx.update(|cx| harness.plugins.close_all_windows(cx));
    let problems = harness.plugins.close(&harness.project);
    assert!(problems.is_empty(), "{problems:?}");
    let Harness {
        project,
        engine,
        plugins,
        folder,
    } = harness;
    drop((project, engine, plugins));
    let harness = Harness::open(folder, true);
    harness.plugins.poll(&harness.project);
    harness
}

#[gpui::test]
fn a_window_that_was_open_opens_where_it_was_when_the_project_opens(cx: &mut TestAppContext) {
    for format in FORMATS {
        windows_are_remembered(format, cx);
    }
}

fn windows_are_remembered(format: PluginFormat, cx: &mut TestAppContext) {
    tell_the_plugin(None, None);
    let mut harness = Harness::new();
    std::fs::write(harness.path("workspace.json"), OPEN_AT_100_60).unwrap();
    harness.add_track(record(format, "piano"), Vec::new());
    // The first poll reads the file; the window comes back once there is an application.
    harness.play(512);
    assert_eq!(windows(cx), 0);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert!(harness.plugins.window_is_open(&id(SLOT)));
    assert_eq!(windows(cx), 1);
    assert_eq!(window_origin(cx), (100, 60), "{format:?}");
    // None of it is an edit: the last undo step is still the one that added the track, and
    // no record changed.
    assert_eq!(harness.project.undo_label(), Some("Add track"));

    // Quitting with the window open: it is taken down, and the project remembers it as open.
    let harness = close_and_open_again(harness, cx);
    assert_eq!(remembered(&harness)["open"], true, "{format:?}");
    assert_eq!(windows(cx), 0);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 1);
    assert_eq!(window_origin(cx), (100, 60), "{format:?}");

    // The composer closes it: the project remembers that too, and where it was.
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    let harness = close_and_open_again(harness, cx);
    assert_eq!(remembered(&harness)["open"], false, "{format:?}");
    assert_eq!(remembered(&harness)["x"], 100, "{format:?}");
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);
    assert_eq!(harness.project.undo_label(), None);

    // Opened again by the composer, it opens where it was.
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .unwrap();
    assert_eq!(window_origin(cx), (100, 60), "{format:?}");
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    assert_eq!(windows(cx), 0);
}

/// A window whose record is gone is forgotten the next time the file is written, and a
/// window of a display that is not there any more, or that the window would be off, opens in
/// the middle of the main one.
#[gpui::test]
fn a_window_off_every_display_opens_in_the_middle_and_a_gone_record_is_forgotten(
    cx: &mut TestAppContext,
) {
    tell_the_plugin(None, None);
    let mut harness = Harness::new();
    std::fs::write(
        harness.path("workspace.json"),
        r#"{"plugin_windows": {
            "track/instrument": {"open": true, "x": 5000, "y": 60},
            "gone/instrument": {"open": true, "x": 1, "y": 1}
        }}"#,
    )
    .unwrap();
    harness.add_track(record(PluginFormat::Clap, "piano"), Vec::new());
    harness.play(512);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 1);
    let (x, y) = window_origin(cx);
    // The middle of the test display, 1920 by 1080, for a window of 320 by 240.
    assert_eq!((x, y), (800, 420));
    let harness = close_and_open_again(harness, cx);
    let text = std::fs::read_to_string(harness.path("workspace.json")).unwrap();
    assert!(!text.contains("gone/instrument"), "{text}");
    assert_eq!(remembered(&harness)["x"], 800);
    cx.update(|cx| harness.plugins.close_all_windows(cx));
}

/// A VST 3 plugin that asks to be loaded again (`kReloadComponent`) is a new plugin, and the
/// window of the old one goes with it. The new one's window comes back where the old one was.
#[gpui::test]
fn a_plugin_that_reloads_gets_its_window_back_where_it_was(cx: &mut TestAppContext) {
    tell_the_plugin(None, None);
    let mut harness = Harness::new();
    std::fs::write(harness.path("workspace.json"), OPEN_AT_100_60).unwrap();
    harness.add_track(
        record(PluginFormat::Vst3, "piano"),
        vec![Played::On {
            frame: 2048,
            pitch: RELOAD_KEY,
            velocity: 100,
        }],
    );
    harness.play(512);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert!(harness.plugins.window_is_open(&id(SLOT)));

    harness.render(4096);
    let retries = harness.plugins.take_retries();
    assert_eq!(retries, [id(SLOT)]);
    for instance in &retries {
        assert!(harness.project.rebind(instance).unwrap());
    }
    // The old plugin's window went with it.
    assert!(!harness.plugins.window_is_open(&id(SLOT)));
    harness.plugins.poll(&harness.project);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert!(harness.plugins.window_is_open(&id(SLOT)));
    assert_eq!(windows(cx), 1);
    assert_eq!(window_origin(cx), (100, 60));
    cx.update(|cx| harness.plugins.close_all_windows(cx));
}

/// A project opened read-only (`--render`, `--inspect`) never writes the file, and one whose
/// file cannot be read says so and leaves it alone.
#[gpui::test]
fn a_read_only_project_writes_nothing_and_a_file_that_does_not_read_is_left_alone(
    cx: &mut TestAppContext,
) {
    tell_the_plugin(None, None);
    let mut harness = Harness::new();
    harness.add_track(record(PluginFormat::Clap, "piano"), Vec::new());
    let Harness {
        project,
        engine,
        plugins,
        folder,
    } = harness;
    drop((project, engine, plugins));

    let mut harness = Harness::open(folder, false);
    harness.play(512);
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .unwrap();
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    harness.plugins.close(&harness.project);
    assert!(!harness.path("workspace.json").exists());
    let Harness {
        project,
        engine,
        plugins,
        folder,
    } = harness;
    drop((project, engine, plugins));

    let mut harness = Harness::open(folder, true);
    std::fs::write(harness.path("workspace.json"), "[not an object").unwrap();
    let problems = harness.plugins.poll(&harness.project);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].to_string().contains("workspace.json"),
        "{problems:?}"
    );
    harness.play(512);
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .unwrap();
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    let problems = harness.plugins.close(&harness.project);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let text = std::fs::read_to_string(harness.path("workspace.json")).unwrap();
    assert_eq!(text, "[not an object");
}
