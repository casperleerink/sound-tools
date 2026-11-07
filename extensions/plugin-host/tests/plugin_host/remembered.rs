//! Where a plugin's window was and whether it was open, kept by this machine next to the scan
//! cache, never in the project folder.
//!
//! GPUI's test platform places a window where it was asked to, on one display of 1920 by 1080,
//! but cannot drag one. So a test writes the machine's file where a drag would have left it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::TestAppContext;
use plugin_host::{PluginFormat, Plugins, ScanCache};
use test_plugin_support::RELOAD_KEY;

use crate::support::{
    FORMATS, Harness, Played, id, plugin_folder, record, scanner, tell_the_plugin,
};

const SLOT: &str = "track/instrument";

const STORE: &str = "plugin-windows.json";

fn windows(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.windows().len())
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

/// `machine` stands for the cache folder of this machine, where the windows are kept.
fn open_on(folder: tempfile::TempDir, machine: &Path, writes_state: bool) -> Harness {
    let search = vec![plugin_folder(folder.path())];
    let cache = ScanCache::at(machine.join("plugins.json"));
    let plugins = match writes_state {
        true => Plugins::new(search, scanner(), cache),
        false => Plugins::read_only(search, scanner(), cache),
    };
    Harness::with_plugins(folder, plugins)
}

fn project_with_a_plugin(format: PluginFormat, machine: &Path) -> Harness {
    tell_the_plugin(None, None);
    let mut harness = open_on(tempfile::tempdir().unwrap(), machine, true);
    harness.add_track(record(format, "piano"), Vec::new());
    harness
}

/// As a drag and a quit would have left it.
fn remember(harness: &Harness, machine: &Path, windows: serde_json::Value) {
    let project = dunce::canonicalize(harness.folder.path()).unwrap();
    let whole = serde_json::json!({ project.to_string_lossy(): windows });
    std::fs::write(machine.join(STORE), whole.to_string()).unwrap();
}

fn remembered(harness: &Harness, machine: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(machine.join(STORE)).expect("the file");
    let whole: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    let project = dunce::canonicalize(harness.folder.path()).unwrap();
    whole[project.to_string_lossy().as_ref()][SLOT].clone()
}

fn every_file(folder: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(next) = folders.pop() {
        for entry in std::fs::read_dir(next).unwrap().flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => folders.push(path),
                false => {
                    files.insert(path.clone(), std::fs::read(&path).unwrap());
                }
            }
        }
    }
    files
}

/// As quitting and starting do.
fn close_and_open_again(mut harness: Harness, machine: &Path, cx: &mut TestAppContext) -> Harness {
    cx.update(|cx| harness.plugins.close_all_windows(cx));
    let problems = harness.plugins.close(&mut harness.project);
    assert!(problems.is_empty(), "{problems:?}");
    let Harness {
        project,
        engine,
        plugins,
        folder,
    } = harness;
    drop((project, engine, plugins));
    let harness = open_on(folder, machine, true);
    harness.plugins.poll(&harness.project);
    harness
}

#[gpui::test]
fn a_window_that_was_open_opens_where_it_was_and_the_project_folder_does_not_change(
    cx: &mut TestAppContext,
) {
    for format in FORMATS {
        windows_are_remembered(format, cx);
    }
}

fn windows_are_remembered(format: PluginFormat, cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    let machine = machine.path();
    let mut harness = project_with_a_plugin(format, machine);
    harness.play(512);
    // Once round, so the folder has everything the project writes on its own.
    let harness = close_and_open_again(harness, machine, cx);
    let before = every_file(harness.folder.path());
    let point = harness.project.history().unwrap().current();

    remember(
        &harness,
        machine,
        serde_json::json!({ SLOT: {"open": true, "x": 100, "y": 60, "display": 1} }),
    );
    let harness = close_and_open_again(harness, machine, cx);
    assert_eq!(windows(cx), 0);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert!(harness.plugins.window_is_open(&id(SLOT)));
    assert_eq!(windows(cx), 1);
    assert_eq!(window_origin(cx), (100, 60), "{format:?}");
    assert_eq!(harness.project.history().unwrap().current(), point);

    let harness = close_and_open_again(harness, machine, cx);
    assert_eq!(remembered(&harness, machine)["open"], true, "{format:?}");
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 1);
    assert_eq!(window_origin(cx), (100, 60), "{format:?}");

    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    let harness = close_and_open_again(harness, machine, cx);
    assert_eq!(remembered(&harness, machine)["open"], false, "{format:?}");
    assert_eq!(remembered(&harness, machine)["x"], 100, "{format:?}");
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert_eq!(windows(cx), 0);

    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .unwrap();
    assert_eq!(window_origin(cx), (100, 60), "{format:?}");
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    assert_eq!(windows(cx), 0);
    let harness = close_and_open_again(harness, machine, cx);

    assert_eq!(every_file(harness.folder.path()), before, "{format:?}");
}

