//! The library: free sampled instruments a record names by id, such as
//! `"vsco/cello-section-sustain"`, downloaded when the composer asks and shared by every project
//! of the machine.
//!
//! The catalog is in the code ([`CATALOG`]): each instrument is one SFZ file of a library on
//! GitHub, at a pinned commit, so what an id plays never changes. Only the files that SFZ file
//! needs are downloaded, into the library folder the runtime gives ([`set_folder`]):
//! `<folder>/<library>/<commit>/` with the paths of the repository. A file `.<name>.done`
//! next to them says an instrument is whole. A download that stopped half way has none, and
//! its files are fetched again.
//!
//! What is on this machine is never part of a project, as plugins: a project that names an
//! instrument this machine lacks loads, says so in `problems.txt`, and plays it once it is
//! downloaded. Nothing downloads by itself, also not when an agent writes a record: only
//! [`download`], which the Sampler card calls when the composer picks an instrument or clicks
//! Download.
//!
//! Downloads run on threads of their own with `/usr/bin/curl`, as the app's own update, so
//! there is no HTTP code here. A finished one names the Samplers that waited for it
//! ([`take_finished`]), and the runtime runs their behaviour again.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sound_core::{Assets, InstanceId};

pub use crate::catalog::CATALOG;
use crate::instrument::relative_inside;
use crate::sfz;

/// A library on GitHub that instruments of the catalog come from.
#[derive(Debug, PartialEq, Eq)]
pub struct Library {
    /// The first part of the id of its instruments, and its folder.
    pub id: &'static str,
    pub name: &'static str,
    /// `owner/name` on GitHub.
    pub repository: &'static str,
    /// The commit every file is read at.
    pub commit: &'static str,
    /// Its licence, short: `CC0`, `CC BY 4.0`.
    pub license: &'static str,
    /// What a CC BY licence asks to show with the sound, or `None`.
    pub attribution: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Piano,
    Keys,
    Strings,
    Brass,
    Woodwinds,
    Guitar,
    Bass,
    Drums,
    Percussion,
}

impl Category {
    pub const ALL: [Self; 9] = [
        Self::Piano,
        Self::Keys,
        Self::Strings,
        Self::Brass,
        Self::Woodwinds,
        Self::Guitar,
        Self::Bass,
        Self::Drums,
        Self::Percussion,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Piano => "Piano",
            Self::Keys => "Keys",
            Self::Strings => "Strings",
            Self::Brass => "Brass",
            Self::Woodwinds => "Woodwinds",
            Self::Guitar => "Guitar",
            Self::Bass => "Bass",
            Self::Drums => "Drums",
            Self::Percussion => "Percussion",
        }
    }
}

/// One instrument of the catalog.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    /// `<library>/<name>`, as a record names it.
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub library: &'static Library,
    /// Its SFZ file in the repository.
    pub sfz: &'static str,
    /// What its download is, and what it takes in memory once it plays, in bytes.
    pub download_bytes: u64,
    pub memory_bytes: u64,
}

impl Entry {
    /// The part of the id after the library, which names its marker file.
    fn short_name(&self) -> &'static str {
        self.id.split_once('/').map_or(self.id, |(_, name)| name)
    }
}

/// The instrument of the catalog with this id.
pub fn entry(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

/// A library instrument as a record names it: an id of [`CATALOG`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LibraryId(&'static Entry);

impl LibraryId {
    pub fn entry(&self) -> &'static Entry {
        self.0
    }
}

impl TryFrom<String> for LibraryId {
    type Error = String;

    fn try_from(id: String) -> Result<Self, String> {
        entry(&id).map(Self).ok_or_else(|| {
            format!("{id:?} is no instrument of the library: agent-docs/sampler.md lists their ids")
        })
    }
}

impl From<LibraryId> for String {
    fn from(id: LibraryId) -> String {
        id.0.id.to_string()
    }
}

impl fmt::Display for LibraryId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.id)
    }
}

/// Where the library is on this machine, and where its files come from.
struct Machine {
    folder: Option<PathBuf>,
    /// The start of the URL of a file: `<source>/<repository>/<commit>/<path>`. Any URL curl
    /// takes, so a test serves files from a folder.
    source: String,
}

static MACHINE: LazyLock<RwLock<Machine>> = LazyLock::new(|| {
    RwLock::new(Machine {
        folder: None,
        source: "https://raw.githubusercontent.com".to_string(),
    })
});

/// Sets the library folder of this machine. Without one, no library instrument plays.
pub fn set_folder(folder: PathBuf) {
    machine_mut().folder = Some(folder);
}

/// Where files come from instead of GitHub, for tests: `file:///…` with
/// `<repository>/<commit>/<path>` under it.
pub fn set_source(source: &str) {
    machine_mut().source = source.trim_end_matches('/').to_string();
}

fn machine_mut() -> std::sync::RwLockWriteGuard<'static, Machine> {
    MACHINE.write().unwrap_or_else(PoisonError::into_inner)
}

fn machine() -> std::sync::RwLockReadGuard<'static, Machine> {
    MACHINE.read().unwrap_or_else(PoisonError::into_inner)
}

