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
use std::process::{Child, Stdio};
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
/// that never answers costs, once.
pub(crate) const SCAN_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Why the cache of this machine could not be written or tidied, when it could not. It
    /// costs the next start a scan and nothing else, and the composer is told.
    pub cache_error: Option<String>,
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

    /// Scans one bundle in a child process.
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
        // Blocking here is the point: this runs on the thread that scans, never on the one that
        // draws. `output` would wait for ever, and a deadline needs a handle to kill.
        #[allow(clippy::disallowed_methods)]
        let mut child = command
            .spawn()
            .map_err(|error| ScanError::Host(format!("the scanner did not start: {error}")))?;
        // Both pipes are read while the child runs and not after it ends. A plugin that prints
        // more than a pipe holds while it loads, which real ones do, would otherwise block on
        // its own write and be killed at the deadline as if it had hung.
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
            return Err(ScanError::Bundle(reason));
        }
        let mut plugins = Vec::new();
        // Anything the plugin itself printed is between these lines. It is not ours to read.
        for line in text.lines().filter_map(|line| line.strip_prefix(MARK)) {
            let plugin = serde_json::from_str(line).map_err(|error| {
                ScanError::Bundle(format!("the scanner printed something unreadable: {error}"))
            })?;
            plugins.push(plugin);
        }
        Ok(plugins)
    }
}

/// Kills the child and waits for it, so no child of this process is left behind. Gives
/// `reason`, with why the child could not be stopped when it could not.
fn stop(child: &mut Child, reason: String) -> String {
    match child.kill().and_then(|()| child.wait()) {
        Ok(_) => reason,
        Err(error) => format!("{reason}. The scanner could not be stopped: {error}"),
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

/// Every folder this machine keeps plugins in, for every format this build hosts. The lists
/// are the ones the specifications give: `entry.h` for CLAP, and Steinberg's for VST 3. Each
/// format also takes more folders from an environment variable.
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
        // A search folder that is not there is normal: this machine has no plugins of that kind.
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let format = path.extension().and_then(PluginFormat::of_extension);
        if let Some(format) = format {
            // A VST 3 bundle is a folder, or on Windows also the older single file, and a CLAP
            // one either. Either way it is one entry and nothing inside it is another plugin.
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
    /// The plugin windows of every project, when there is no file to keep them in.
    kept_placements: crate::placements::Kept,
}

/// Where the cache of this machine is kept, unless the environment says otherwise. The variable
/// is what a test sets so that it never touches the machine's own file.
const CACHE_VARIABLE: &str = "SOUND_TOOLS_PLUGIN_CACHE";

impl ScanCache {
    /// The cache of this machine: `~/Library/Caches/sound-tools/plugins.json` on macOS,
    /// `%LOCALAPPDATA%\Sound Tools\Cache\plugins.json` on Windows, next to the app's other
    /// folders there, and `~/.cache/sound-tools/plugins.json` on Linux, or under
    /// `XDG_CACHE_HOME` when it is set.
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
            kept_placements: crate::placements::Kept::default(),
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

    /// Removes what a writer left behind when it ended between writing its own file and
    /// renaming it, which only a crash does. A file younger than [`STALE_AFTER`] may be the one
    /// another runtime is writing right now, so it stays.
    fn tidy(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        remove_stale_writes(path).map_err(|errors| {
            format!("what an earlier write of the plugin cache left could not be removed: {errors}")
        })
    }

    /// Where this machine keeps the plugin windows of every project: the same folder as the
    /// cache, see `placements.rs`. A cache with no file keeps them in memory, for as long as a
    /// clone of it lives.
    pub(crate) fn placement_store(&self) -> crate::placements::PlacementStore {
        crate::placements::PlacementStore::new(
            self.path
                .as_ref()
                .map(|path| path.with_file_name(crate::placements::FILE)),
            self.kept_placements.clone(),
        )
    }
}

/// Removes what a writer of `path` left behind when it ended between writing its own file and
/// renaming it, which only a crash does. A file younger than [`STALE_AFTER`] may be the one
/// another runtime is writing right now, so it stays. What could not be removed is the error.
pub(crate) fn remove_stale_writes(path: &Path) -> Result<(), String> {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(());
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        // No folder yet, so nothing was left in it.
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

/// How old a file a writer of the cache left behind must be before it is taken for one whose
/// writer ended. A write takes milliseconds.
const STALE_AFTER: Duration = Duration::from_secs(60);

/// Writes `bytes` as the whole of `path`, or leaves what is there.
///
/// Two runtimes on one machine may finish a scan at the same moment, and each writes the cache.
/// A plain write truncates the file and then fills it, so the two could interleave, and a
/// reader in between reads half a file. So each writer writes a file of its own, named after
/// its process and a count, and renames it over the cache, which the system does in one step: a
/// reader sees one whole file or the other, and the last rename wins. Both are what one scan
/// found, so either is right.
pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
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
    // The file of this writer, if it was made, goes with the failure. One that cannot be
    // removed is taken away by `ScanCache::tidy` at a later start, and said here.
    match std::fs::remove_file(&temporary) {
        Err(left) if left.kind() != std::io::ErrorKind::NotFound => Err(format!(
            "{error}, and {} was left behind: {left}",
            temporary.display()
        )),
        _ => Err(error.to_string()),
    }
}

/// Renames `from` over `to`.
///
/// Windows refuses to replace a file while another process has it open without letting it be
/// deleted, which another writer's rename and a virus scanner looking at a new file both do for
/// a moment. So there a refusal is tried again a few times before it counts. Elsewhere a rename
/// is never refused for that.
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

/// How often a refused rename is tried, and how long apart: 50 ms in all, much longer than a
/// rename takes.
const RENAME_TRIES: u32 = 10;
const RENAME_PAUSE: Duration = Duration::from_millis(5);

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
/// `Info.plist`, so the stamp does not pick one: it is the latest status change of the binary
/// folder, of every file directly in it, and of the `Info.plist`. The system sets a file's
/// status change time on every write, rename, copy or replace, and nothing can set it back, so
/// a binary that was changed or replaced moves it, even one of the same size whose modified
/// time an installer kept. The modified time, the size and the creation time, which every
/// system has, miss exactly that file. The folder's own moves when a file is added or taken
/// away. A plugin that is a single file, as a CLAP one may be, is stamped by that file.
///
/// And the architecture of the host: a universal binary is another plugin to an arm64 host
/// than to one under Rosetta, and one that is x86_64 only fails on the first and not the second.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    architecture: String,
    /// Nanoseconds since the epoch of the system's file times.
    changed: i128,
}

/// The stamp of `bundle` on this host. `None` says the bundle is gone.
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

/// When the status of `path` last changed: `ctime`. A link that leads nowhere is stamped by the
/// link itself, so it is remembered as the bundle that failed and is not looked at again by
/// every scan.
#[cfg(unix)]
fn status_changed(path: &Path) -> Option<i128> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::metadata(path)
        .or_else(|_| std::fs::symlink_metadata(path))
        .ok()?;
    Some(i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()))
}

