//! What the Sampler plays: zones, each a region of an SFZ file with its sample in memory. One
//! `sample` of a record is an instrument of one zone that takes its pitch, its part of the
//! file and its envelope from the record, so the processor has one way to play.
//!
//! An SFZ instrument is read on the control side and kept while anything plays it: a second
//! Sampler with the same file, or the same Sampler after a knob moved, gets the same one and
//! reads nothing. It is read again when the SFZ file or a file it includes changed, or when it
//! missed samples, which may have arrived since.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, Weak};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sound_core::{AssetName, Assets, InstanceId};
use sound_media::{Audio, MediaError};

use crate::library::{self, Entry};
use crate::sfz::{self, LoopMode, Region};

/// The folder of SFZ instruments, under `assets/`.
pub const INSTRUMENTS_FOLDER: &str = "instruments";

/// An SFZ file of the project, as a record names it: its path under `assets/instruments/`,
/// such as `"cello/cello-sustain.sfz"`. Any file names, since packs come with their own, but
/// never a path that leaves that folder.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SfzPath(String);

impl SfzPath {
    pub fn new(path: &str) -> Result<Self, String> {
        let inside = relative_inside(Path::new(path)).is_some() && !path.contains('\\');
        let sfz = Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("sfz"));
        match inside && sfz {
            true => Ok(Self(path.to_string())),
            false => Err(format!(
                "{path:?} is no SFZ file under assets/instruments/: write its path in that folder, such as `cello/cello.sfz`"
            )),
        }
    }

    /// Where the file is in a project, for messages: `assets/instruments/cello/cello.sfz`.
    pub fn project_path(&self) -> String {
        format!(
            "{}/{INSTRUMENTS_FOLDER}/{}",
            sound_core::ASSETS_FOLDER,
            self.0
        )
    }
}

impl fmt::Display for SfzPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<String> for SfzPath {
    type Error = String;

    fn try_from(path: String) -> Result<Self, String> {
        Self::new(&path)
    }
}

impl From<SfzPath> for String {
    fn from(path: SfzPath) -> String {
        path.0
    }
}

/// `path` with `.` and `..` worked out, when it stays inside where it starts.
pub(crate) fn relative_inside(path: &Path) -> Option<PathBuf> {
    let mut inside = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => inside.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !inside.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!inside.as_os_str().is_empty()).then_some(inside)
}

/// `assets/instruments/` of a project.
pub(crate) fn instruments_folder(assets: &Assets) -> PathBuf {
    match AssetName::new(INSTRUMENTS_FOLDER, "any", "sfz") {
        Ok(any) => assets
            .path(&any)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
        Err(_) => PathBuf::new(),
    }
}

/// How a zone loops, in frames of its file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Looping {
    /// Plays to its end. A one shot also ignores the key coming up.
    No { one_shot: bool },
    /// From the first frame of the loop to the frame after its last. A sustain loop ends when
    /// the key comes up.
    Loop { start: f64, end: f64, sustain: bool },
}

/// One region with its sample.
pub(crate) struct Zone {
    pub region: Region,
    pub audio: Arc<Audio>,
    pub looping: Looping,
}

/// What a Sampler plays.
pub struct Instrument {
    pub(crate) zones: Box<[Zone]>,
    /// The keys that pick an articulation, and the one before any is played.
    pub(crate) switch_keys: Option<(u8, u8)>,
    pub(crate) switch_default: Option<u8>,
    /// One sample of a record: its pitch, part and envelope come from the record.
    pub(crate) from_record: bool,
    /// Why some regions are silent, for `problems.txt`.
    problem: Option<String>,
}

