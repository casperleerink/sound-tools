//! The scan: what it finds, what it says about a plugin that is not an instrument, and what
//! happens when the plugin crashes while it is being looked at.

use std::time::{Duration, Instant};

use plugin_host::{PluginFormat, Plugins, ScanCommand};

use crate::support::{FORMATS, no_cache, plugin_folder, plugin_folder_of, plugin_id, scanner};

#[test]
fn the_scan_finds_the_test_plugin_of_every_format_and_says_it_is_an_instrument() {
    let folder = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], scanner(), no_cache());
    let scan = plugins.scan();
    // One bundle per format, found by the extension of the bundle: one list of folders covers
    // every format this build hosts.
    assert_eq!(scan.bundles, FORMATS.len());
    assert_eq!(scan.failures, []);
    for format in FORMATS {
        let found = scan.find(format, plugin_id(format)).expect("the plugin");
        assert_eq!(found.name, "Sound Tools Test Tone", "{format:?}");
        assert_eq!(found.vendor, "Sound Tools", "{format:?}");
        assert!(found.is_instrument(), "{:?}", found.features);
        let bundle = format!("plugins/test-tone.{}", format.as_str());
        assert_eq!(found.path, folder.path().join(bundle), "{format:?}");
    }
}

#[test]
fn a_plugin_that_is_not_an_instrument_is_not_one() {
    let folder = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], scanner(), no_cache());
    let scan = plugins.scan();
    for format in FORMATS {
        let mut found = scan.find(format, plugin_id(format)).unwrap().clone();
        found.features = vec!["audio-effect".to_string(), "stereo".to_string()];
        assert!(!found.is_instrument(), "{format:?}");
    }
}

#[test]
fn a_search_folder_that_is_not_there_is_no_error() {
    let folder = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(
        vec![folder.path().join("nothing-here")],
        scanner(),
        no_cache(),
    );
    let scan = plugins.scan();
    assert_eq!(scan.bundles, 0);
    assert_eq!(scan.plugins, []);
    assert_eq!(scan.failures, []);
}

/// The whole reason the scan is a child process.
#[test]
fn a_plugin_that_crashes_while_it_is_scanned_is_reported_and_this_process_lives() {
    for format in FORMATS {
        a_crash_while_scanned(format);
    }
}

fn a_crash_while_scanned(format: PluginFormat) {
    let folder = tempfile::tempdir().unwrap();
    let crashing = scanner().with_environment("SOUND_TOOLS_TEST_PLUGIN_CRASH", "1");
    let search = vec![plugin_folder_of(folder.path(), format)];
    let plugins = Plugins::new(search.clone(), crashing, no_cache());
    let scan = plugins.scan();
    assert_eq!(scan.bundles, 1, "{format:?}");
    assert_eq!(scan.plugins, [], "{format:?}");
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
    let failure = &scan.failures[0];
    let bundle = format!("test-tone.{}", format.as_str());
    assert!(failure.path.ends_with(&bundle), "{failure:?}");
    assert!(
        failure.message.contains("signal") || failure.message.contains("exit"),
        "{failure:?}"
    );
    let notices = plugins.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("could not be scanned"), "{notices:?}");
    assert_eq!(plugins.take_notices(), Vec::<String>::new());

    // This process is still here, and a scanner that works still finds the plugin.
    let plugins = Plugins::new(search, scanner(), no_cache());
    assert!(
        plugins.scan().find(format, plugin_id(format)).is_some(),
        "{format:?}"
    );
}

/// Real plugins print while they load. The scan reads its own lines and leaves theirs alone.
#[test]
fn a_plugin_that_prints_while_it_is_scanned_is_still_found() {
    let folder = tempfile::tempdir().unwrap();
    let chatty = scanner().with_environment("SOUND_TOOLS_TEST_PLUGIN_CHATTER", "1");
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], chatty, no_cache());
    let scan = plugins.scan();
    assert_eq!(scan.failures, []);
    for format in FORMATS {
        assert!(scan.find(format, plugin_id(format)).is_some(), "{scan:?}");
    }
}

