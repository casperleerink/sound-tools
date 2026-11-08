//! The scan on a thread of its own, and the cache of this machine. The thread that draws must
//! never wait for a bundle: one that hangs costs a minute.

use std::time::{Duration, Instant};

use plugin_host::{PluginFormat, Plugins, ScanCache, ScanCommand};

use crate::support::{
    Harness, Played, id, no_cache, plugin_folder, plugin_folder_of, plugin_id, record, scanner,
};

/// Short, as the bundle that hangs is the only one: no bundle that answers can miss it.
const DEADLINE: Duration = Duration::from_millis(300);

fn one_bundle_hangs(format: PluginFormat) -> ScanCommand {
    scanner()
        .with_environment(test_plugin_support::HANG_VARIABLE, format.as_str())
        .with_timeout(DEADLINE)
}

/// Bundles wait at `gate` until [`open_the_gate`], with the usual deadline, so a busy machine
/// only makes a test slower and never makes it fail.
fn waiting_at(gate: &std::path::Path) -> ScanCommand {
    let path = gate.to_str().expect("a temporary folder of this test");
    scanner().with_environment(test_plugin_support::GATE_VARIABLE, path)
}

fn open_the_gate(gate: &std::path::Path) {
    let open = std::path::PathBuf::from(format!("{}.go", gate.display()));
    std::fs::write(open, []).expect("the gate opens");
}

#[test]
fn a_scan_in_the_background_never_blocks_the_thread_that_started_it() {
    let folder = tempfile::tempdir().unwrap();
    let gate = folder.path().join("scan-gate");
    let search = vec![plugin_folder(folder.path())];
    let plugins = Plugins::new(search, waiting_at(&gate), no_cache());

    let started = Instant::now();
    plugins.start_scanning();
    // Everything the window asks while it draws.
    let mut slowest = Duration::ZERO;
    while started.elapsed() < Duration::from_millis(200) {
        let asked = Instant::now();
        plugins.instruments();
        plugins.installed_name(PluginFormat::Clap, plugin_id(PluginFormat::Clap));
        plugins.scan_is_running();
        slowest = slowest.max(asked.elapsed());
    }
    // Generous: an answer that waited for the scan would wait for the gate.
    assert!(
        slowest < Duration::from_secs(1),
        "the picker waited {slowest:?} for the scan"
    );
    assert!(plugins.scan_is_running(), "the scan was already over");
    assert_eq!(
        plugins.instruments(),
        [],
        "a bundle answered through the gate"
    );

    open_the_gate(&gate);
    plugins.wait_for_scan();
    let scan = plugins.scan();
    assert!(scan.finished);
    assert_eq!(scan.bundles, 2);
    for format in [PluginFormat::Clap, PluginFormat::Vst3] {
        assert!(scan.find(format, plugin_id(format)).is_some(), "{scan:?}");
    }
    assert_eq!(scan.failures, [], "{:?}", scan.failures);
    assert_eq!(plugins.instruments().len(), 2);
}

#[test]
fn a_bundle_that_hangs_in_a_scan_in_the_background_is_reported() {
    let folder = tempfile::tempdir().unwrap();
    let search = vec![plugin_folder_of(folder.path(), PluginFormat::Vst3)];
    let plugins = Plugins::new(search, one_bundle_hangs(PluginFormat::Vst3), no_cache());
    plugins.start_scanning();
    plugins.wait_for_scan();
    let scan = plugins.scan();
    assert!(scan.finished);
    assert_eq!(scan.bundles, 1);
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
    assert!(scan.failures[0].path.ends_with("test-tone.vst3"));
    assert_eq!(plugins.instruments(), []);
}