impl Instrument {
    /// One file across the keyboard, as the record says.
    pub fn of_sample(audio: Arc<Audio>) -> Self {
        let region = Region {
            sample: String::new(),
            keys: (0, 127),
            velocities: (0, 127),
            keycenter: 60,
            keytrack_cents: 100.0,
            tune_cents: 0.0,
            random: (0.0, 1.0),
            sequence: (1, 1),
            switch: None,
            trigger: sfz::Trigger::Attack,
            offset: 0,
            end: None,
            loop_mode: None,
            loop_start: None,
            loop_end: None,
            volume_db: 0.0,
            amplitude: 1.0,
            pan: 0.0,
            velocity_tracking: 0.0,
            envelope: sfz::Adsr {
                attack: 0.0,
                decay: 0.0,
                sustain: 1.0,
                release: 0.0,
            },
            group: 0,
            off_by: None,
            off_mode: sfz::OffMode::Fast,
        };
        let zone = Zone {
            region,
            audio,
            looping: Looping::No { one_shot: false },
        };
        Self {
            zones: Box::new([zone]),
            switch_keys: None,
            switch_default: None,
            from_record: true,
            problem: None,
        }
    }

    /// Whether `other` plays the same, so notes that sound go on: the same instrument, or one
    /// sample of a record in the same file.
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        match (&self.zones[..], &other.zones[..]) {
            ([this], [that]) => {
                self.from_record && other.from_record && Arc::ptr_eq(&this.audio, &that.audio)
            }
            _ => false,
        }
    }

    pub fn zone_count(&self) -> usize {
        self.zones.len()
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// A file the instrument was read from, with its size and time then.
type Stamp = (PathBuf, u64, Option<SystemTime>);

fn stamp(path: &Path) -> Option<Stamp> {
    let metadata = fs::metadata(path).ok()?;
    Some((path.to_path_buf(), metadata.len(), metadata.modified().ok()))
}

struct Known {
    files: Vec<Stamp>,
    instrument: Weak<Instrument>,
}

static KNOWN: LazyLock<Mutex<HashMap<PathBuf, Known>>> = LazyLock::new(Mutex::default);

/// The SFZ instrument a record names, from memory when it was read before and nothing changed.
/// An error says why it plays nothing; an instrument that plays may still have a problem.
pub fn load_sfz(
    assets: &Assets,
    path: &SfzPath,
    instance: &InstanceId,
) -> Result<Option<Arc<Instrument>>, String> {
    let file = sfz_file(assets, path);
    let instruments = instruments_folder(assets);
    // Samples may be anywhere in `assets/`, such as recordings in `assets/audio/`.
    let root = instruments.parent().unwrap_or(&instruments);
    let shown = path.project_path();
    let missing = || {
        format!(
            "the instrument {shown} is not there, so the Sampler is silent. Put the SFZ file and its samples under assets/instruments/, or correct `sfz`"
        )
    };
    load(&file, root, &shown, missing, (assets, instance))
}

/// The library instrument `entry`, downloaded whole on this machine.
pub(crate) fn load_library(
    entry: &'static Entry,
    assets: &Assets,
    instance: &InstanceId,
) -> Result<Option<Arc<Instrument>>, String> {
    let (file, root) = library::sfz_file(entry)
        .ok_or_else(|| format!("{} cannot play: this machine has no library", entry.name))?;
    let missing = || {
        format!(
            "the files of {} are gone from the library of this machine",
            entry.name
        )
    };
    load(&file, &root, entry.name, missing, (assets, instance))
}

fn sfz_file(assets: &Assets, path: &SfzPath) -> PathBuf {
    instruments_folder(assets).join(&path.0)
}

/// Whether the instrument of `state` is loading in the background, for the card.
pub fn is_loading(state: &crate::SamplerState, assets: &Assets) -> bool {
    let file = match (&state.sfz, &state.library) {
        (Some(path), _) => sfz_file(assets, path),
        (None, Some(id)) => match library::sfz_file(id.entry()) {
            Some((file, _)) => file,
            None => return false,
        },
        (None, None) => return false,
    };
    background().running.contains(&file)
}

static IN_BACKGROUND: AtomicBool = AtomicBool::new(false);

/// Loads an instrument that is not in memory on a thread of its own, so the window does not
/// wait for hundreds of samples: until it is there, the Sampler plays what it played, and its
/// card says it loads. Without this, as in a render, an inspect or a test, it loads at once.
pub fn load_in_background() {
    IN_BACKGROUND.store(true, Ordering::Relaxed);
}

/// The instruments that load in the background and the Samplers that wait for them.
#[derive(Default)]
struct Loading {
    running: BTreeSet<PathBuf>,
    /// Why one did not load, with the size and time of its file then.
    failed: HashMap<PathBuf, (Option<Stamp>, String)>,
    waiting: BTreeMap<PathBuf, BTreeSet<(PathBuf, InstanceId)>>,
    /// Samplers whose instrument is done, for [`take_loaded`].
    done: Vec<(PathBuf, InstanceId)>,
    /// What was loaded, held until the Samplers that wait for it took it: from when it is
    /// loaded to the call of [`take_loaded`] after the one that named them.
    held: Vec<Arc<Instrument>>,
    taken: Vec<Arc<Instrument>>,
}

static LOADING: LazyLock<(Mutex<Loading>, Condvar)> = LazyLock::new(Default::default);

fn background() -> MutexGuard<'static, Loading> {
    LOADING.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The Samplers of the project of `assets` whose instrument loaded since the last call, to run
/// their behaviour again. What they load is held until the next call, so it is still there
/// when they take it.
pub(crate) fn take_loaded(assets: &Assets) -> Vec<InstanceId> {
    let project = instruments_folder(assets);
    let mut loading = background();
    // Their Samplers took what was held at the last call, so it is let go of here.
    let held = std::mem::take(&mut loading.held);
    loading.taken = held;
    let (ours, others): (Vec<_>, Vec<_>) = std::mem::take(&mut loading.done)
        .into_iter()
        .partition(|(folder, _)| *folder == project);
    loading.done = others;
    ours.into_iter().map(|(_, instance)| instance).collect()
}

/// Waits until no instrument loads. For tests.
pub fn wait_for_loading() {
    let (lock, done) = &*LOADING;
    let mut loading = lock.lock().unwrap_or_else(PoisonError::into_inner);
    while !loading.running.is_empty() {
        loading = done.wait(loading).unwrap_or_else(PoisonError::into_inner);
    }
}

/// The SFZ instrument at `file`, whose files stay under `root`, from memory when it was read
/// before and nothing changed. `None` while it loads in the background for the Sampler
/// `waiter`. `missing` says why when the file is not there.
fn load(
    file: &Path,
    root: &Path,
    shown: &str,
    missing: impl Fn() -> String,
    (assets, instance): (&Assets, &InstanceId),
) -> Result<Option<Arc<Instrument>>, String> {
    if let Some(instrument) = known(file) {
        return Ok(Some(instrument));
    }
    if !file.exists() {
        return Err(missing());
    }
    if !IN_BACKGROUND.load(Ordering::Relaxed) {
        return read(file, root, shown).map(Some);
    }
    let mut loading = background();
    if let Some((stamped, error)) = loading.failed.get(file)
        && *stamped == stamp(file)
    {
        return Err(error.clone());
    }
    let waiter = (instruments_folder(assets), instance.clone());
    loading
        .waiting
        .entry(file.to_path_buf())
        .or_default()
        .insert(waiter);
    if loading.running.insert(file.to_path_buf()) {
        let spawned = std::thread::Builder::new()
            .name("sampler instrument".into())
            .spawn({
                let (file, root, shown) =
                    (file.to_path_buf(), root.to_path_buf(), shown.to_string());
                move || {
                    let read = read(&file, &root, &shown);
                    let mut loading = background();
                    loading.running.remove(&file);
                    match read {
                        Ok(instrument) => {
                            loading.failed.remove(&file);
                            loading.held.push(instrument);
                        }
                        Err(error) => {
                            loading.failed.insert(file.clone(), (stamp(&file), error));
                        }
                    }
                    let waiting = loading.waiting.remove(&file).unwrap_or_default();
                    loading.done.extend(waiting);
                    LOADING.1.notify_all();
                }
            });
        if let Err(error) = spawned {
            loading.running.remove(file);
            return Err(format!(
                "the Sampler is silent: {shown} did not start to load: {error}"
            ));
        }
    }
    Ok(None)
}

/// The instrument at `file` when it is in memory and its files did not change.
fn known(file: &Path) -> Option<Arc<Instrument>> {
    let known = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    let entry = known.get(file)?;
    let instrument = entry.instrument.upgrade()?;
    let same = entry
        .files
        .iter()
        .all(|saved| stamp(&saved.0).as_ref() == Some(saved));
    (instrument.problem.is_none() && same).then_some(instrument)
}

/// Reads the instrument at `file` and keeps it in memory for the next ask.
fn read(file: &Path, root: &Path, shown: &str) -> Result<Arc<Instrument>, String> {
    let text = fs::read_to_string(file)
        .map_err(|error| format!("the Sampler is silent: {shown} cannot be read: {error}"))?;
    let (instrument, files) = read_sfz(root, file, &text, shown)?;
    let instrument = Arc::new(instrument);
    let mut known = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    known.retain(|_, entry| entry.instrument.strong_count() > 0);
    known.insert(
        file.to_path_buf(),
        Known {
            files,
            instrument: Arc::downgrade(&instrument),
        },
    );
    Ok(instrument)
}

/// Reads the SFZ file at `file`, whose text is `text`, and the files it names, which must be under `root`. Gives the
/// text files it read, to know when to read it again.
fn read_sfz(
    root: &Path,
    file: &Path,
    text: &str,
    shown: &str,
) -> Result<(Instrument, Vec<Stamp>), String> {
    let folder = file.parent().unwrap_or(root).to_path_buf();
    let read = Mutex::new(vec![stamp(file)].into_iter().flatten().collect::<Vec<_>>());
    let include = |name: &str| {
        let path = inside(root, &folder, name).ok_or("it is outside its folder")?;
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        if let Some(stamp) = stamp(&path) {
            read.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(stamp);
        }
        Ok(text)
    };
    let sfz = sfz::parse(text, &include)
        .map_err(|error| format!("the Sampler is silent: {shown} cannot be read: {error}"))?;
    let files = read.into_inner().unwrap_or_else(PoisonError::into_inner);

    let paths: Vec<Option<PathBuf>> = sfz
        .regions
        .iter()
        .map(|region| inside(root, &folder, &region.sample))
        .collect();
    let mut loaded = load_all(&paths, &sfz.regions);
    let mut zones = Vec::with_capacity(sfz.regions.len());
    let mut missing = Vec::new();
    let mut unreadable = Vec::new();
    for (region, loaded) in sfz.regions.into_iter().zip(loaded.drain(..)) {
        let Some(loaded) = loaded else {
            unreadable.push(format!("{} is outside its folder", region.sample));
            continue;
        };
        match loaded {
            Ok(audio) => {
                let looping = looping(&region, &audio);
                zones.push(Zone {
                    region,
                    audio,
                    looping,
                });
            }
            Err(MediaError::Missing { .. }) => missing.push(region.sample),
            Err(error) => unreadable.push(error.to_string()),
        }
    }
    missing.sort();
    missing.dedup();
    unreadable.sort();
    unreadable.dedup();
    let mut problems = Vec::new();
    if let Some(first) = missing.first() {
        problems.push(format!(
            "{} sample{} of {shown} {} missing, such as {first}",
            missing.len(),
            if missing.len() == 1 { "" } else { "s" },
            if missing.len() == 1 { "is" } else { "are" },
        ));
    }
    if let Some(first) = unreadable.first() {
        problems.push(format!(
            "{} sample{} of {shown} cannot play: {first}",
            unreadable.len(),
            if unreadable.len() == 1 { "" } else { "s" },
        ));
    }
    if zones.is_empty() {
        let why = match problems.is_empty() {
            true => "it has no region that plays".to_string(),
            false => problems.join("; "),
        };
        return Err(format!("the Sampler is silent: {shown}: {why}"));
    }
    let problem = (!problems.is_empty()).then(|| {
        format!(
            "the Sampler plays {shown} with gaps: {}",
            problems.join("; ")
        )
    });
    let instrument = Instrument {
        zones: zones.into_boxed_slice(),
        switch_keys: sfz.switch_keys,
        switch_default: sfz.switch_default,
        from_record: false,
        problem,
    };
    Ok((instrument, files))
}

/// The file of each region, `None` for one with no path. Read on every core at once: a piano
/// is hundreds of FLAC files to decode. A file several regions name is read once, since
/// [`sound_media::load_path`] shares what it read.
fn load_all(
    paths: &[Option<PathBuf>],
    regions: &[Region],
) -> Vec<Option<Result<Arc<Audio>, MediaError>>> {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let read = || {
        let mut read = Vec::new();
        loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            let (Some(path), Some(region)) = (paths.get(index), regions.get(index)) else {
                return read;
            };
            let audio = path
                .as_ref()
                .map(|path| sound_media::load_path(path, &region.sample));
            read.push((index, audio));
        }
    };
    let mut loaded: Vec<_> = (0..paths.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads).map(|_| scope.spawn(read)).collect();
        for worker in workers {
            // A worker that panicked leaves its regions unread, so they count as missing.
            for (index, audio) in worker.join().unwrap_or_default() {
                if let Some(slot) = loaded.get_mut(index) {
                    *slot = audio;
                }
            }
        }
    });
    loaded
}