/// A plugin that leaves a helper process behind, as a licensing one does. The helper inherits
/// the pipe the scan reads, so the pipe does not end when the child does: a scan that waited
/// for the end of that pipe would wait for the helper, past every deadline, and the project
/// would never open. The deadline covers the readers, so the bundle is read and the scan goes
/// on.
#[test]
fn a_plugin_that_leaves_a_helper_holding_the_pipe_does_not_hold_up_the_scan() {
    for format in FORMATS {
        a_helper_that_outlives_the_child(format);
    }
}

fn a_helper_that_outlives_the_child(format: PluginFormat) {
    let folder = tempfile::tempdir().unwrap();
    // The helper holds the pipe until this test lets it go, and says when it has gone.
    let helper = folder.path().join("helper");
    let (stop, done) = (with_suffix(&helper, ".stop"), with_suffix(&helper, ".done"));
    let with_helper = scanner()
        .with_environment(
            "SOUND_TOOLS_TEST_PLUGIN_DESCENDANT",
            helper.to_str().expect("a path"),
        )
        .with_timeout(Duration::from_secs(5));
    let search = vec![plugin_folder_of(folder.path(), format)];
    let plugins = Plugins::new(search, with_helper, no_cache());

    let started = Instant::now();
    let scan = plugins.scan();
    let took = started.elapsed();

    // It came back with the plugin the child printed, while the helper still held the pipe:
    // nothing has told the helper to go yet.
    assert!(took < Duration::from_secs(10), "the scan waited {took:?}");
    assert!(!done.exists(), "{format:?}: the scan waited for the helper");
    assert_eq!(scan.bundles, 1, "{format:?}");
    assert_eq!(scan.failures, [], "{format:?}");
    assert!(
        scan.find(format, plugin_id(format)).is_some(),
        "{format:?}: {scan:?}"
    );

    // Nothing of this test is left running: the helper is told to go and waited for.
    std::fs::write(&stop, b"").unwrap();
    let waited = Instant::now();
    while !done.exists() {
        assert!(
            waited.elapsed() < Duration::from_secs(30),
            "the helper process never ended"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn with_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

#[test]
fn a_scanner_that_does_not_exist_is_a_failure_per_bundle_and_not_a_panic() {
    let folder = tempfile::tempdir().unwrap();
    let missing = ScanCommand::new("/definitely/not/a/program", []);
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], missing, no_cache());
    let scan = plugins.scan();
    assert_eq!(scan.plugins, []);
    assert_eq!(scan.failures.len(), FORMATS.len());
    assert!(
        scan.failures[0].message.contains("did not start"),
        "{:?}",
        scan.failures
    );
}

/// A plugin that never answers while it is listed. Licensed plugins do this when they cannot
/// reach their server, and the scan runs while a project opens, so waiting for it would hold
/// the project open for ever. The child is stopped and the bundle is reported like one that
/// crashed.
#[test]
fn a_plugin_that_hangs_while_it_is_scanned_is_given_up_on_and_reported() {
    for format in FORMATS {
        a_hang_while_scanned(format);
    }
}

fn a_hang_while_scanned(format: PluginFormat) {
    let folder = tempfile::tempdir().unwrap();
    let hanging = scanner()
        .with_environment("SOUND_TOOLS_TEST_PLUGIN_HANG", "1")
        .with_timeout(Duration::from_millis(300));
    let search = vec![plugin_folder_of(folder.path(), format)];
    let plugins = Plugins::new(search, hanging, no_cache());

    let started = Instant::now();
    let scan = plugins.scan();
    let took = started.elapsed();

    // The plugin sleeps for ten minutes. This came back in a fraction of a second.
    assert!(took < Duration::from_secs(10), "the scan waited {took:?}");
    assert_eq!(scan.plugins, [], "{format:?}");
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
    assert!(
        scan.failures[0].message.contains("did not finish within"),
        "{:?}",
        scan.failures
    );

    // It is one line for a person, like any other bundle that could not be read.
    let notices = plugins.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("could not be scanned"), "{notices:?}");
}