/// As Kontakt does on its first load on Windows.
#[test]
fn a_slow_bundle_does_not_hold_back_the_others() {
    let folder = tempfile::tempdir().unwrap();
    let search = vec![plugin_folder(folder.path())];
    let gate = folder.path().join("scan-gate");
    let scanner = waiting_at(&gate).with_environment(
        test_plugin_support::GATE_FORMAT_VARIABLE,
        PluginFormat::Clap.as_str(),
    );
    let plugins = Plugins::new(search, scanner, no_cache());
    plugins.start_scanning();
    let (vst3, started) = (PluginFormat::Vst3, Instant::now());
    while plugins.installed_name(vst3, plugin_id(vst3)).is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the VST 3 plugin waits for the CLAP one"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    open_the_gate(&gate);
    plugins.wait_for_scan();
    let scan = plugins.scan();
    assert_eq!(scan.failures, []);
    assert_eq!(scan.plugins.len(), 2);
}

#[test]
fn a_record_waiting_for_the_scan_is_reported_and_plays_when_the_plugin_turns_up() {
    let folder = tempfile::tempdir().unwrap();
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

    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("still being looked at"),
        "{problems:?}"
    );
    assert_eq!(harness.play(2048).first_sound(), None);

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

/// Without the cache every start pays seconds for every bundle.
#[test]
fn a_bundle_that_has_not_changed_is_not_looked_at_again() {
    let folder = tempfile::tempdir().unwrap();
    let cache = ScanCache::at(folder.path().join("plugins.json"));
    let search = vec![plugin_folder(folder.path())];

    let plugins = Plugins::new(search.clone(), scanner(), cache.clone());
    assert_eq!(plugins.scan().plugins.len(), 2);

    // A scanner that cannot start at all, so a bundle looked at again fails.
    let broken = ScanCommand::new("/definitely/not/a/program", []);
    let plugins = Plugins::new(search.clone(), broken.clone(), cache.clone());
    let scan = plugins.scan();
    assert_eq!(scan.failures, [], "a bundle was looked at again");
    assert_eq!(scan.plugins.len(), 2);

    // A new file replaces the binary, as an installer does: the installed one is a link to the
    // built library, which other tests load.
    let binary = folder.path().join("plugins/test-tone.clap");
    let mut bytes = std::fs::read(&binary).unwrap();
    bytes.push(0);
    let next = folder.path().join("test-tone.clap.next");
    std::fs::write(&next, bytes).unwrap();
    std::fs::rename(&next, &binary).unwrap();
    let plugins = Plugins::new(search, broken, cache);
    let scan = plugins.scan();
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
    assert!(scan.failures[0].path.ends_with("test-tone.clap"));
    assert_eq!(scan.plugins.len(), 1);
}

/// Else it would cost its deadline every time the composer opened a project.
#[test]
fn a_bundle_that_crashed_is_remembered_and_not_tried_again() {
    let folder = tempfile::tempdir().unwrap();
    let cache = ScanCache::at(folder.path().join("plugins.json"));
    let search = vec![plugin_folder_of(folder.path(), PluginFormat::Clap)];
    let crashing = scanner().with_environment(test_plugin_support::CRASH_VARIABLE, "1");

    let plugins = Plugins::new(search.clone(), crashing, cache.clone());
    assert_eq!(plugins.scan().failures.len(), 1);

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

/// A look that finds nothing new changes nothing, so the picker it fills again does not make it
/// look for ever.
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

/// With no picker open: the host looks again while a record waits. The loop is the window's
/// poll.
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

/// Unix only: a folder on Windows has no mode that keeps files from being made in it.
#[cfg(unix)]
#[test]
fn a_cache_that_cannot_be_written_is_told() {
    use std::os::unix::fs::PermissionsExt as _;
    let folder = tempfile::tempdir().unwrap();
    let search = vec![plugin_folder_of(folder.path(), PluginFormat::Clap)];
    let locked = folder.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let cache = ScanCache::at(locked.join("plugins.json"));

    let plugins = Plugins::new(search, scanner(), cache);
    assert_eq!(plugins.scan().plugins.len(), 1);
    let notices = plugins.take_notices();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].contains("the plugin cache") && notices[0].contains("was not written"),
        "{notices:?}"
    );
}