/// The folder of the files of `library` at its commit.
fn library_folder(library: &Library) -> Option<PathBuf> {
    let folder = machine().folder.clone()?;
    Some(folder.join(library.id).join(library.commit))
}

/// Where the SFZ file of `entry` is on this machine, and the folder its paths stay in.
pub(crate) fn sfz_file(entry: &Entry) -> Option<(PathBuf, PathBuf)> {
    let root = library_folder(entry.library)?;
    Some((root.join(entry.sfz), root))
}

fn marker(entry: &Entry) -> Option<PathBuf> {
    let root = library_folder(entry.library)?;
    Some(root.join(format!(".{}.done", entry.short_name())))
}

/// Where an instrument is on this machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Downloaded whole.
    Here,
    /// Not here, and no download of it runs.
    Missing,
    /// On its way: the bytes that are here, of its [`Entry::download_bytes`].
    Downloading { bytes: u64 },
    /// The last download failed, and why.
    Failed(String),
    /// This machine has no library folder, as in a test with no library.
    NoLibrary,
}

#[derive(Default)]
struct Downloads {
    /// Running, by entry id, with the bytes that are here.
    running: BTreeMap<&'static str, u64>,
    failed: BTreeMap<&'static str, String>,
    /// The Samplers waiting for each, with the `assets/` of their project.
    waiting: BTreeMap<&'static str, BTreeSet<(PathBuf, InstanceId)>>,
    /// Samplers whose instrument came, or failed to: for [`take_finished`].
    finished: Vec<(PathBuf, InstanceId)>,
}

static DOWNLOADS: LazyLock<(Mutex<Downloads>, Condvar)> = LazyLock::new(Default::default);

fn downloads() -> MutexGuard<'static, Downloads> {
    DOWNLOADS.0.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn status(entry: &'static Entry) -> Status {
    let Some(marker) = marker(entry) else {
        return Status::NoLibrary;
    };
    let downloads = downloads();
    if let Some(bytes) = downloads.running.get(entry.id) {
        return Status::Downloading { bytes: *bytes };
    }
    if marker.exists() {
        return Status::Here;
    }
    match downloads.failed.get(entry.id) {
        Some(error) => Status::Failed(error.clone()),
        None => Status::Missing,
    }
}

/// The Sampler `instance` of the project of `assets` plays `entry`, which is not here: its
/// behaviour runs again when a download of it ends.
pub(crate) fn wait_for(entry: &'static Entry, assets: &Assets, instance: &InstanceId) {
    let project = project_of(assets);
    downloads()
        .waiting
        .entry(entry.id)
        .or_default()
        .insert((project, instance.clone()));
}

/// Starts the download of `entry`, also after one failed. Does nothing while one runs or when
/// it is here.
pub fn download(entry: &'static Entry) {
    downloads().failed.remove(entry.id);
    if status(entry) != Status::Missing {
        return;
    }
    let Some(root) = library_folder(entry.library) else {
        return;
    };
    downloads().running.insert(entry.id, 0);
    let spawned = std::thread::Builder::new()
        .name(format!("download {}", entry.id))
        .spawn(move || {
            let result = fetch_instrument(entry, &root);
            let mut downloads = downloads();
            downloads.running.remove(entry.id);
            if let Err(error) = result {
                downloads.failed.insert(entry.id, error);
            }
            let waiting = downloads.waiting.remove(entry.id).unwrap_or_default();
            downloads.finished.extend(waiting);
            DOWNLOADS.1.notify_all();
        });
    if let Err(error) = spawned {
        let mut downloads = downloads();
        downloads.running.remove(entry.id);
        let error = format!("the download did not start: {error}");
        downloads.failed.insert(entry.id, error);
    }
}

/// The Samplers of the project of `assets` whose download finished since the last call, to
/// run their behaviour again.
pub(crate) fn take_finished(assets: &Assets) -> Vec<InstanceId> {
    let project = project_of(assets);
    let mut downloads = downloads();
    let (ours, others): (Vec<_>, Vec<_>) = std::mem::take(&mut downloads.finished)
        .into_iter()
        .partition(|(folder, _)| *folder == project);
    downloads.finished = others;
    ours.into_iter().map(|(_, instance)| instance).collect()
}

/// Waits until no download runs. For tests.
pub fn wait_for_downloads() {
    let (lock, done) = &*DOWNLOADS;
    let mut downloads = lock.lock().unwrap_or_else(PoisonError::into_inner);
    while !downloads.running.is_empty() {
        downloads = done.wait(downloads).unwrap_or_else(PoisonError::into_inner);
    }
}

/// Identifies a project by its `assets/` folder.
fn project_of(assets: &Assets) -> PathBuf {
    crate::instrument::instruments_folder(assets)
}

