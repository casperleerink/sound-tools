//! The scan: what plugins this machine has, found in child processes.
//!
//! Loading a plugin runs its code, so each bundle is listed by a child process of its own: a
//! crash costs one bundle and is reported. The child is this program with [`SCAN_ARGUMENT`].
//!
//! Every child has a deadline that covers the whole bundle, the child and the threads that read
//! its output. Licensed plugins hang while they are listed when they cannot reach their server,
//! and some leave a helper process behind that holds the output pipe open.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::PluginFormat;

/// The argument that makes a program list one bundle. The format and the bundle path follow it.
pub const SCAN_ARGUMENT: &str = "--scan-plugin";

/// Marks every line the child means. A real plugin prints to standard output while it
/// initializes, which would otherwise be read as a plugin or make the bundle unreadable.
const MARK: &str = "sound-tools-plugin ";

/// How long one bundle may take. Most take milliseconds, but Kontakt on Windows takes more than
/// ten seconds on its first load.
pub(crate) const SCAN_TIMEOUT: Duration = Duration::from_secs(60);

/// How many bundles are looked at at once, so one that hangs holds back only its own child.
const SCANNERS: usize = 4;

const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// How long the threads that read the child's pipes may still take once the child has ended.
/// Everything it printed is in the pipe by then, so a reader still waiting is waiting for a
/// helper process that inherited the pipe, which may hold it for the rest of the session.
const READER_GRACE: Duration = Duration::from_millis(250);

/// One plugin a bundle holds. A bundle can hold several.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScannedPlugin {
    pub format: PluginFormat,
    /// For CLAP the id its maker chose, for VST 3 the class id as thirty-two hex digits.
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// What the plugin says it is: CLAP features such as `instrument`, or VST 3 subcategories
    /// such as `Instrument` and `Synth`.
    pub features: Vec<String>,
    /// The bundle this plugin came from. The child does not know it; the parent fills it in.
    #[serde(default)]
    pub path: PathBuf,
}

impl ScannedPlugin {
    /// The quiet line under its name, in a picker and on its card: `CLAP · <maker>`.
    pub fn detail(&self) -> String {
        match self.vendor.is_empty() {
            true => self.format.name().to_string(),
            false => format!("{} · {}", self.format.name(), self.vendor),
        }
    }

    /// Whether the plugin says it plays notes, which puts it in the instrument picker. CLAP
    /// writes `instrument` and VST 3 `Instrument`.
    pub fn is_instrument(&self) -> bool {
        self.features
            .iter()
            .any(|feature| feature.eq_ignore_ascii_case("instrument"))
    }

    /// Whether the plugin says it is an effect, which puts it in the effect picker. CLAP writes
    /// `audio-effect` and VST 3 `Fx`.
    pub fn is_effect(&self) -> bool {
        self.features.iter().any(|feature| {
            feature.eq_ignore_ascii_case("audio-effect") || feature.eq_ignore_ascii_case("fx")
        })
    }

    pub fn offer_key(&self) -> String {
        crate::PluginRecord::offer_key(self.format, &self.id)
    }
}

/// A bundle that could not be scanned, and why.
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
    /// While false, a plugin that is not in `plugins` yet may still turn up.
    pub finished: bool,
    /// Why the cache of this machine could not be written or tidied. It costs the next start a
    /// scan and nothing else.
    pub cache_error: Option<String>,
}

