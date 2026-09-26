//! The scan: what plugins this machine has, found in another process.
//!
//! Loading a plugin means running its code. A plugin that crashes while it is being looked at
//! must not take the application down, so the scan runs in a child process, one child per
//! bundle: a crash costs one bundle and is reported. The child is this program with
//! [`SCAN_ARGUMENT`], the format and the bundle; tests use a small program of their own.
//!
//! Every child has a deadline, and it covers the whole bundle: the child, and the threads that
//! read what it printed. A plugin that hangs while it is listed, which licensed ones do when
//! they cannot reach their server, would otherwise hold the project open for ever, and so would
//! one that leaves a helper process behind holding the pipe its output went into.
//!
//! The format of a bundle is its file extension, `.clap` or `.vst3`, so one list of folders
//! covers both and a test folder can hold one of each.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::PluginFormat;

/// The argument that makes a program list one bundle. The format and the bundle path follow it.
/// The runtime passes it to its own executable.
pub const SCAN_ARGUMENT: &str = "--scan-plugin";

/// Every line the child means is marked with this. Loading a plugin runs its code, and a real
/// one prints to standard output while it initializes. Without the mark its logging would be
/// read as a plugin, or would make the whole bundle unreadable.
const MARK: &str = "sound-tools-plugin ";

/// How long one bundle may take. A working bundle costs milliseconds; this is what a plugin
/// that never answers costs, once. See README.md for the measured numbers.
pub const SCAN_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the child is looked at while the deadline runs. Short enough that a normal scan
/// pays nothing, long enough that waiting costs no thread.
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// How long the threads that read the child's pipes may still take once the child has ended,
/// inside the deadline of the bundle.
///
/// Everything the child printed is in the pipe by the time it exits, so a reader normally ends
/// in microseconds. One that is still waiting is waiting for a helper process that inherited
/// the pipe and outlived its parent, which licensed plugins leave behind: that pipe may not end
/// for the rest of the session, and what it would still carry is not this bundle's.
const READER_GRACE: Duration = Duration::from_millis(250);

/// One plugin a bundle holds. A bundle can hold several.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScannedPlugin {
    pub format: PluginFormat,
    /// The plugin's own id, what a record names. For CLAP the id its maker chose, for VST 3
    /// the class id as thirty-two hex digits.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// What the plugin says it is: CLAP features such as `instrument`, or VST 3 subcategories
    /// such as `Instrument` and `Synth`.
    pub features: Vec<String>,
    /// The bundle this plugin came from. The child does not know it; the parent fills it in.
    #[serde(default)]
    pub path: PathBuf,
}

impl ScannedPlugin {
    /// Whether the plugin plays notes. Only these fit the `instrument` child of a track.
    /// A plugin that says nothing about itself is not offered as one.
    ///
    /// CLAP writes `instrument` and VST 3 writes `Instrument`, so the word is what counts and
    /// not its case.
    pub fn is_instrument(&self) -> bool {
        self.features
            .iter()
            .any(|feature| feature.eq_ignore_ascii_case("instrument"))
    }

    /// Whether the plugin says it takes audio in and makes audio out of it. Only these are
    /// offered for an effect slot of a rack.
    ///
    /// CLAP writes `audio-effect` and VST 3 writes `Fx`, so both words count. A plugin that
    /// says both this and `instrument` is offered in both places; nothing here can tell
    /// whether either is true.
    pub fn is_effect(&self) -> bool {
        self.features.iter().any(|feature| {
            feature.eq_ignore_ascii_case("audio-effect") || feature.eq_ignore_ascii_case("fx")
        })
    }

    /// How an offer of this plugin is told from every other in a picker.
    pub fn offer_key(&self) -> String {
        crate::PluginRecord::offer_key(self.format, &self.id)
    }
}

/// A bundle that could not be scanned, and why. It is reported, and the scan goes on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanFailure {
    pub path: PathBuf,
    pub message: String,
}