/// A path an SFZ file names, relative to its folder, when it stays under `root`.
fn inside(root: &Path, folder: &Path, name: &str) -> Option<PathBuf> {
    let relative = folder
        .strip_prefix(root)
        .ok()?
        .join(name.replace('\\', "/"));
    relative_inside(&relative).map(|relative| root.join(relative))
}

/// How a region loops: as its opcodes say, else with the loop of its file. A loop that does not
/// fit in the part that plays is no loop.
fn looping(region: &Region, audio: &Audio) -> Looping {
    let file_loop = audio.sample_loop();
    let mode = region.loop_mode.unwrap_or(match file_loop {
        Some(_) => LoopMode::Continuous,
        None => LoopMode::NoLoop,
    });
    let start = region
        .loop_start
        .or(file_loop.map(|file_loop| file_loop.start));
    let end = region.loop_end.or(file_loop.map(|file_loop| file_loop.end));
    let sustain = match mode {
        LoopMode::NoLoop => return Looping::No { one_shot: false },
        LoopMode::OneShot => return Looping::No { one_shot: true },
        LoopMode::Continuous => false,
        LoopMode::Sustain => true,
    };
    match (start, end) {
        (Some(start), Some(end)) if start < end && end < audio.frames() => Looping::Loop {
            start: start as f64,
            // The last frame of the loop is in it.
            end: (end + 1) as f64,
            sustain,
        },
        _ => Looping::No { one_shot: false },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_sfz_path_stays_under_assets_instruments() {
        for good in [
            "cello/cello.sfz",
            "Salamander/Salamander Grand Piano V3.sfz",
            "a.SFZ",
        ] {
            assert!(SfzPath::new(good).is_ok(), "{good}");
        }
        for wrong in [
            "../cello.sfz",
            "/cello.sfz",
            "cello/../../x.sfz",
            "cello.wav",
            "a\\b.sfz",
            "",
        ] {
            assert!(SfzPath::new(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_sample_path_may_go_up_but_not_out_of_the_assets() {
        let root = Path::new("/p/assets");
        let folder = root.join("instruments/pack/programs");
        assert_eq!(
            inside(root, &folder, "..\\samples\\a.wav"),
            Some(root.join("instruments/pack/samples/a.wav"))
        );
        assert_eq!(
            inside(root, &folder, "../../../audio/a.wav"),
            Some(root.join("audio/a.wav"))
        );
        assert_eq!(inside(root, &folder, "../../../../a.wav"), None);
        assert_eq!(inside(root, &folder, "/etc/a.wav"), None);
    }
}