impl Scan {
    pub fn find(&self, format: PluginFormat, plugin_id: &str) -> Option<&ScannedPlugin> {
        self.plugins
            .iter()
            .find(|plugin| plugin.format == format && plugin.id == plugin_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Bundle {
    pub path: PathBuf,
    pub format: PluginFormat,
}

/// Why one bundle was not scanned.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ScanError {
    /// The child crashed, hung or printed something unreadable. That is the bundle's own doing,
    /// so it is remembered and not tried again at every start.
    Bundle(String),
    /// This process could not start, watch or read the child. That says nothing about the
    /// bundle, so the next scan tries it again.
    Host(String),
}

/// How to start the child that scans one bundle.
#[derive(Clone, Debug)]
pub struct ScanCommand {
    program: PathBuf,
    arguments: Vec<OsString>,
    /// For the child only, so a test can change what the child does without changing its own
    /// environment.
    environment: Vec<(OsString, OsString)>,
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

    /// This program with [`SCAN_ARGUMENT`], which it must handle with [`scan_one_bundle`].
    pub fn this_program() -> std::io::Result<Self> {
        let program = std::env::current_exe()?;
        Ok(Self::new(program, [OsString::from(SCAN_ARGUMENT)]))
    }

    pub fn with_environment(mut self, name: &str, value: &str) -> Self {
        self.environment
            .push((OsString::from(name), OsString::from(value)));
        self
    }

    /// So a test does not wait [`SCAN_TIMEOUT`].
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn scan(&self, bundle: &Bundle) -> Result<Vec<ScannedPlugin>, ScanError> {
        let mut command = sound_core::process::background_command(&self.program);
        command
            .args(&self.arguments)
            .arg(bundle.format.as_str())
            .arg(&bundle.path);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        for (name, value) in &self.environment {
            command.env(name, value);
        }
        // This runs on the thread that scans, never on the one that draws. Not `output`, which
        // would wait for ever: a deadline needs a handle to kill.
        #[allow(clippy::disallowed_methods)]
        let mut child = command
            .spawn()
            .map_err(|error| ScanError::Host(format!("the scanner did not start: {error}")))?;
        // Read while the child runs: a plugin that prints more than a pipe holds, which real
        // ones do, would otherwise block on its own write and look hung.
        let (output, errors) = match (drain(child.stdout.take()), drain(child.stderr.take())) {
            (Ok(output), Ok(errors)) => (output, errors),
            (Err(error), _) | (_, Err(error)) => {
                let reason = format!("the scanner's output could not be read: {error}");
                return Err(ScanError::Host(stop(&mut child, reason)));
            }
        };
        let deadline = Instant::now() + self.timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(error) => {
                    let reason = format!("the scanner could not be waited for: {error}");
                    return Err(ScanError::Host(stop(&mut child, reason)));
                }
            }
            if Instant::now() >= deadline {
                let reason = format!(
                    "the scanner did not finish within {:?} and was stopped. The plugin hangs while it is listed",
                    self.timeout
                );
                return Err(ScanError::Bundle(stop(&mut child, reason)));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        let status = child
            .wait()
            .map_err(|error| ScanError::Host(format!("the scanner could not be read: {error}")))?;
        let until = deadline.min(Instant::now() + READER_GRACE);
        let text = read(output, until);
        if !status.success() {
            let errors = read(errors, until);
            // The runtime says `Error: <bundle>: <why>`, and whoever reads this knows the bundle.
            let named = format!("Error: {}: ", bundle.path.display());
            let message = errors.trim();
            let message = message.strip_prefix(&named).unwrap_or(message);
            let reason = if message.is_empty() {
                // A crash gives no message. The exit status says how it ended.
                format!("the scanner ended with {status}")
            } else {
                message.to_string()
            };
            return Err(ScanError::Bundle(reason));
        }
        let mut plugins = Vec::new();
        for line in text.lines().filter_map(|line| line.strip_prefix(MARK)) {
            let plugin = serde_json::from_str(line).map_err(|error| {
                ScanError::Bundle(format!("the scanner printed something unreadable: {error}"))
            })?;
            plugins.push(plugin);
        }
        Ok(plugins)
    }
}

/// Kills the child and waits for it. Gives `reason`, with why the child could not be stopped
/// when it could not.
fn stop(child: &mut Child, reason: String) -> String {
    match child.kill().and_then(|()| child.wait()) {
        Ok(_) => reason,
        Err(error) => format!("{reason}. The scanner could not be stopped: {error}"),
    }
}

/// One pipe of the child, being read on a thread of its own.
struct Draining {
    reader: JoinHandle<()>,
    /// Filled as the child prints, so whoever gives up waiting for a pipe a helper process
    /// holds open still has everything the child wrote.
    bytes: Arc<Mutex<Vec<u8>>>,
}

fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> std::io::Result<Option<Draining>> {
    let Some(mut pipe) = pipe else {
        return Ok(None);
    };
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
        })?;
    Ok(Some(Draining { reader, bytes }))
}

