//! The scan on a thread of its own, and the cache of this machine.
//!
//! The window must never look at a plugin on the thread that draws: a bundle that hangs costs
//! ten seconds, and this machine's real folders cost seconds even when nothing hangs. So the
//! window starts the scan on a thread and opens the project at once, and a record whose plugin
//! the scan has not reached yet is reported and played as soon as it turns up.

use std::time::{Duration, Instant};

use plugin_host::{PluginFormat, Plugins, ScanCache, ScanCommand};

use crate::support::{
    Harness, Played, id, no_cache, plugin_folder, plugin_folder_of, plugin_id, record, scanner,
};

/// How long a bundle may take in these tests. Shorter than the ten seconds of a real scan, so
/// a test waits seconds and not minutes, and long enough that a bundle which answers is never
/// given up on because the machine was busy.
const DEADLINE: Duration = Duration::from_secs(2);

/// A scanner where the bundle of one format never answers and the other does, so a scan has
/// one of each.
fn one_bundle_hangs(format: PluginFormat) -> ScanCommand {
    scanner()
        .with_environment("SOUND_TOOLS_TEST_PLUGIN_HANG", format.as_str())
        .with_timeout(DEADLINE)
}

/// A scanner whose bundles all wait at `gate` until [`open_the_gate`]. Nothing is given up on:
/// the deadline is the usual one and is never meant to be reached, so a busy machine only
/// makes the test slower and never makes it fail.
fn waiting_at(gate: &std::path::Path) -> ScanCommand {
    let path = gate.to_str().expect("a temporary folder of this test");
    scanner().with_environment(test_plugin_support::GATE_VARIABLE, path)
}

fn open_the_gate(gate: &std::path::Path) {
    let open = std::path::PathBuf::from(format!("{}.go", gate.display()));
    std::fs::write(open, []).expect("the gate opens");
}

/// What the composer's thread does while the scan runs: it draws, which means it asks the host
/// what it knows. None of that may wait for a bundle.
#[test]
fn a_scan_in_the_background_never_blocks_the_thread_that_started_it() {
    let folder = tempfile::tempdir().unwrap();
    let search = vec![plugin_folder(folder.path())];
    let plugins = Plugins::new(search, one_bundle_hangs(PluginFormat::Vst3), no_cache());

    let started = Instant::now();
    plugins.start_scanning();
    // Everything the window asks while it draws, over and over, while a bundle hangs.
    let mut asked = 0;
    while started.elapsed() < Duration::from_millis(200) {
        plugins.instruments();
        plugins.installed_name(PluginFormat::Clap, plugin_id(PluginFormat::Clap));
        plugins.scan_is_running();
        asked += 1;
    }
    let longest = Instant::now();
    plugins.instruments();
    assert!(
        longest.elapsed() < Duration::from_millis(50),
        "the picker waited {:?} for the scan",
        longest.elapsed()
    );
    assert!(asked > 100, "only {asked} answers in 200 ms");
    assert!(plugins.scan_is_running(), "the scan was already over");

    // And when it ends, the picker has what answered and the bundle that hung is reported.
    plugins.wait_for_scan();
    let scan = plugins.scan();
    assert!(scan.finished);
    assert_eq!(scan.bundles, 2);
    let clap = PluginFormat::Clap;
    assert!(scan.find(clap, plugin_id(clap)).is_some(), "{scan:?}");
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
    assert!(scan.failures[0].path.ends_with("test-tone.vst3"));
    assert_eq!(plugins.instruments().len(), 1);
}

/// A project that names a plugin the scan has not reached yet opens, says so, and plays the
/// plugin as soon as it turns up. Nothing of the composer's is needed in between.
///
/// The scan is held at a gate this test opens itself, so what it checks does not depend on how
/// busy the machine is. With a bundle that hangs and a short deadline instead, the bundle that
/// answers was killed on its own deadline when ten tests ran at once.
#[test]
fn a_record_waiting_for_the_scan_is_reported_and_plays_when_the_plugin_turns_up() {
    let folder = tempfile::tempdir().unwrap();
    // Every bundle waits at the gate, so no plugin of this project is found while it is shut.
    let gate = folder.path().join("scan-gate");
    let search = vec![plugin_folder(folder.path())];
    let plugins = Plugins::new(search, waiting_at(&gate), no_cache());
    plugins.start_scanning();

    let mut harness = Harness::with_plugins(folder, plugins);
    let played = (0..40)
        .map(|index| Played::On {
            frame: index * 512,
            pitch: 60,
            velocity: 100,
        })
        .collect();
    harness.add_track(record(PluginFormat::Vst3, "piano"), played);

    // The project is open and everything else in it plays. The track is silent and says why.
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("still being looked at"),
        "{problems:?}"
    );
    assert_eq!(harness.play(2048).first_sound(), None);

    // The gate opens, the scan ends, the host asks for the record to be run again, and it plays.
    open_the_gate(&gate);
    harness.plugins.wait_for_scan();
    let scan = harness.plugins.scan();
    let vst3 = PluginFormat::Vst3;
    assert!(scan.find(vst3, plugin_id(vst3)).is_some(), "{scan:?}");
    harness.plugins.poll(&harness.project);
    let retries = harness.plugins.take_retries();
    assert_eq!(retries, vec![id("track/instrument")], "{retries:?}");
    for instance in &retries {
        assert!(harness.project.rebind(instance).unwrap());
    }
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert!(harness.render(2048).first_sound().is_some());
}

