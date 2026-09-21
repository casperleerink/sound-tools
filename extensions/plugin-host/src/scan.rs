//! The scan: what CLAP plugins this machine has, found in another process.
//!
//! Loading a plugin means running its code. A plugin that crashes while it is being looked at
//! must not take the application down, so the scan runs in a child process, one child per
//! bundle: a crash costs one bundle and is reported. The child is this program with one
//! argument ([`SCAN_ARGUMENT`]); tests use a small program of their own.
//!
//! There is no cache. See README.md for what a scan of this machine costs.
//!
//! Every child has a deadline. A plugin that hangs while it is listed, which licensed ones do
//! when they cannot reach their server, would otherwise hold the project open for ever.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The argument that makes a program scan one bundle and print the result. The runtime passes
/// it to its own executable.
pub const SCAN_ARGUMENT: &str = "--scan-clap";

/// Every line the child means is marked with this. Loading a plugin runs its code, and a real
/// one prints to standard output while it initializes. Without the mark its logging would be
/// read as a plugin, or would make the whole bundle unreadable.
const MARK: &str = "sound-tools-clap ";

/// How long one bundle may take. Measured September 20, 2026 on an Apple Silicon laptop: a real
/// bundle costs about 10 ms, so this is a thousand times what a working plugin needs, and it is
/// what a plugin that never answers costs the project once.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the child is looked at while the deadline runs. Short enough that a normal scan
/// pays nothing, long enough that waiting costs no thread.
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// What a bundle holds. A bundle can hold several plugins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScannedPlugin {
    /// The plugin's own id, what a record names.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// The CLAP features of the plugin, such as `instrument` or `audio-effect`.
    pub features: Vec<String>,
    /// The bundle this plugin came from. The child does not know it; the parent fills it in.
    #[serde(default)]
    pub path: PathBuf,
}

impl ScannedPlugin {
    /// Whether the plugin plays notes. Only these fit the `instrument` child of a track.
    /// A plugin that says nothing about itself is not offered as one.
    pub fn is_instrument(&self) -> bool {
        self.features.iter().any(|feature| feature == "instrument")
    }
}

/// A bundle that could not be scanned, and why. It is reported, and the scan goes on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanFailure {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub plugins: Vec<ScannedPlugin>,
    pub failures: Vec<ScanFailure>,
    /// Bundles looked at, including the ones that failed.
    pub bundles: usize,
}

impl Scan {
    pub fn find(&self, plugin_id: &str) -> Option<&ScannedPlugin> {
        self.plugins.iter().find(|plugin| plugin.id == plugin_id)
    }
}

/// How to start the child that scans one bundle.
#[derive(Clone, Debug)]
pub struct ScanCommand {
    program: PathBuf,
    arguments: Vec<OsString>,
    /// Extra environment for the child only, so a test can change what the child does without
    /// changing its own environment.
    environment: Vec<(OsString, OsString)>,
    /// How long one bundle may take before its child is killed.
    timeout: Duration,
}

impl ScanCommand {
    /// A program that takes the bundle path as its last argument.
    pub fn new(program: impl Into<PathBuf>, arguments: impl IntoIterator<Item = OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: arguments.into_iter().collect(),
            environment: Vec::new(),
            timeout: SCAN_TIMEOUT,
        }
    }

    /// This program with [`SCAN_ARGUMENT`], which is how the runtime scans. Whoever runs it
    /// must handle that argument with [`scan_one_bundle`].
    pub fn this_program() -> std::io::Result<Self> {
        let program = std::env::current_exe()?;
        Ok(Self::new(program, [OsString::from(SCAN_ARGUMENT)]))
    }

    pub fn with_environment(mut self, name: &str, value: &str) -> Self {
        self.environment
            .push((OsString::from(name), OsString::from(value)));
        self
    }

    /// Another deadline per bundle, so a test does not wait [`SCAN_TIMEOUT`].
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Scans one bundle in a child process. An error here is the child's, not ours: it failed
    /// to start, crashed, or printed something we could not read.
    fn scan(&self, bundle: &Path) -> Result<Vec<ScannedPlugin>, String> {
        let mut command = Command::new(&self.program);
        command.args(&self.arguments).arg(bundle);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        for (name, value) in &self.environment {
            command.env(name, value);
        }
        // Blocking here is the point: the scan is what the project waits for, once. `output`
        // would wait for ever, and a deadline needs a handle to kill.
        #[allow(clippy::disallowed_methods)]
        let mut child = command
            .spawn()
            .map_err(|error| format!("the scanner did not start: {error}"))?;
        let deadline = Instant::now() + self.timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(error) => return Err(format!("the scanner could not be waited for: {error}")),
            }
            if Instant::now() >= deadline {
                // Killed and then waited for, so no child of this process is left behind.
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the scanner did not finish within {:?} and was stopped. The plugin hangs while it is listed",
                    self.timeout
                ));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("the scanner could not be read: {error}"))?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            let message = message.trim();
            let reason = if message.is_empty() {
                // A crash gives no message. The exit status says how it ended.
                format!("the scanner ended with {}", output.status)
            } else {
                message.to_string()
            };
            return Err(reason);
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut plugins = Vec::new();
        // Anything the plugin itself printed is between these lines. It is not ours to read.
        for line in text.lines().filter_map(|line| line.strip_prefix(MARK)) {
            let plugin = serde_json::from_str(line)
                .map_err(|error| format!("the scanner printed something unreadable: {error}"))?;
            plugins.push(plugin);
        }
        Ok(plugins)
    }
}