/// What a reader has read by `deadline`. A reader still waiting is left behind: it ends when
/// the helper holding its pipe does, and holds nothing else.
fn read(draining: Option<Draining>, deadline: Instant) -> String {
    let Some(draining) = draining else {
        return String::new();
    };
    while !draining.reader.is_finished() && Instant::now() < deadline {
        std::thread::sleep(POLL_INTERVAL);
    }
    let bytes = match draining.bytes.lock() {
        Ok(bytes) => bytes.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

/// How deep to look inside a search folder. Plugins usually sit one folder per vendor deep.
const MAX_DEPTH: usize = 4;

/// Every folder this machine keeps plugins in, as the specifications list them: `entry.h` for
/// CLAP, Steinberg's for VST 3, and each format's environment variable.
pub fn default_search_paths() -> Vec<PathBuf> {
    let home = |inside: &str| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(inside));
    let under = |variable: &str, inside: &str| {
        std::env::var_os(variable).map(|folder| PathBuf::from(folder).join(inside))
    };
    let fixed = |path: &str| Some(PathBuf::from(path));
    let (clap, vst3) = if cfg!(target_os = "macos") {
        (
            vec![
                home("Library/Audio/Plug-Ins/CLAP"),
                fixed("/Library/Audio/Plug-Ins/CLAP"),
            ],
            vec![
                home("Library/Audio/Plug-Ins/VST3"),
                fixed("/Library/Audio/Plug-Ins/VST3"),
                fixed("/Network/Library/Audio/Plug-Ins/VST3"),
            ],
        )
    } else if cfg!(target_os = "windows") {
        (
            vec![
                under("COMMONPROGRAMFILES", "CLAP"),
                under("LOCALAPPDATA", r"Programs\Common\CLAP"),
            ],
            vec![
                under("COMMONPROGRAMFILES", "VST3"),
                under("LOCALAPPDATA", r"Programs\Common\VST3"),
            ],
        )
    } else {
        (
            vec![home(".clap"), fixed("/usr/lib/clap")],
            vec![
                home(".vst3"),
                fixed("/usr/lib/vst3"),
                fixed("/usr/local/lib/vst3"),
            ],
        )
    };
    let named = |variable: &str| {
        std::env::var_os(variable)
            .map(|extra| std::env::split_paths(&extra).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut paths: Vec<PathBuf> = clap.into_iter().flatten().collect();
    paths.extend(named("CLAP_PATH"));
    paths.extend(vst3.into_iter().flatten());
    paths.extend(named("VST3_PATH"));
    paths
}

/// Every plugin bundle under `folders`, in a fixed order so a scan is repeatable.
pub(crate) fn bundles(folders: &[PathBuf]) -> Vec<Bundle> {
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
        // Normal: this machine has no plugins of that kind.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let format = path.extension().and_then(PluginFormat::of_extension);
        if let Some(format) = format {
            // A folder or a single file, either way one bundle: nothing inside is another.
            found.push(Bundle { path, format });
        } else if path.is_dir() {
            collect(&path, depth + 1, found);
        }
    }
}

/// The child side: loads one bundle and gives one marked line of JSON per plugin in it.
pub fn scan_one_bundle(format: PluginFormat, bundle: &Path) -> Result<String, String> {
    // A plugin that crashes here would otherwise put up the system's crash dialog, and the
    // child would wait for the composer to close it until the deadline killed it as hung.
    #[cfg(target_os = "windows")]
    {
        const SEM_FAILCRITICALERRORS: u32 = 0x0001;
        const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
        const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn SetErrorMode(mode: u32) -> u32;
        }
        // SAFETY: it takes flags and only changes how this process reports its own errors.
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX)
        };
    }
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
/// It belongs to the machine and not to a project: a project folder in git must not carry a
/// list of what one laptop happens to have. A bundle is remembered with a [`Stamp`], so only a
/// plugin that was installed, updated or replaced is looked at again. A bundle that crashed or
/// hung is not tried again on every start, only by `Scan again` in a picker
/// ([`Self::retrying_failures`]) and `runtime --plugins` ([`Self::refreshing`]).
///
/// A cache with no file keeps the last scan in memory, shared by its clones, so a scan again
/// while the app runs still looks only at what changed.
#[derive(Clone, Debug)]
pub struct ScanCache {
    path: Option<PathBuf>,
    reuse: Reuse,
    /// The last scan, when there is no file to keep it in.
    kept: Arc<Mutex<Vec<CachedBundle>>>,
    /// The plugin windows of every project, when there is no file to keep them in.
    kept_placements: crate::placements::Kept,
}