/// What this machine has. It grows while a scan runs: a reader sees the bundles that are done.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    pub plugins: Vec<ScannedPlugin>,
    pub failures: Vec<ScanFailure>,
    /// Bundles looked at, including the ones that failed.
    pub bundles: usize,
    /// Whether every bundle has been looked at. While this is false the picker says a scan is
    /// running, and a plugin that is not in `plugins` yet may still turn up.
    pub finished: bool,
}

impl Scan {
    pub fn find(&self, format: PluginFormat, plugin_id: &str) -> Option<&ScannedPlugin> {
        self.plugins
            .iter()
            .find(|plugin| plugin.format == format && plugin.id == plugin_id)
    }
}

/// One bundle to look at: where it is and what format it is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bundle {
    pub path: PathBuf,
    pub format: PluginFormat,
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
    /// A program that takes the format and the bundle path as its last two arguments.
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
    fn scan(&self, bundle: &Bundle) -> Result<Vec<ScannedPlugin>, String> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.arguments)
            .arg(bundle.format.as_str())
            .arg(&bundle.path);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        for (name, value) in &self.environment {
            command.env(name, value);
        }
        // Blocking here is the point: this runs on the thread that scans, never on the one that
        // draws. `output` would wait for ever, and a deadline needs a handle to kill.
        #[allow(clippy::disallowed_methods)]
        let mut child = command
            .spawn()
            .map_err(|error| format!("the scanner did not start: {error}"))?;
        // Both pipes are read while the child runs and not after it ends. A plugin that prints
        // more than a pipe holds while it loads, which real ones do, would otherwise block on
        // its own write and be killed at the deadline as if it had hung.
        let output = drain(child.stdout.take());
        let errors = drain(child.stderr.take());
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
        let status = child
            .wait()
            .map_err(|error| format!("the scanner could not be read: {error}"))?;
        // The readers get what is left of the bundle's deadline, and no more than the grace: a
        // pipe that a descendant of the child holds open would otherwise hold the scan.
        let until = deadline.min(Instant::now() + READER_GRACE);
        let text = read(output, until);
        if !status.success() {
            let errors = read(errors, until);
            let message = errors.trim();
            let reason = if message.is_empty() {
                // A crash gives no message. The exit status says how it ended.
                format!("the scanner ended with {status}")
            } else {
                message.to_string()
            };
            return Err(reason);
        }
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

/// One pipe of the child, being read on a thread of its own.
struct Draining {
    reader: JoinHandle<()>,
    /// What has been read so far. The thread appends to it as the child prints, so whoever
    /// gives up waiting still has everything the child wrote.
    bytes: Arc<Mutex<Vec<u8>>>,
}

/// Reads a pipe of the child on a thread of its own, so the child never blocks on a full one.
///
/// Every chunk goes into the shared buffer as it arrives, and not at the end. A pipe ends when
/// the last writer lets go of it, and a plugin may leave a helper process behind that inherited
/// it: then this thread waits for that helper and not for the plugin. The bundle is still read,
/// because what the child printed is already here.
fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> Option<Draining> {
    let mut pipe = pipe?;
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let filling = bytes.clone();
    let reader = std::thread::Builder::new()
        .name("plugin-scan-output".to_string())
        .spawn(move || {
            let mut chunk = [0_u8; 8192];
            // A pipe that cannot be read leaves what was read before the error.
            while let Ok(read) = pipe.read(&mut chunk) {
                if read == 0 {
                    return;
                }
                let Ok(mut held) = filling.lock() else {
                    return;
                };
                held.extend_from_slice(&chunk[..read]);
            }
        })
        .ok()?;
    Some(Draining { reader, bytes })
}

