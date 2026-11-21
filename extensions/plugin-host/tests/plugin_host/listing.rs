//! A host that looks a plugin up and loads none, which is what `runtime --inspect` opens:
//! loading one runs somebody else's code in this process. It still reports what it can check
//! without one.

use plugin_host::{PluginFormat, PluginRecord, Plugins};
use sound_core::feeds::NoSources;

use crate::support::{
    FORMATS, Harness, Played, lifecycle, no_cache, plugin_folder, record, scanner, state_asset,
    tell_the_plugin,
};

fn listing(folder: &std::path::Path) -> Plugins {
    Plugins::listing(vec![plugin_folder(folder)], scanner(), no_cache())
}

fn one_note() -> Vec<Played> {
    vec![Played::On {
        frame: 0,
        pitch: 60,
        velocity: 100,
    }]
}

#[test]
fn a_host_that_only_lists_never_runs_a_plugin_this_machine_has() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("lifecycle.log");
        tell_the_plugin(Some(&log), None);
        let plugins = listing(folder.path());
        let mut harness = Harness::with_plugins(folder, plugins);
        harness.add_track(record(format, "piano"), one_note());

        assert_eq!(harness.problems(), Vec::<String>::new(), "{format:?}");
        assert_eq!(harness.play(2048).first_sound(), None, "{format:?}");
        // Listing a bundle happens in a child process and writes nothing here.
        assert_eq!(lifecycle(&log), Vec::new(), "{format:?}");
    }
    tell_the_plugin(None, None);
}

#[test]
fn a_plugin_this_machine_does_not_have_is_still_reported_by_a_host_that_only_lists() {
    let folder = tempfile::tempdir().unwrap();
    let plugins = listing(folder.path());
    let mut harness = Harness::with_plugins(folder, plugins);
    let missing = PluginRecord::new(PluginFormat::Clap, "com.example.nothing", "piano").unwrap();
    harness.add_track(missing, one_note());

    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("com.example.nothing"), "{problems:?}");
    assert!(
        problems[0].contains("this machine has no CLAP plugin"),
        "{problems:?}"
    );
}

#[test]
fn a_state_file_that_cannot_be_read_is_reported_by_a_host_that_only_lists() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("lifecycle.log");
        tell_the_plugin(Some(&log), None);
        let plugins = listing(folder.path());
        let mut harness = Harness::with_plugins(folder, plugins);
        // A folder where the state belongs reads as an error of the operating system.
        let path = harness.project.assets().path(&state_asset("piano"));
        std::fs::create_dir_all(&path).expect("a folder in the way of the state");
        harness.add_track(record(format, "piano"), one_note());

        let problems = harness.problems();
        assert_eq!(problems.len(), 1, "{format:?}: {problems:?}");
        assert!(
            problems[0].contains("could not be read"),
            "{format:?}: {problems:?}"
        );
        assert_eq!(lifecycle(&log), Vec::new(), "{format:?}");
    }
    tell_the_plugin(None, None);
}

/// A run without a window scans nothing: a plugin the cache does not remember is not looked
/// at, and the problem says so rather than that it is missing. Once a scan remembers it, it is
/// found.
#[test]
fn a_host_that_only_remembers_scans_no_new_plugin_and_says_so() {
    let installed = tempfile::tempdir().unwrap();
    let folders = vec![plugin_folder(installed.path())];
    let cache = no_cache();
    let open = || {
        let plugins = Plugins::listing(folders.clone(), scanner(), cache.clone().only_remembered());
        let mut harness = Harness::with_plugins(tempfile::tempdir().unwrap(), plugins);
        harness.add_track(record(PluginFormat::Clap, "piano"), one_note());
        harness
    };
    let status = |harness: &Harness| {
        let instrument = sound_core::InstanceId::new("track/instrument").unwrap();
        let facts = sound_core::facts::of(&harness.project, &NoSources, &instrument, None);
        facts.unwrap().to_value()["status"].clone()
    };

    let harness = open();
    let problems = harness.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("a run without a window looks at no new"),
        "{problems:?}"
    );
    // Not "missing": that it is not on this computer is not known.
    assert_eq!(status(&harness), "not_loaded");

    Plugins::listing(folders.clone(), scanner(), cache.clone()).wait_for_scan();
    let harness = open();
    assert_eq!(harness.problems(), Vec::<String>::new());
    assert_eq!(status(&harness), "not_loaded");
}