/// The cache of this machine. Without it every start pays for every bundle, which on the
/// machine this was written on is seconds. See README.md for the numbers.
#[test]
fn a_bundle_that_has_not_changed_is_not_looked_at_again() {
    let folder = tempfile::tempdir().unwrap();
    let cache = ScanCache::at(folder.path().join("plugins.json"));
    let search = vec![plugin_folder(folder.path())];

    let plugins = Plugins::new(search.clone(), scanner(), cache.clone());
    assert_eq!(plugins.scan().plugins.len(), 2);

    // A scanner that cannot start at all. Everything is still found, so no child process ran.
    let broken = ScanCommand::new("/definitely/not/a/program", []);
    let plugins = Plugins::new(search.clone(), broken.clone(), cache.clone());
    let scan = plugins.scan();
    assert_eq!(scan.failures, [], "a bundle was looked at again");
    assert_eq!(scan.plugins.len(), 2);

    // The binary of one bundle changes, so that bundle is looked at again and this one fails.
    let binary = folder.path().join("plugins/test-tone.clap");
    let bytes = std::fs::read(&binary).unwrap();
    std::fs::write(&binary, bytes).unwrap();
    filetime_now(&binary);
    let plugins = Plugins::new(search, broken, cache);
    let scan = plugins.scan();
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
    assert!(scan.failures[0].path.ends_with("test-tone.clap"));
    assert_eq!(scan.plugins.len(), 1);
}

/// A plugin that crashed or hung is not tried again on every start: it would cost its deadline
/// every time the composer opened a project. `runtime --plugins` looks at everything again.
#[test]
fn a_bundle_that_crashed_is_remembered_and_not_tried_again() {
    let folder = tempfile::tempdir().unwrap();
    let cache = ScanCache::at(folder.path().join("plugins.json"));
    let search = vec![plugin_folder_of(folder.path(), PluginFormat::Clap)];
    let crashing = scanner().with_environment("SOUND_TOOLS_TEST_PLUGIN_CRASH", "1");

    let plugins = Plugins::new(search.clone(), crashing, cache.clone());
    assert_eq!(plugins.scan().failures.len(), 1);

    // A scanner that works. The bundle is not looked at again, so it is still a failure.
    let plugins = Plugins::new(search.clone(), scanner(), cache.clone());
    let scan = plugins.scan();
    assert_eq!(scan.plugins, [], "the bundle was tried again");
    assert_eq!(scan.failures.len(), 1);

    // Unless everything is looked at again, which is what `runtime --plugins` does.
    let plugins = Plugins::new(search, scanner(), cache.refreshing());
    let scan = plugins.scan();
    assert_eq!(scan.failures, []);
    assert_eq!(scan.plugins.len(), 1);
}

/// Touches a file so that its modified time is now, whatever it was. Writing the same bytes
/// does that on macOS, but not with enough resolution to be sure inside one test.
fn filetime_now(path: &std::path::Path) {
    let file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("the binary");
    // One byte more is a different binary, which is exactly what the cache is keyed on.
    use std::io::Write as _;
    let mut file = file;
    file.write_all(&[0]).expect("the binary is touched");
}

/// A plugin installed while the app runs is found without a restart, when a picker asks what
/// there is: the window's host looks at the plugin folders again then. A look that finds
/// nothing new changes nothing, so the picker it fills again does not make it look for ever.
///
/// The loops below ask until the plugin turns up, so the test does not depend on when the
/// thread that looks again runs.
#[test]
fn a_plugin_installed_while_the_app_runs_is_in_the_next_picker() {
    let folder = tempfile::tempdir().unwrap();
    let search = vec![plugin_folder_of(folder.path(), PluginFormat::Clap)];
    let plugins = Plugins::new(search, scanner(), no_cache());
    plugins.start_scanning();
    plugins.wait_for_scan();
    assert_eq!(plugins.instruments().len(), 1);

    test_vst3_plugin::install_into(&folder.path().join("plugins"));
    let started = Instant::now();
    while plugins.instruments().len() < 2 {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the plugin was not found"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let generation = plugins.scan_generation();
    for _ in 0..20 {
        plugins.instruments();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(plugins.scan_generation(), generation);
    assert_eq!(plugins.instruments().len(), 2);
}

/// A record that names a plugin this machine does not have plays once the composer installs
/// it, with no picker open and no restart: the host looks again while a record waits. The loop
/// is what the window's poll does.
#[test]
fn a_record_whose_plugin_is_installed_while_the_app_runs_plays() {
    let folder = tempfile::tempdir().unwrap();
    let search = vec![plugin_folder_of(folder.path(), PluginFormat::Clap)];
    let plugins = Plugins::new(search, scanner(), no_cache());
    plugins.start_scanning();
    plugins.wait_for_scan();
    let mut harness = Harness::with_plugins(folder, plugins);
    let played = (0..40)
        .map(|index| Played::On {
            frame: index * 512,
            pitch: 60,
            velocity: 100,
        })
        .collect();
    harness.add_track(record(PluginFormat::Vst3, "piano"), played);
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("this machine has no"), "{problems:?}");

    test_vst3_plugin::install_into(&harness.folder.path().join("plugins"));
    let started = Instant::now();
    while !harness.problems().is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the record still waits: {:?}",
            harness.problems()
        );
        std::thread::sleep(Duration::from_millis(20));
        harness.plugins.poll(&harness.project);
        for instance in harness.plugins.take_retries() {
            harness.project.rebind(&instance).unwrap();
        }
    }
    assert!(harness.play(2048).first_sound().is_some());
}