/// When the status of `path` last changed: the `ChangeTime` NTFS keeps, which is what `ctime` is
/// elsewhere. The standard library does not read it yet. A link that leads nowhere is stamped
/// by the link itself, as on the other systems.
#[cfg(target_os = "windows")]
fn status_changed(path: &Path) -> Option<i128> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    // Reading the attributes is all the query needs, which also works on a folder (backup
    // semantics) and on a binary another process has loaded.
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
    // SAFETY: the handle is open for as long as `file` lives, and `info` is a `FILE_BASIC_INFO`
    // of the size the call is told.
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

/// The few Windows calls the stamp needs, declared here instead of taking a dependency.
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
/// tells `progress` after each one so a caller can show what is known while the rest runs.
///
/// `stop` ends the scan between bundles, for a host that goes while its scan runs.
pub(crate) fn scan_folders(
    folders: &[PathBuf],
    command: &ScanCommand,
    cache: &ScanCache,
    stop: &std::sync::atomic::AtomicBool,
    mut progress: impl FnMut(&Scan),
) -> Scan {
    use std::sync::atomic::Ordering;

    let bundles = bundles(folders);
    let remembered = cache.read();
    let tidied = cache.tidy();
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
                Some(message) => Err(ScanError::Bundle(message.clone())),
                None => Ok(entry.plugins.clone()),
            },
            None => command.scan(&bundle),
        };
        // What went wrong on this side says nothing about the bundle, so it is not remembered.
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
            Ok(plugins) => scan
                .plugins
                .extend(plugins.into_iter().map(|plugin| ScannedPlugin {
                    path: bundle.path.clone(),
                    ..plugin
                })),
            Err(ScanError::Bundle(message) | ScanError::Host(message)) => {
                scan.failures.push(ScanFailure {
                    path: bundle.path,
                    message,
                })
            }
        }
        progress(&scan);
    }
    scan.plugins
        .sort_by(|left, right| (left.format, &left.id).cmp(&(right.format, &right.id)));
    scan.finished = true;
    // A scan that found what was remembered writes nothing, so looking again while the app
    // runs costs no write.
    let written = match to_remember != remembered {
        true => cache.write(&to_remember),
        false => Ok(()),
    };
    scan.cache_error = written.and(tidied).err();
    progress(&scan);
    scan
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

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