/// How deep to look inside a search folder. Plugins usually sit one folder per vendor deep.
const MAX_DEPTH: usize = 4;

/// The folders macOS keeps CLAP plugins in, plus `CLAP_PATH` from the environment.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/CLAP"));
    }
    paths.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    if let Some(extra) = std::env::var_os("CLAP_PATH") {
        paths.extend(std::env::split_paths(&extra));
    }
    paths
}

/// Every `.clap` bundle under `folders`, in a fixed order so a scan is repeatable.
pub fn bundles(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for folder in folders {
        collect(folder, 0, &mut found);
    }
    found.sort();
    found.dedup();
    found
}

fn collect(folder: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        // A search folder that is not there is normal: this machine has no plugins of that kind.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "clap")
        {
            // A bundle is a folder on macOS and a file elsewhere. Either way it is one entry
            // and nothing inside it is another plugin.
            found.push(path);
        } else if path.is_dir() {
            collect(&path, depth + 1, found);
        }
    }
}

/// Scans every bundle under `folders`, each in its own child process.
pub fn scan(folders: &[PathBuf], command: &ScanCommand) -> Scan {
    let bundles = bundles(folders);
    let mut scan = Scan {
        bundles: bundles.len(),
        ..Scan::default()
    };
    for bundle in bundles {
        match command.scan(&bundle) {
            Ok(plugins) => scan
                .plugins
                .extend(plugins.into_iter().map(|plugin| ScannedPlugin {
                    path: bundle.clone(),
                    ..plugin
                })),
            Err(message) => scan.failures.push(ScanFailure {
                path: bundle,
                message,
            }),
        }
    }
    scan.plugins.sort_by(|left, right| left.id.cmp(&right.id));
    scan
}

/// The child side: loads one bundle and prints one line of JSON per plugin in it.
///
/// Everything that can go wrong here is the plugin's. The caller is a process of its own, so a
/// crash inside the plugin's own code costs this process and nothing else.
pub fn scan_one_bundle(bundle: &Path) -> Result<String, String> {
    // SAFETY: loading a plugin runs its code, which no host can check in advance. This is why
    // the scan runs in a child process. See the module documentation.
    let entry = unsafe { clack_host::entry::PluginEntry::load(bundle) }
        .map_err(|error| format!("{}: {error}", bundle.display()))?;
    let factory = entry
        .get_plugin_factory()
        .ok_or_else(|| format!("{}: the bundle has no plugin factory", bundle.display()))?;
    let mut lines = String::new();
    for descriptor in factory.plugin_descriptors() {
        let text = |value: Option<&std::ffi::CStr>| {
            value.map_or(String::new(), |value| value.to_string_lossy().into_owned())
        };
        let Some(id) = descriptor.id() else {
            continue;
        };
        let plugin = ScannedPlugin {
            id: text(Some(id)),
            name: text(descriptor.name()),
            vendor: text(descriptor.vendor()),
            version: text(descriptor.version()),
            features: descriptor
                .features()
                .map(|feature| feature.to_string_lossy().into_owned())
                .collect(),
            path: PathBuf::new(),
        };
        let line = serde_json::to_string(&plugin)
            .map_err(|error| format!("{}: {error}", bundle.display()))?;
        lines.push_str(MARK);
        lines.push_str(&line);
        lines.push('\n');
    }
    Ok(lines)
}