/// What of the last scan a scan may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reuse {
    Everything,
    /// The bundles that were read, so the ones that failed are tried again.
    Plugins,
    Nothing,
}

/// A file to keep the cache in instead of this machine's own.
const CACHE_VARIABLE: &str = "SOUND_TOOLS_PLUGIN_CACHE";

impl ScanCache {
    /// `~/Library/Caches/sound-tools/plugins.json` on macOS,
    /// `%LOCALAPPDATA%\Sound Tools\Cache\plugins.json` on Windows, and
    /// `~/.cache/sound-tools/plugins.json` on Linux, or under `XDG_CACHE_HOME` when it is set.
    pub fn of_this_machine() -> Self {
        if let Some(named) = std::env::var_os(CACHE_VARIABLE) {
            return Self::at(PathBuf::from(named));
        }
        let home = || std::env::var_os("HOME").map(PathBuf::from);
        let folder = if cfg!(target_os = "macos") {
            home().map(|home| home.join("Library/Caches/sound-tools"))
        } else if cfg!(target_os = "windows") {
            std::env::var_os("LOCALAPPDATA")
                .map(|local| PathBuf::from(local).join("Sound Tools").join("Cache"))
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .or_else(|| home().map(|home| home.join(".cache")))
                .map(|caches| caches.join("sound-tools"))
        };
        Self::kept_at(folder.map(|folder| folder.join("plugins.json")))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self::kept_at(Some(path.into()))
    }

    /// Nothing is kept on disk, so what this machine has changes nothing. The scans of one host
    /// still remember each other, in memory.
    pub fn none() -> Self {
        Self::kept_at(None)
    }

    fn kept_at(path: Option<PathBuf>) -> Self {
        Self {
            path,
            reuse: Reuse::Everything,
            kept: Arc::new(Mutex::new(Vec::new())),
            kept_placements: crate::placements::Kept::default(),
        }
    }

    /// Looks at every bundle again and writes what it finds.
    pub fn refreshing(mut self) -> Self {
        self.reuse = Reuse::Nothing;
        self
    }

    /// Looks again at the bundles that crashed or hung, and at nothing else that did not change.
    pub fn retrying_failures(mut self) -> Self {
        self.reuse = Reuse::Plugins;
        self
    }

    fn read(&self) -> Vec<CachedBundle> {
        let mut remembered = match (self.reuse, &self.path) {
            (Reuse::Nothing, _) => Vec::new(),
            (_, None) => self
                .kept
                .lock()
                .map(|kept| kept.clone())
                .unwrap_or_default(),
            // A cache that cannot be read, such as one of an older build, is no error:
            // everything is scanned again and the file is written over.
            (_, Some(path)) => std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default(),
        };
        if self.reuse == Reuse::Plugins {
            remembered.retain(|bundle: &CachedBundle| bundle.failure.is_none());
        }
        remembered
    }

    fn write(&self, bundles: &[CachedBundle]) -> Result<(), String> {
        let Some(path) = &self.path else {
            if let Ok(mut kept) = self.kept.lock() {
                *kept = bundles.to_vec();
            }
            return Ok(());
        };
        let failed = |error: String| {
            format!(
                "the plugin cache {} was not written: {error}",
                path.display()
            )
        };
        let text =
            serde_json::to_string_pretty(bundles).map_err(|error| failed(error.to_string()))?;
        write_whole(path, text.as_bytes()).map_err(failed)
    }

    fn tidy(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        remove_stale_writes(path).map_err(|errors| {
            format!("what an earlier write of the plugin cache left could not be removed: {errors}")
        })
    }