/// What such a thread has read, waiting for it no longer than the deadline of the bundle.
///
/// A thread that is still waiting for a pipe a descendant of the child holds open is left to
/// it: it ends when that descendant does, it holds nothing but its own pipe, and the scan goes
/// on. That is what keeps a licensing helper from stopping a scan for ever.
fn read(draining: Option<Draining>, deadline: Instant) -> String {
    let Some(draining) = draining else {
        return String::new();
    };
    while !draining.reader.is_finished() && Instant::now() < deadline {
        std::thread::sleep(POLL_INTERVAL);
    }
    let bytes = match draining.bytes.lock() {
        Ok(bytes) => bytes.clone(),
        // A thread that panicked, which nothing of ours does while it holds this.
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

/// How deep to look inside a search folder. Plugins usually sit one folder per vendor deep.
const MAX_DEPTH: usize = 4;

/// Every folder this machine keeps plugins in, for every format this build hosts.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = crate::clap::default_search_paths();
    paths.extend(crate::vst3::default_search_paths());
    paths
}

/// Every plugin bundle under `folders`, in a fixed order so a scan is repeatable.
pub fn bundles(folders: &[PathBuf]) -> Vec<Bundle> {
    let mut found = Vec::new();
    for folder in folders {
        collect(folder, 0, &mut found);
    }
    found.sort();
    found.dedup();
    found
}

fn collect(folder: &Path, depth: usize, found: &mut Vec<Bundle>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        // A search folder that is not there is normal: this machine has no plugins of that kind.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let format = path.extension().and_then(PluginFormat::of_extension);
        if let Some(format) = format {
            // A bundle is a folder on macOS for VST 3 and either for CLAP. Either way it is one
            // entry and nothing inside it is another plugin.
            found.push(Bundle { path, format });
        } else if path.is_dir() {
            collect(&path, depth + 1, found);
        }
    }
}

/// The child side: loads one bundle and prints one line of JSON per plugin in it.
///
/// Everything that can go wrong here is the plugin's. The caller is a process of its own, so a
/// crash inside the plugin's own code costs this process and nothing else.
pub fn scan_one_bundle(format: PluginFormat, bundle: &Path) -> Result<String, String> {
    let plugins = match format {
        PluginFormat::Clap => crate::clap::scan_bundle(bundle)?,
        PluginFormat::Vst3 => crate::vst3::scan_bundle(bundle)?,
    };
    let mut lines = String::new();
    for plugin in plugins {
        let line = serde_json::to_string(&plugin)
            .map_err(|error| format!("{}: {error}", bundle.display()))?;
        lines.push_str(MARK);
        lines.push_str(&line);
        lines.push('\n');
    }
    Ok(lines)
}

/// What this machine's plugins were last time, so that a start pays for nothing it has already
/// paid for.
///
/// It belongs to the machine and not to a project: two projects on one machine have the same
/// plugins, and a project folder in git must not carry a list of what one laptop happens to
/// have. A bundle is remembered with a [`Stamp`] of its binary folder, so a plugin that was
/// installed, updated or replaced is looked at again and nothing else is. A bundle that crashed
/// or hung is remembered as such and is not tried again on every start; `runtime --plugins`
/// looks at everything again and writes the result, which is how one that was fixed comes back.
///
/// A cache with no file keeps the last scan in memory instead, shared by its copies. So a scan
/// again while the app runs, which is how a plugin installed meanwhile is found, looks only at
/// what changed, also in a test, which never touches the file of this machine.
#[derive(Clone, Debug)]
pub struct ScanCache {
    path: Option<PathBuf>,
    /// Whether what is remembered may be used. `false` looks at every bundle again.
    reuse: bool,
    /// The last scan, when there is no file to keep it in.
    kept: Arc<Mutex<Vec<CachedBundle>>>,
}

/// Where the cache of this machine is kept, unless the environment says otherwise. The variable
/// is what a test sets so that it never touches the machine's own file.
const CACHE_VARIABLE: &str = "SOUND_TOOLS_PLUGIN_CACHE";

impl ScanCache {
    /// The cache of this machine: `~/Library/Caches/sound-tools/plugins.json`.
    pub fn of_this_machine() -> Self {
        if let Some(named) = std::env::var_os(CACHE_VARIABLE) {
            return Self::at(PathBuf::from(named));
        }
        let path = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Caches/sound-tools/plugins.json"));
        Self::kept_at(path)
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self::kept_at(Some(path.into()))
    }

