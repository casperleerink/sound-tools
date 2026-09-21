//! A host that looks a plugin up and loads none, which is what `runtime --inspect` opens.
//!
//! Loading a plugin runs somebody else's code in this process, and one that only loads and
//! goes can take the process down with it. Inspecting prints a project and makes no sound, so
//! it needs no plugin. What it must keep is everything this side can check without one: a
//! plugin this machine does not have, and a state file that cannot be read.

use plugin_host::{PluginFormat, PluginRecord, Plugins};

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

/// Nothing of the plugin's own code runs: it is never activated, never processed and never
/// taken apart. The slot is silent and says nothing, because there is nothing wrong with it.
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
        // The plugin is the one that writes this file, so an empty one is a plugin that never
        // ran. Listing a bundle happens in a child process and writes nothing here.
        assert_eq!(lifecycle(&log), Vec::new(), "{format:?}");
    }
    tell_the_plugin(None, None);
}

/// What an agent reads from `--inspect` is kept: the scan still answers, so a plugin this
/// machine does not have is named with its id.
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

/// The other thing this side can check without the plugin: the state file. A record whose
/// state asset cannot be read would have played silence and said nothing under `--inspect`,
/// and then failed the moment the composer opened the project for real.
#[test]
fn a_state_file_that_cannot_be_read_is_reported_by_a_host_that_only_lists() {
    for format in FORMATS {
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("lifecycle.log");
        tell_the_plugin(Some(&log), None);
        let plugins = listing(folder.path());
        let mut harness = Harness::with_plugins(folder, plugins);
        // A folder where the bytes of the state belong. Reading it is an error of the
        // operating system's, which is what a state file nobody may read looks like.
        let path = harness.project.assets().path(&state_asset("piano"));
        std::fs::create_dir_all(&path).expect("a folder in the way of the state");
        harness.add_track(record(format, "piano"), one_note());

        let problems = harness.problems();
        assert_eq!(problems.len(), 1, "{format:?}: {problems:?}");
        assert!(
            problems[0].contains("could not be read"),
            "{format:?}: {problems:?}"
        );
        // And still no plugin of this machine ran in this process.
        assert_eq!(lifecycle(&log), Vec::new(), "{format:?}");
    }
    tell_the_plugin(None, None);
}