#[gpui::test]
fn a_window_off_every_display_opens_in_the_middle_and_a_gone_record_is_forgotten(
    cx: &mut TestAppContext,
) {
    for (x, display) in [(5000, 1), (100, 99)] {
        let machine = tempfile::tempdir().unwrap();
        let machine = machine.path();
        let mut harness = project_with_a_plugin(PluginFormat::Clap, machine);
        remember(
            &harness,
            machine,
            serde_json::json!({
                SLOT: {"open": true, "x": x, "y": 60, "display": display},
                "gone/instrument": {"open": true, "x": 1, "y": 1}
            }),
        );
        harness.play(512);
        cx.update(|cx| harness.plugins.settle_windows(cx));
        assert_eq!(windows(cx), 1);
        // The middle of the test display for a window of 320 by 240.
        assert_eq!(window_origin(cx), (800, 420), "{x} on display {display}");
        let harness = close_and_open_again(harness, machine, cx);
        let text = std::fs::read_to_string(machine.join(STORE)).unwrap();
        assert!(!text.contains("gone/instrument"), "{text}");
        assert_eq!(remembered(&harness, machine)["x"], 800);
        cx.update(|cx| harness.plugins.close_all_windows(cx));
    }
}

/// `kReloadComponent` makes a new plugin, and the window of the old one goes with it.
#[gpui::test]
fn a_plugin_that_reloads_gets_its_window_back_where_it_was(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    let machine = machine.path();
    tell_the_plugin(None, None);
    let mut harness = open_on(tempfile::tempdir().unwrap(), machine, true);
    remember(
        &harness,
        machine,
        serde_json::json!({ SLOT: {"open": true, "x": 100, "y": 60} }),
    );
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
    assert!(!harness.plugins.window_is_open(&id(SLOT)));
    harness.plugins.poll(&harness.project);
    cx.update(|cx| harness.plugins.settle_windows(cx));
    assert!(harness.plugins.window_is_open(&id(SLOT)));
    assert_eq!(windows(cx), 1);
    assert_eq!(window_origin(cx), (100, 60));
    cx.update(|cx| harness.plugins.close_all_windows(cx));
}

/// `--render` and `--inspect` open read-only.
#[gpui::test]
fn a_read_only_project_keeps_no_window(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    let machine = machine.path();
    let harness = project_with_a_plugin(PluginFormat::Clap, machine);
    let Harness {
        project,
        engine,
        plugins,
        folder,
    } = harness;
    drop((project, engine, plugins));

    let mut harness = open_on(folder, machine, false);
    harness.play(512);
    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .unwrap();
    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    harness.plugins.close(&mut harness.project);
    assert!(!machine.join(STORE).exists());
}

#[gpui::test]
fn a_store_that_cannot_be_written_is_said_once(cx: &mut TestAppContext) {
    let machine = tempfile::tempdir().unwrap();
    // A file where the folder of the cache should be.
    let blocked = machine.path().join("blocked");
    std::fs::write(&blocked, "").unwrap();
    let mut harness = project_with_a_plugin(PluginFormat::Clap, &blocked);
    harness.play(512);

    cx.update(|cx| harness.plugins.open_window(&id(SLOT), cx))
        .unwrap();
    let now = Instant::now();
    let said = |problems: Vec<plugin_host::PluginProblem>| {
        problems
            .iter()
            .filter(|problem| problem.to_string().contains("plugin windows"))
            .count()
    };
    let first = said(harness.plugins.poll_at(&harness.project, now));
    let second = said(
        harness
            .plugins
            .poll_at(&harness.project, now + Duration::from_secs(1)),
    );
    assert_eq!((first, second), (1, 0));

    cx.update(|cx| harness.plugins.close_window(&id(SLOT), cx));
    let third = said(
        harness
            .plugins
            .poll_at(&harness.project, now + Duration::from_secs(2)),
    );
    assert_eq!(third, 1);
}