    /// Nothing is kept on disk, so no test can be changed by what this machine has. Every test
    /// uses this. The scans of one host still remember each other, in memory.
    pub fn none() -> Self {
        Self::kept_at(None)
    }

    fn kept_at(path: Option<PathBuf>) -> Self {
        Self {
            path,
            reuse: true,
            kept: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Looks at every bundle again and writes what it finds. This is what `runtime --plugins`
    /// does, so a plugin that hung once can be tried again.
    pub fn refreshing(mut self) -> Self {
        self.reuse = false;
        self
    }

    fn read(&self) -> Vec<CachedBundle> {
        if !self.reuse {
            return Vec::new();
        }
        let Some(path) = &self.path else {
            return self
                .kept
                .lock()
                .map(|kept| kept.clone())
                .unwrap_or_default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        // A cache that cannot be read is no error: everything is scanned again and the file is
        // written over. A file of an older build is such a file.
        serde_json::from_str(&text).unwrap_or_default()
    }

    fn write(&self, bundles: &[CachedBundle]) {
        let Some(path) = &self.path else {
            if let Ok(mut kept) = self.kept.lock() {
                *kept = bundles.to_vec();
            }
            return;
        };
        let Ok(text) = serde_json::to_string_pretty(bundles) else {
            return;
        };
        // A cache that cannot be written costs the next start a scan and nothing else.
        if let Err(error) = write_whole(path, text.as_bytes()) {
            eprintln!(
                "the plugin cache {} was not written: {error}",
                path.display()
            );
        }
    }
}

/// Writes `bytes` as the whole of `path`, or leaves what is there.
///
/// Two runtimes on one machine may finish a scan at the same moment, and each writes the cache.
/// A plain write truncates the file and then fills it, so the two could interleave, and a
/// reader in between reads half a file. So each writer writes a file of its own, named after
/// its process and a count, and renames it over the cache, which the system does in one step: a
/// reader sees one whole file or the other, and the last rename wins. Both are what one scan
/// found, so either is right.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITES: AtomicU64 = AtomicU64::new(0);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    let count = WRITES.fetch_add(1, Ordering::Relaxed);
    name.push(format!(".{}-{count}.tmp", std::process::id()));
    let temporary = path.with_file_name(name);
    let written =
        std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        // Nothing else will ever remove a file only this writer knows the name of.
        let _gone = std::fs::remove_file(&temporary);
    }
    written
}

/// One bundle as the cache remembers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CachedBundle {
    path: PathBuf,
    format: PluginFormat,
    stamp: Stamp,
    #[serde(default)]
    plugins: Vec<ScannedPlugin>,
    /// Why this bundle has no plugins: it crashed, hung, or is not a plugin at all.
    #[serde(default)]
    failure: Option<String>,
}

/// What a bundle would load on this host, as far as a scan needs to know: a bundle with the
/// same stamp as last time is not looked at again.
///
/// Which of the files in the binary folder is the executable is up to the bundle's
/// `Info.plist`, so the stamp does not pick one: it is the latest status change (`ctime`) of
/// the binary folder, of every file directly in it, and of the `Info.plist`. The system sets a
/// file's status change time on every write, rename, copy or replace, and nothing can set it
/// back, so a binary that was changed or replaced moves it, even one of the same size whose
/// modified time an installer kept. The folder's own moves when a file is added or taken away.
/// A plugin that is a single file, as a CLAP one may be, is stamped by that file.
///
/// And the architecture of the host: a universal binary is another plugin to an arm64 host
/// than to one under Rosetta, and one that is x86_64 only fails on the first and not the second.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    architecture: String,
    /// Nanoseconds since the epoch.
    changed: i128,
}