    /// Where this machine keeps the plugin windows of every project: the cache's folder, or
    /// memory for a cache with no file.
    pub(crate) fn placement_store(&self) -> crate::placements::PlacementStore {
        crate::placements::PlacementStore::new(
            self.path
                .as_ref()
                .map(|path| path.with_file_name(crate::placements::FILE)),
            self.kept_placements.clone(),
        )
    }
}

/// Removes the files of [`write_whole`] that a crash left behind before their rename. A file
/// younger than [`STALE_AFTER`] may be another runtime's write in progress, so it stays.
pub(crate) fn remove_stale_writes(path: &Path) -> Result<(), String> {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(());
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Ok(());
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let mut errors = Vec::new();
    for entry in entries.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if !file.starts_with(&prefix) || !file.ends_with(".tmp") {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok());
        if age.is_some_and(|age| age >= STALE_AFTER)
            && let Err(error) = std::fs::remove_file(entry.path())
        {
            errors.push(format!("{}: {error}", entry.path().display()));
        }
    }
    match errors.is_empty() {
        true => Ok(()),
        false => Err(errors.join(", ")),
    }
}

/// A write takes milliseconds.
const STALE_AFTER: Duration = Duration::from_secs(60);

/// Writes `bytes` as the whole of `path`, or leaves what is there.
///
/// Two runtimes on one machine may write the cache at the same moment, and a plain write lets a
/// reader see half a file. So each writer writes a file of its own and renames it over the
/// cache in one step: a reader sees one whole file or the other, and the last rename wins.
pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::sync::atomic::AtomicU64;
    static WRITES: AtomicU64 = AtomicU64::new(0);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    let count = WRITES.fetch_add(1, Ordering::Relaxed);
    name.push(format!(".{}-{count}.tmp", std::process::id()));
    let temporary = path.with_file_name(name);
    let written = std::fs::write(&temporary, bytes).and_then(|()| rename_over(&temporary, path));
    let Err(error) = written else {
        return Ok(());
    };
    // One that cannot be removed is left for `remove_stale_writes` at a later start.
    match std::fs::remove_file(&temporary) {
        Err(left) if left.kind() != std::io::ErrorKind::NotFound => Err(format!(
            "{error}, and {} was left behind: {left}",
            temporary.display()
        )),
        _ => Err(error.to_string()),
    }
}

/// Windows refuses to replace a file another process has open without delete sharing, which
/// another writer's rename and a virus scanner both do for a moment. So there a refusal is
/// tried again a few times before it counts.
fn rename_over(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut tries = 1;
    loop {
        match std::fs::rename(from, to) {
            Err(error)
                if cfg!(target_os = "windows")
                    && error.kind() == std::io::ErrorKind::PermissionDenied
                    && tries < RENAME_TRIES =>
            {
                tries += 1;
                std::thread::sleep(RENAME_PAUSE);
            }
            renamed => return renamed,
        }
    }
}

/// 50 ms in all, much longer than a rename takes.
const RENAME_TRIES: u32 = 10;
const RENAME_PAUSE: Duration = Duration::from_millis(5);

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

/// What a bundle would load on this host: a bundle with the same stamp as last time is not
/// looked at again.
///
/// The bundle's `Info.plist` picks the executable, so the stamp is the latest status change
/// time of the binary folder, of every file directly in it, and of the `Info.plist`. Nothing
/// can set that time back, so it catches a binary replaced with the same size and a kept
/// modified time, which the size, modified and creation times miss. A single-file plugin is
/// stamped by that file.
///
/// And the host's architecture: a universal binary is another plugin under Rosetta, and an
/// x86_64-only one fails on arm64 and not there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    architecture: String,
    /// Nanoseconds since the epoch of the system's file times.
    changed: i128,
}

/// `None` says the bundle is gone.
fn stamp(bundle: &Path) -> Option<Stamp> {
    let binaries = binary_folder(bundle);
    let mut latest = match std::fs::read_dir(&binaries) {
        Ok(entries) => {
            let mut files: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
            files.push(binaries);
            files.push(bundle.join("Contents").join("Info.plist"));
            files.iter().filter_map(|file| status_changed(file)).max()
        }
        Err(_) => None,
    };
    if latest.is_none() {
        latest = Some(status_changed(bundle)?);
    }
    Some(Stamp {
        architecture: std::env::consts::ARCH.to_string(),
        changed: latest?,
    })
}