/// Downloads the SFZ file of `entry`, the files it includes and the samples it names into
/// `root`, then marks it whole.
fn fetch_instrument(entry: &'static Entry, root: &Path) -> Result<(), String> {
    let source = machine().source.clone();
    let url = |path: &str| {
        format!(
            "{source}/{}/{}/{}",
            entry.library.repository,
            entry.library.commit,
            url_path(path)
        )
    };
    let sfz_path = Path::new(entry.sfz);
    let folder = sfz_path.parent().unwrap_or(Path::new(""));
    let text = fetch_text(&url(entry.sfz), &root.join(sfz_path))?;
    let include = |name: &str| {
        let path = relative_inside(&folder.join(name)).ok_or("it is outside the library")?;
        let path = path.to_string_lossy().replace('\\', "/");
        fetch_text(&url(&path), &root.join(&path))
    };
    let parsed = sfz::parse(&text, &include).map_err(|error| error.to_string())?;
    let samples: BTreeSet<String> = parsed
        .regions
        .iter()
        .filter_map(|region| relative_inside(&folder.join(&region.sample)))
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let files: Vec<(String, PathBuf)> = samples
        .iter()
        .map(|path| (url(path), root.join(path)))
        .collect();
    fetch_files(entry, &files)?;
    if let Some(marker) = marker(entry) {
        fs::write(&marker, entry.id).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// A path of a repository in a URL: every byte but letters, digits, `-._~/` as `%XX`.
fn url_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                encoded.push(char::from(byte));
            }
            byte => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn curl() -> Command {
    let mut command = Command::new(if cfg!(target_os = "macos") {
        "/usr/bin/curl"
    } else {
        "curl"
    });
    command.args([
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--retry",
        "2",
    ]);
    command
}

/// Downloads one text file to `path` and gives its text.
fn fetch_text(url: &str, path: &Path) -> Result<String, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let output = curl()
        .arg("--output")
        .arg(path)
        .arg(url)
        .output()
        .map_err(|error| format!("could not run curl: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{url}: {}", error.trim()));
    }
    fs::read_to_string(path).map_err(|error| error.to_string())
}

/// Downloads `files`, each `(url, path)`, several at once, and keeps the bytes that are here
/// in the running download of `entry` while it waits.
fn fetch_files(entry: &'static Entry, files: &[(String, PathBuf)]) -> Result<(), String> {
    let quote = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    let mut config = String::new();
    for (url, path) in files {
        let path = path.to_string_lossy();
        config.push_str(&format!(
            "url = \"{}\"\noutput = \"{}\"\n",
            quote(url),
            quote(&path)
        ));
    }
    let Some(first) = files.first() else {
        return Ok(());
    };
    // The list goes next to the files, where this download may write.
    let list = first
        .1
        .with_file_name(format!(".{}.download", entry.short_name()));
    if let Some(parent) = list.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&list, config).map_err(|error| error.to_string())?;
    // On the download's own thread, which only waits for curl.
    #[allow(clippy::disallowed_methods)]
    let child = curl()
        .args([
            "--create-dirs",
            "--parallel",
            "--parallel-max",
            "8",
            "--config",
        ])
        .arg(&list)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            remove_quietly(&list);
            return Err(format!("could not run curl: {error}"));
        }
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(error) => break Err(error.to_string()),
        }
        let bytes = files
            .iter()
            .filter_map(|(_, path)| fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .sum();
        downloads().running.insert(entry.id, bytes);
        std::thread::sleep(Duration::from_millis(200));
    };
    remove_quietly(&list);
    let status = status?;
    if status.success() {
        return Ok(());
    }
    let mut error = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        // The reason is the best there is; when it cannot be read, the exit status says it.
        if stderr.read_to_string(&mut error).is_err() {
            error.clear();
        }
    }
    let error = error.lines().next().unwrap_or("").trim().to_string();
    Err(match error.is_empty() {
        true => format!("curl stopped with {status}"),
        false => error,
    })
}

fn remove_quietly(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        // A list left behind starts with a dot and is written over by the next download.
        Err(_) => {}
    }
}

/// `1.2 GB`, `85 MB`, for a size in a list.
pub fn size_text(bytes: u64) -> String {
    let megabytes = bytes as f64 / 1_000_000.0;
    if megabytes >= 1_000.0 {
        format!("{:.1} GB", megabytes / 1_000.0)
    } else {
        format!("{:.0} MB", megabytes.max(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_in_a_url_keeps_its_slashes_and_encodes_the_rest() {
        assert_eq!(
            url_path("Strings/Cello Section/susvib_A2#1.wav"),
            "Strings/Cello%20Section/susvib_A2%231.wav"
        );
    }

    #[test]
    fn every_id_of_the_catalog_is_its_library_and_a_name_and_is_unique() {
        let mut ids = BTreeSet::new();
        for entry in CATALOG {
            let (library, name) = entry.id.split_once('/').unwrap();
            assert_eq!(library, entry.library.id, "{}", entry.id);
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{}",
                entry.id
            );
            assert!(ids.insert(entry.id), "{} twice", entry.id);
            assert!(entry.sfz.ends_with(".sfz"), "{}", entry.id);
        }
    }

    #[test]
    fn sizes_read_as_megabytes_or_gigabytes() {
        assert_eq!(size_text(85_400_000), "85 MB");
        assert_eq!(size_text(1_230_000_000), "1.2 GB");
        assert_eq!(size_text(10), "1 MB");
    }
}
