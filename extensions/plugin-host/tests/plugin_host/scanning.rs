//! The scan: what it finds, what it says about a plugin that is not an instrument, and what
//! happens when the plugin crashes while it is being looked at.

use plugin_host::{Plugins, ScanCommand};

use crate::support::{plugin_folder, scanner};

#[test]
fn the_scan_finds_the_test_plugin_and_says_it_is_an_instrument() {
    let folder = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], scanner());
    let scan = plugins.scan();
    assert_eq!(scan.bundles, 1);
    assert_eq!(scan.failures, []);
    let found = scan.find(test_clap_plugin::PLUGIN_ID).expect("the plugin");
    assert_eq!(found.name, "Sound Tools Test Tone");
    assert_eq!(found.vendor, "Sound Tools");
    assert!(found.is_instrument(), "{:?}", found.features);
    assert_eq!(found.path, folder.path().join("plugins/test-tone.clap"));
}

#[test]
fn a_plugin_that_is_not_an_instrument_is_not_one() {
    let folder = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], scanner());
    let mut found = plugins
        .scan()
        .find(test_clap_plugin::PLUGIN_ID)
        .unwrap()
        .clone();
    found.features = vec!["audio-effect".to_string(), "stereo".to_string()];
    assert!(!found.is_instrument());
}

#[test]
fn a_search_folder_that_is_not_there_is_no_error() {
    let folder = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(vec![folder.path().join("nothing-here")], scanner());
    let scan = plugins.scan();
    assert_eq!(scan.bundles, 0);
    assert_eq!(scan.plugins, []);
    assert_eq!(scan.failures, []);
}

/// The whole reason the scan is a child process.
#[test]
fn a_plugin_that_crashes_while_it_is_scanned_is_reported_and_this_process_lives() {
    let folder = tempfile::tempdir().unwrap();
    let crashing = scanner().with_environment("SOUND_TOOLS_TEST_PLUGIN_CRASH", "1");
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], crashing);
    let scan = plugins.scan();
    assert_eq!(scan.bundles, 1);
    assert_eq!(scan.plugins, []);
    assert_eq!(scan.failures.len(), 1, "{:?}", scan.failures);
    let failure = &scan.failures[0];
    assert!(failure.path.ends_with("test-tone.clap"), "{failure:?}");
    assert!(
        failure.message.contains("signal") || failure.message.contains("exit"),
        "{failure:?}"
    );
    let notices = plugins.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains("could not be scanned"), "{notices:?}");
    assert_eq!(plugins.take_notices(), Vec::<String>::new());

    // This process is still here, and a scanner that works still finds the plugin.
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], scanner());
    assert!(plugins.scan().find(test_clap_plugin::PLUGIN_ID).is_some());
}

/// Real plugins print while they load. The scan reads its own lines and leaves theirs alone.
#[test]
fn a_plugin_that_prints_while_it_is_scanned_is_still_found() {
    let folder = tempfile::tempdir().unwrap();
    let chatty = scanner().with_environment("SOUND_TOOLS_TEST_PLUGIN_CHATTER", "1");
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], chatty);
    let scan = plugins.scan();
    assert_eq!(scan.failures, []);
    assert!(scan.find(test_clap_plugin::PLUGIN_ID).is_some(), "{scan:?}");
}

#[test]
fn a_scanner_that_does_not_exist_is_a_failure_per_bundle_and_not_a_panic() {
    let folder = tempfile::tempdir().unwrap();
    let missing = ScanCommand::new("/definitely/not/a/program", []);
    let plugins = Plugins::new(vec![plugin_folder(folder.path())], missing);
    let scan = plugins.scan();
    assert_eq!(scan.plugins, []);
    assert_eq!(scan.failures.len(), 1);
    assert!(
        scan.failures[0].message.contains("did not start"),
        "{:?}",
        scan.failures
    );
}