/// `ctime`. A link that leads nowhere is stamped by the link itself, so it is remembered as a
/// failed bundle and not looked at again by every scan.
#[cfg(unix)]
fn status_changed(path: &Path) -> Option<i128> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::metadata(path)
        .or_else(|_| std::fs::symlink_metadata(path))
        .ok()?;
    Some(i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()))
}

/// The `ChangeTime` NTFS keeps, its `ctime`, which the standard library does not read. A link
/// that leads nowhere is stamped by the link itself, as on Unix.
#[cfg(target_os = "windows")]
fn status_changed(path: &Path) -> Option<i128> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    // Attributes only, which also opens a folder (backup semantics) and a binary another
    // process has loaded.
    let open = |flags: u32| {
        std::fs::OpenOptions::new()
            .access_mode(windows::FILE_READ_ATTRIBUTES)
            .custom_flags(windows::FILE_FLAG_BACKUP_SEMANTICS | flags)
            .open(path)
    };
    let file = open(0)
        .or_else(|_| open(windows::FILE_FLAG_OPEN_REPARSE_POINT))
        .ok()?;
    let mut info = windows::FileBasicInfo::default();
    // SAFETY: the handle lives as long as `file`, and `info` is a `FILE_BASIC_INFO` of the size
    // the call is told.
    let read = unsafe {
        windows::GetFileInformationByHandleEx(
            file.as_raw_handle(),
            windows::FILE_BASIC_INFO_CLASS,
            (&raw mut info).cast(),
            std::mem::size_of::<windows::FileBasicInfo>() as u32,
        )
    };
    // The time counts tenths of a microsecond.
    (read != 0).then(|| i128::from(info.change_time) * 100)
}

/// Declared here instead of taking a dependency.
#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::c_void;

    pub(super) const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    pub(super) const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    pub(super) const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    /// `FileBasicInfo` of `FILE_INFO_BY_HANDLE_CLASS`.
    pub(super) const FILE_BASIC_INFO_CLASS: i32 = 0;

    /// `FILE_BASIC_INFO`.
    #[repr(C)]
    #[derive(Default)]
    pub(super) struct FileBasicInfo {
        _creation_time: i64,
        _last_access_time: i64,
        _last_write_time: i64,
        pub(super) change_time: i64,
        _file_attributes: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(super) fn GetFileInformationByHandleEx(
            file: *mut c_void,
            class: i32,
            information: *mut c_void,
            size: u32,
        ) -> i32;
    }
}

/// The folder of a bundle that holds its binaries: `Contents/MacOS` on macOS, and the folder
/// of this machine's architecture elsewhere, such as `Contents/x86_64-linux` or
/// `Contents/x86_64-win`. VST 3 calls 64-bit Arm `arm64` on Windows.
pub(crate) fn binary_folder(bundle: &Path) -> PathBuf {
    let contents = bundle.join("Contents");
    let architecture = std::env::consts::ARCH;
    if cfg!(target_os = "macos") {
        contents.join("MacOS")
    } else if cfg!(target_os = "windows") {
        let architecture = match architecture {
            "aarch64" => "arm64",
            other => other,
        };
        contents.join(format!("{architecture}-win"))
    } else {
        contents.join(format!("{architecture}-linux"))
    }
}

