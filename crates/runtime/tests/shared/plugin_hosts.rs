//! The plugin hosts of this crate's tests. None of them looks in the plugin folders of this
//! machine or at its scan cache, so a test runs the same everywhere and never waits for a scan
//! of somebody's plugin collection.

use std::path::{Path, PathBuf};

use plugin_host::{Plugins, ScanCache, ScanCommand};
use runtime::{open_or_create_with, open_read_only_with};
use sound_core::{Engine, EngineControl, Project};

/// The real `runtime` executable with its scan argument, so the child process of a scan is the
/// one the application starts.
pub(crate) fn scanner() -> ScanCommand {
    ScanCommand::new(
        env!("CARGO_BIN_EXE_runtime"),
        [std::ffi::OsString::from(plugin_host::SCAN_ARGUMENT)],
    )
}

/// Puts the repository's test plugin of every format into `plugins/` inside `root`. What it
/// returns is all a host with the test plugin looks in.
pub(crate) fn test_plugin_folders(root: &Path) -> Vec<PathBuf> {
    let folder = root.join("plugins");
    test_clap_plugin::install_into(&folder);
    test_vst3_plugin::install_into(&folder);
    vec![folder]
}

/// A plugin host that scans only that folder, so the test plugin is all it finds.
pub(crate) fn test_plugin_host(root: &Path) -> Plugins {
    Plugins::new(test_plugin_folders(root), scanner(), ScanCache::none())
}

/// Opens the project as the application does, with a host that has no folder to look in: it
/// finds no plugin and starts no child process. For every test that needs no plugin.
pub(crate) fn open_or_create(folder: &Path, control: EngineControl) -> (Project, Plugins) {
    let plugins = Plugins::new(Vec::new(), scanner(), ScanCache::none());
    let project = open_or_create_with(folder, control, plugins.clone()).unwrap();
    (project, plugins)
}

/// Opens the project as `--render` does, without its lock, with a host that finds no plugin.
pub(crate) fn open_read_only(folder: &Path) -> (Project, Engine, Plugins) {
    let plugins = Plugins::read_only(Vec::new(), scanner(), ScanCache::none());
    let (project, engine) = open_read_only_with(folder, plugins.clone()).unwrap();
    (project, engine, plugins)
}