/// The stamp of `bundle` on this host. `None` says the bundle is gone.
fn stamp(bundle: &Path) -> Option<Stamp> {
    use std::os::unix::fs::MetadataExt as _;
    let changed = |path: &Path| {
        let metadata = std::fs::metadata(path).ok()?;
        Some(i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()))
    };
    let binaries = bundle.join("Contents/MacOS");
    let mut latest = match std::fs::read_dir(&binaries) {
        Ok(entries) => {
            let mut files: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
            files.push(binaries);
            files.push(bundle.join("Contents/Info.plist"));
            files.iter().filter_map(|file| changed(file)).max()
        }
        Err(_) => None,
    };
    if latest.is_none() {
        latest = Some(changed(bundle)?);
    }
    Some(Stamp {
        architecture: std::env::consts::ARCH.to_string(),
        changed: latest?,
    })
}

/// Scans every bundle under `folders`, using `cache` for the ones that have not changed, and
/// tells `progress` after each one so a caller can show what is known while the rest runs.
///
/// `stop` ends the scan between bundles, for a host that goes while its scan runs.
pub fn scan_folders(
    folders: &[PathBuf],
    command: &ScanCommand,
    cache: &ScanCache,
    stop: &std::sync::atomic::AtomicBool,
    mut progress: impl FnMut(&Scan),
) -> Scan {
    use std::sync::atomic::Ordering;

    let bundles = bundles(folders);
    let remembered = cache.read();
    let mut scan = Scan {
        bundles: bundles.len(),
        ..Scan::default()
    };
    let mut to_remember = Vec::with_capacity(bundles.len());
    for bundle in bundles {
        if stop.load(Ordering::Acquire) {
            return scan;
        }
        let stamp = stamp(&bundle.path);
        let known = stamp.as_ref().and_then(|stamp| {
            remembered.iter().find(|entry| {
                entry.path == bundle.path && entry.format == bundle.format && entry.stamp == *stamp
            })
        });
        let found = match known {
            Some(entry) => match &entry.failure {
                Some(message) => Err(message.clone()),
                None => Ok(entry.plugins.clone()),
            },
            None => command.scan(&bundle),
        };
        if let Some(stamp) = stamp {
            to_remember.push(CachedBundle {
                path: bundle.path.clone(),
                format: bundle.format,
                stamp,
                plugins: found.clone().unwrap_or_default(),
                failure: found.as_ref().err().cloned(),
            });
        }
        match found {
            Ok(plugins) => scan
                .plugins
                .extend(plugins.into_iter().map(|plugin| ScannedPlugin {
                    path: bundle.path.clone(),
                    ..plugin
                })),
            Err(message) => scan.failures.push(ScanFailure {
                path: bundle.path,
                message,
            }),
        }
        progress(&scan);
    }
    scan.plugins
        .sort_by(|left, right| (left.format, &left.id).cmp(&(right.format, &right.id)));
    scan.finished = true;
    // A scan that found what was remembered writes nothing, so looking again while the app
    // runs costs no write.
    if to_remember != remembered {
        cache.write(&to_remember);
    }
    progress(&scan);
    scan
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    /// A bundle as macOS lays one out: the binary the `Info.plist` names, a helper next to it,
    /// and resources. Nothing in it is a plugin; these tests are about the files.
    fn bundle_in(folder: &Path) -> PathBuf {
        let bundle = folder.join("piano.vst3");
        let binaries = bundle.join("Contents/MacOS");
        std::fs::create_dir_all(&binaries).unwrap();
        std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"), "<plist/>").unwrap();
        std::fs::write(binaries.join("a-helper"), "helper").unwrap();
        std::fs::write(binaries.join("piano"), "the first build").unwrap();
        std::fs::write(bundle.join("Contents/Resources/sound"), "samples").unwrap();
        bundle
    }

    /// An installer that replaces a binary may keep its modified time, and a new build may be
    /// the same size. The binary is the one the `Info.plist` names, which need not be the
    /// first file in its folder. Either way the bundle must be looked at again. And a file
    /// that is not in the binary folder, such as the plugin's samples, changes nothing.
    #[test]
    fn a_binary_replaced_with_the_same_size_and_time_changes_the_stamp() {
        let folder = tempfile::tempdir().unwrap();
        let bundle = bundle_in(folder.path());
        let before = stamp(&bundle).unwrap();

        std::fs::write(bundle.join("Contents/Resources/sound"), "other samples").unwrap();
        assert_eq!(stamp(&bundle), Some(before.clone()));

        let binary = bundle.join("Contents/MacOS/piano");
        let modified = std::fs::metadata(&binary).unwrap().modified().unwrap();
        std::fs::write(&binary, "the next build!").unwrap();
        let file = std::fs::File::options().write(true).open(&binary).unwrap();
        file.set_modified(modified).unwrap();
        let metadata = std::fs::metadata(&binary).unwrap();
        assert_eq!(metadata.len(), "the first build".len() as u64);
        assert_eq!(metadata.modified().unwrap(), modified);
        assert_ne!(stamp(&bundle), Some(before));
    }

    /// A bundle remembered by a host of another architecture: a universal binary is another
    /// plugin under Rosetta, and an x86_64-only one loads there and not here.
    #[test]
    fn a_bundle_scanned_by_a_host_of_another_architecture_is_looked_at_again() {
        let folder = tempfile::tempdir().unwrap();
        let bundle = bundle_in(folder.path());
        let cache = ScanCache::at(folder.path().join("plugins.json"));
        // A scanner that cannot start: a bundle it is asked about is a failure.
        let broken = ScanCommand::new("/definitely/not/a/program", []);
        let remembered = |architecture: &str| {
            let here = stamp(&bundle).unwrap();
            vec![CachedBundle {
                path: bundle.clone(),
                format: PluginFormat::Vst3,
                stamp: Stamp {
                    architecture: architecture.to_string(),
                    ..here
                },
                plugins: vec![ScannedPlugin {
                    format: PluginFormat::Vst3,
                    id: "534F554E44544F4F4C53544553545430".to_string(),
                    name: "Piano".to_string(),
                    vendor: String::new(),
                    version: String::new(),
                    features: vec!["Instrument".to_string()],
                    path: PathBuf::new(),
                }],
                failure: None,
            }]
        };
        let scan = |cache: &ScanCache| {
            scan_folders(
                &[folder.path().to_path_buf()],
                &broken,
                cache,
                &AtomicBool::new(false),
                |_| {},
            )
        };

        let other = match std::env::consts::ARCH {
            "aarch64" => "x86_64",
            _ => "aarch64",
        };
        cache.write(&remembered(other));
        let found = scan(&cache);
        assert_eq!(found.plugins, []);
        assert_eq!(
            found.failures.len(),
            1,
            "the bundle was not looked at again"
        );

        cache.write(&remembered(std::env::consts::ARCH));
        let found = scan(&cache);
        assert_eq!(found.failures, []);
        assert_eq!(found.plugins.len(), 1);
    }

    /// Two runtimes finish a scan at the same moment and each writes the cache, while a third
    /// reads it. The reader never sees half a file.
    #[test]
    fn two_writers_at_once_never_leave_half_a_cache() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("plugins.json");
        let entry = CachedBundle {
            path: folder.path().join("piano.vst3"),
            format: PluginFormat::Vst3,
            stamp: Stamp {
                architecture: std::env::consts::ARCH.to_string(),
                changed: 1,
            },
            plugins: Vec::new(),
            failure: None,
        };
        let writers: Vec<_> = [1, 400]
            .into_iter()
            .map(|count| {
                let cache = ScanCache::at(&path);
                let bundles = vec![entry.clone(); count];
                std::thread::spawn(move || {
                    for _ in 0..300 {
                        cache.write(&bundles);
                    }
                })
            })
            .collect();
        let (mut reads, mut halves) = (0, 0);
        while writers.iter().any(|writer| !writer.is_finished()) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            reads += 1;
            if serde_json::from_str::<Vec<CachedBundle>>(&text).is_err() {
                halves += 1;
            }
        }
        for writer in writers {
            writer.join().unwrap();
        }
        assert!(reads > 0);
        assert_eq!(halves, 0, "{halves} of {reads} reads found half a cache");
        // And nothing is left of the writers but the cache.
        let left: Vec<_> = std::fs::read_dir(folder.path())
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }
}