/// Scans every bundle under `folders`, using `cache` for the ones that have not changed, and
/// tells `progress` after each one. `stop` ends the scan between bundles.
pub(crate) fn scan_folders(
    folders: &[PathBuf],
    command: &ScanCommand,
    cache: &ScanCache,
    stop: &AtomicBool,
    mut progress: impl FnMut(&Scan),
) -> Scan {
    let bundles = bundles(folders);
    let remembered = cache.read();
    let tidied = cache.tidy();
    let mut scan = Scan {
        bundles: bundles.len(),
        ..Scan::default()
    };
    let mut to_remember = Vec::with_capacity(bundles.len());
    let next = AtomicUsize::new(0);
    // Each scanner hands what it found to this thread, which alone keeps the scan.
    let look = |found: Sender<_>| {
        while !stop.load(Ordering::Acquire) {
            let Some(bundle) = bundles.get(next.fetch_add(1, Ordering::Relaxed)) else {
                return;
            };
            if found
                .send((bundle, look_at(bundle, command, &remembered)))
                .is_err()
            {
                return;
            }
        }
    };
    std::thread::scope(|scope| {
        let (found, receiver) = std::sync::mpsc::channel();
        let started = (0..SCANNERS.min(bundles.len()))
            .filter(|_| {
                let found = found.clone();
                let thread = std::thread::Builder::new().name("plugin-scanner".to_string());
                thread.spawn_scoped(scope, move || look(found)).is_ok()
            })
            .count();
        // A machine that cannot start a thread scans on this one.
        match started {
            0 => look(found),
            _ => drop(found),
        }
        for (bundle, (stamp, found)) in receiver {
            // A failure on this side says nothing about the bundle, so it is not remembered.
            let outcome = match &found {
                Ok(plugins) => Some((plugins.clone(), None)),
                Err(ScanError::Bundle(message)) => Some((Vec::new(), Some(message.clone()))),
                Err(ScanError::Host(_)) => None,
            };
            if let (Some(stamp), Some((plugins, failure))) = (stamp, outcome) {
                to_remember.push(CachedBundle {
                    path: bundle.path.clone(),
                    format: bundle.format,
                    stamp,
                    plugins,
                    failure,
                });
            }
            match found {
                Ok(plugins) => {
                    scan.plugins
                        .extend(plugins.into_iter().map(|plugin| ScannedPlugin {
                            path: bundle.path.clone(),
                            ..plugin
                        }))
                }
                Err(ScanError::Bundle(message) | ScanError::Host(message)) => {
                    scan.failures.push(ScanFailure {
                        path: bundle.path.clone(),
                        message,
                    })
                }
            }
            progress(&scan);
        }
    });
    if stop.load(Ordering::Acquire) {
        return scan;
    }
    // In order, so a scan that found what was remembered compares equal to it.
    scan.plugins
        .sort_by(|left, right| (left.format, &left.id).cmp(&(right.format, &right.id)));
    scan.failures
        .sort_by(|left, right| left.path.cmp(&right.path));
    to_remember.sort_by(|left, right| (&left.path, left.format).cmp(&(&right.path, right.format)));
    scan.finished = true;
    let written = match to_remember != remembered {
        true => cache.write(&to_remember),
        false => Ok(()),
    };
    scan.cache_error = written.and(tidied).err();
    progress(&scan);
    scan
}

type Looked = (Option<Stamp>, Result<Vec<ScannedPlugin>, ScanError>);

/// What the cache remembers of the bundle when it has not changed, else what a child finds.
fn look_at(bundle: &Bundle, command: &ScanCommand, remembered: &[CachedBundle]) -> Looked {
    let stamp = stamp(&bundle.path);
    let known = stamp.as_ref().and_then(|stamp| {
        remembered.iter().find(|entry| {
            entry.path == bundle.path && entry.format == bundle.format && entry.stamp == *stamp
        })
    });
    let found = match known {
        Some(entry) => match &entry.failure {
            Some(message) => Err(ScanError::Bundle(message.clone())),
            None => Ok(entry.plugins.clone()),
        },
        None => command.scan(bundle),
    };
    (stamp, found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bundle as macOS lays one out, with the binary folder of this platform: the binary the
    /// `Info.plist` names, a helper next to it, and resources. Nothing in it is a plugin; these
    /// tests are about the files.
    fn bundle_in(folder: &Path) -> PathBuf {
        let bundle = folder.join("piano.vst3");
        let binaries = binary_folder(&bundle);
        std::fs::create_dir_all(&binaries).unwrap();
        std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        std::fs::write(bundle.join("Contents/Info.plist"), "<plist/>").unwrap();
        std::fs::write(binaries.join("a-helper"), "helper").unwrap();
        std::fs::write(binaries.join("piano"), "the first build").unwrap();
        std::fs::write(bundle.join("Contents/Resources/sound"), "samples").unwrap();
        bundle
    }

    /// An installer may keep a replaced binary's modified time, and a new build may be the same
    /// size. A file outside the binary folder, such as samples, changes nothing.
    #[test]
    fn a_binary_replaced_with_the_same_size_and_time_changes_the_stamp() {
        let folder = tempfile::tempdir().unwrap();
        let bundle = bundle_in(folder.path());
        let before = stamp(&bundle).unwrap();

        std::fs::write(bundle.join("Contents/Resources/sound"), "other samples").unwrap();
        assert_eq!(stamp(&bundle), Some(before.clone()));

        let binary = binary_folder(&bundle).join("piano");
        let modified = std::fs::metadata(&binary).unwrap().modified().unwrap();
        // Windows moves its file times in steps of up to 16 ms, so the new build has to come
        // in a later step than the first one to be told apart at all.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&binary, "the next build!").unwrap();
        let file = std::fs::File::options().write(true).open(&binary).unwrap();
        file.set_modified(modified).unwrap();
        let metadata = std::fs::metadata(&binary).unwrap();
        assert_eq!(metadata.len(), "the first build".len() as u64);
        assert_eq!(metadata.modified().unwrap(), modified);
        assert_ne!(stamp(&bundle), Some(before));
    }

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
        cache.write(&remembered(other)).unwrap();
        let found = scan(&cache);
        assert_eq!(found.plugins, []);
        assert_eq!(
            found.failures.len(),
            1,
            "the bundle was not looked at again"
        );

        cache.write(&remembered(std::env::consts::ARCH)).unwrap();
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
                        cache.write(&bundles).unwrap();
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

    /// A scanner that did not start says nothing about the bundle. It is reported, and tried
    /// again by the next scan instead of being remembered as a bundle that failed.
    #[test]
    fn a_scanner_that_did_not_start_is_not_remembered_as_a_failed_bundle() {
        let folder = tempfile::tempdir().unwrap();
        bundle_in(folder.path());
        let cache = ScanCache::at(folder.path().join("plugins.json"));
        let scan = scan_folders(
            &[folder.path().to_path_buf()],
            &ScanCommand::new("/definitely/not/a/program", []),
            &cache,
            &AtomicBool::new(false),
            |_| {},
        );
        assert_eq!(scan.failures.len(), 1);
        assert_eq!(cache.read(), []);
    }

    /// A plugin folder may hold a link to a bundle that was taken away. It is remembered like
    /// any bundle that failed, so a look again does not start a child for it every time.
    /// Unix only: making a link on Windows needs administrator rights or developer mode.
    #[cfg(unix)]
    #[test]
    fn a_link_that_leads_nowhere_is_stamped_and_remembered() {
        let folder = tempfile::tempdir().unwrap();
        let link = folder.path().join("gone.vst3");
        std::os::unix::fs::symlink(folder.path().join("nowhere"), &link).unwrap();
        assert!(stamp(&link).is_some());
    }

    /// A writer that ended between its own file and the rename, which only a crash does, left
    /// that file behind. The next scan removes it, and leaves a file that may be another
    /// runtime's write in progress.
    #[test]
    fn what_a_writer_that_ended_left_behind_is_removed() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("plugins.json");
        let old = folder.path().join("plugins.json.999-0.tmp");
        let young = folder.path().join("plugins.json.999-1.tmp");
        let other = folder.path().join("other.tmp");
        for file in [&old, &young, &other] {
            std::fs::write(file, "{").unwrap();
        }
        let an_hour_ago = std::time::SystemTime::now() - Duration::from_secs(3600);
        for file in [&old, &other] {
            let file = std::fs::File::options().write(true).open(file).unwrap();
            file.set_modified(an_hour_ago).unwrap();
        }
        let scan = scan_folders(
            &[folder.path().join("plugins")],
            &ScanCommand::new("/definitely/not/a/program", []),
            &ScanCache::at(&path),
            &AtomicBool::new(false),
            |_| {},
        );
        assert_eq!(scan.cache_error, None);
        assert!(!old.exists());
        assert!(young.exists());
        assert!(other.exists());
    }
}
