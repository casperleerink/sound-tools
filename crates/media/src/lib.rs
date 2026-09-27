//! Media: the audio files of a project, and how to read them.
//!
//! No extension: audio clips, the Sampler and the Drum pad all read files through this crate,
//! and none of them depends on another. The core knows no audio files; to it a file under
//! `assets/audio/` is an asset like any other.
//!
//! - [`AudioAsset`] is how a record names a file: `"voice.wav"`, which is
//!   `assets/audio/voice.wav`. Never a path outside the project.
//! - [`import`] copies a file into `assets/audio/` under a free name.
//! - [`load`] gives the file in memory, [`Audio`], shared by everything that plays it, and
//!   [`info`] what it is, how long and at what rate.
//! - [`Resampler`] plays it at another sample rate than the engine's.
//! - [`Overview`] is what a waveform of it draws, made from the file and never saved.
//! - [`TakeFile`] is a recording on its way in: a WAV file that grows while it records.
//!
//! `README.md` in this crate is the guide.

mod file;
mod overview;
mod resample;
mod take;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sound_core::{ASSETS_FOLDER, AssetName, Assets, InvalidAssetName};

pub use file::{Audio, Container, Encoding, FormatError, Info, SAMPLE_RATES};
pub use overview::{FINEST_FRAMES, Overview};
pub use resample::{Resampler, SCRATCH_FRAMES};
pub use take::{TakeFile, TakeOverview};

/// The folder of audio files, under `assets/`.
pub const AUDIO_FOLDER: &str = "audio";

/// An audio file of the project, as a record names it: its file name under `assets/audio/`,
/// such as `"voice.wav"`.
///
/// It can only hold a name that stays inside that folder: lowercase letters, digits, `-` and
/// `_`, a dot and an extension of the same letters. So a record can never point anywhere
/// else, and a project folder copied to another place plays the same.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AudioAsset(AssetName);

impl AudioAsset {
    /// The file `<name>.<extension>` under `assets/audio/`.
    pub fn new(file_name: &str) -> Result<Self, InvalidAssetName> {
        let (name, extension) = file_name.rsplit_once('.').unwrap_or((file_name, ""));
        AssetName::new(AUDIO_FOLDER, name, extension).map(Self)
    }

    pub fn asset_name(&self) -> &AssetName {
        &self.0
    }

    /// Where the file is in a project, for messages: `assets/audio/voice.wav`.
    pub fn project_path(&self) -> String {
        format!("{ASSETS_FOLDER}/{}", self.0)
    }
}

impl fmt::Display for AudioAsset {
    /// The file name, as a record holds it.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0.as_str();
        let file = whole.strip_prefix(AUDIO_FOLDER).unwrap_or(&whole);
        formatter.write_str(file.trim_start_matches('/'))
    }
}

impl TryFrom<String> for AudioAsset {
    type Error = String;

    fn try_from(file_name: String) -> Result<Self, String> {
        Self::new(&file_name).map_err(|_| {
            format!(
                "{file_name:?} is no file name under assets/audio/: write it as `name.wav`, with lowercase letters, digits, `-` and `_` in the name and the extension"
            )
        })
    }
}

impl From<AudioAsset> for String {
    fn from(asset: AudioAsset) -> String {
        asset.to_string()
    }
}

/// Why a file could not be read or imported.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("{path} is not there")]
    Missing { path: String },
    #[error("{path} cannot be read: {source}")]
    Io { path: String, source: io::Error },
    #[error("{path} cannot be played: {source}")]
    Format { path: String, source: FormatError },
}

/// What is known of a file, by path, while its size and modification time stay the same. One
/// small entry per file a project ever named, for as long as the process runs.
///
/// A file that plays is shared by everything that plays it and let go of when the last of
/// them lets go: the cache holds no file alive, but it keeps what the file is, its [`Info`],
/// so asking how long it is costs one look at its size and time. A file that does not play
/// is kept as its error, so asking again reads nothing, and a file that was not there as that.
/// A file whose size or modification time changed is read again.
///
/// The lock is held only to look up and to put in, never while a file is read, so a look from
/// the thread that draws ([`cached`]) never waits for a read.
struct Known {
    length: u64,
    modified: Option<SystemTime>,
    outcome: Outcome,
}

enum Outcome {
    Plays {
        info: Info,
        audio: Weak<Audio>,
    },
    DoesNotPlay(FormatError),
    /// The file was not there when it was last looked for.
    Missing,
}

static KNOWN: LazyLock<Mutex<HashMap<PathBuf, Known>>> = LazyLock::new(Mutex::default);

fn known() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Known>> {
    KNOWN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The size and modification time of a file, or why there are none. A file that is not there
/// is remembered as that, for [`cached`].
fn stat(path: &Path, shown: &str) -> Result<(u64, Option<SystemTime>), MediaError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok((metadata.len(), metadata.modified().ok())),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            let missing = Known {
                length: 0,
                modified: None,
                outcome: Outcome::Missing,
            };
            known().insert(path.to_path_buf(), missing);
            Err(MediaError::Missing {
                path: shown.to_string(),
            })
        }
        Err(source) => Err(MediaError::Io {
            path: shown.to_string(),
            source,
        }),
    }
}

fn format_error(shown: &str, source: FormatError) -> MediaError {
    MediaError::Format {
        path: shown.to_string(),
        source,
    }
}

/// Reads and parses a whole file, with no lock held, and keeps what came of it.
fn read(
    path: &Path,
    shown: &str,
    (length, modified): (u64, Option<SystemTime>),
) -> Result<Arc<Audio>, MediaError> {
    // A file that cannot be read is not kept: that may pass, as a file that is still copied.
    let bytes = fs::read(path).map_err(|source| MediaError::Io {
        path: shown.to_string(),
        source,
    })?;
    let (outcome, result) = match Audio::parse(bytes) {
        Ok(audio) => {
            let audio = Arc::new(audio);
            let outcome = Outcome::Plays {
                info: audio.info(),
                audio: Arc::downgrade(&audio),
            };
            (outcome, Ok(audio))
        }
        Err(error) => (
            Outcome::DoesNotPlay(error.clone()),
            Err(format_error(shown, error)),
        ),
    };
    let entry = Known {
        length,
        modified,
        outcome,
    };
    known().insert(path.to_path_buf(), entry);
    result
}

/// The file a record names, in memory. Read from disk the first time, then shared.
///
/// For the control side only: it may read a file. What it gives is safe to hand to the audio
/// thread inside a snapshot.
pub fn load(assets: &Assets, asset: &AudioAsset) -> Result<Arc<Audio>, MediaError> {
    let path = assets.path(asset.asset_name());
    let shown = asset.project_path();
    let stamp = stat(&path, &shown)?;
    if let Some(entry) = known().get(&path)
        && (entry.length, entry.modified) == stamp
    {
        match &entry.outcome {
            Outcome::Plays { audio, .. } => {
                if let Some(audio) = audio.upgrade() {
                    return Ok(audio);
                }
            }
            Outcome::DoesNotPlay(error) => return Err(format_error(&shown, error.clone())),
            Outcome::Missing => {}
        }
    }
    read(&path, &shown, stamp)
}

/// What the file a record names is: how many frames at what rate. It reads the header of the
/// file the first time, never the samples, and after that costs one look at the size and time
/// of the file, whether anything holds the file or not.
pub fn info(assets: &Assets, asset: &AudioAsset) -> Result<Info, MediaError> {
    let path = assets.path(asset.asset_name());
    let shown = asset.project_path();
    let stamp = stat(&path, &shown)?;
    if let Some(entry) = known().get(&path)
        && (entry.length, entry.modified) == stamp
    {
        match &entry.outcome {
            Outcome::Plays { info, .. } => return Ok(*info),
            Outcome::DoesNotPlay(error) => return Err(format_error(&shown, error.clone())),
            Outcome::Missing => {}
        }
    }
    let probed = file::probe(&path).map_err(|error| match error {
        file::ProbeError::Io(source) => MediaError::Io {
            path: shown.clone(),
            source,
        },
        file::ProbeError::Format(source) => format_error(&shown, source),
    });
    let outcome = match &probed {
        Ok(info) => Outcome::Plays {
            info: *info,
            audio: Weak::new(),
        },
        Err(MediaError::Format { source, .. }) => Outcome::DoesNotPlay(source.clone()),
        // A file that cannot be read now may be read later, as one that is still copied.
        Err(_) => return probed,
    };
    let entry = Known {
        length: stamp.0,
        modified: stamp.1,
        outcome,
    };
    known().insert(path, entry);
    probed
}

/// What is known of a file a record names, from memory only: no look at the disk at all, for
/// the thread that draws. [`Cached::Unknown`] until [`info`] or [`load`] has looked at it,
/// which the control side does when a clip names the file, and a view can ask a background
/// thread to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Cached {
    Plays(Info),
    DoesNotPlay(FormatError),
    Missing,
    Unknown,
}

pub fn cached(assets: &Assets, asset: &AudioAsset) -> Cached {
    let path = assets.path(asset.asset_name());
    match known().get(&path).map(|entry| &entry.outcome) {
        Some(Outcome::Plays { info, .. }) => Cached::Plays(*info),
        Some(Outcome::DoesNotPlay(error)) => Cached::DoesNotPlay(error.clone()),
        Some(Outcome::Missing) => Cached::Missing,
        None => Cached::Unknown,
    }
}

/// What a file anywhere is, from its header alone: how long a drop of it would be.
pub fn probe(path: &Path) -> Result<Info, MediaError> {
    let shown = path.display().to_string();
    file::probe(path).map_err(|error| match error {
        file::ProbeError::Io(source) if source.kind() == io::ErrorKind::NotFound => {
            MediaError::Missing { path: shown }
        }
        file::ProbeError::Io(source) => MediaError::Io {
            path: shown,
            source,
        },
        file::ProbeError::Format(source) => format_error(&shown, source),
    })
}

/// How many engine frames `file_frames` of a file play, at the file's own speed: every frame
/// whose place in the file is still inside them.
pub fn engine_frames(file_frames: u64, file_rate: u32, engine_rate: u32) -> u64 {
    let frames = u128::from(file_frames) * u128::from(engine_rate.max(1));
    u64::try_from(frames.div_ceil(u128::from(file_rate.max(1)))).unwrap_or(u64::MAX)
}

static RESAMPLERS: LazyLock<Mutex<HashMap<(u32, u32), Weak<Resampler>>>> =
    LazyLock::new(Mutex::default);

/// The filter from one rate to another, made once and shared while anything uses it.
pub fn resampler(file_rate: u32, engine_rate: u32) -> Arc<Resampler> {
    let mut made = RESAMPLERS.lock().unwrap_or_else(PoisonError::into_inner);
    let key = (file_rate, engine_rate);
    if let Some(resampler) = made.get(&key).and_then(Weak::upgrade) {
        return resampler;
    }
    let resampler = Arc::new(Resampler::new(file_rate, engine_rate));
    made.retain(|_, resampler| resampler.strong_count() > 0);
    made.insert(key, Arc::downgrade(&resampler));
    resampler
}

/// Copies an audio file into `assets/audio/` and gives the name a record uses for it.
///
/// The file is read and checked first, so what lands in the project always plays. Its name
/// comes from the file name, in the letters an asset name allows: `My Take (2).WAV` becomes
/// `my-take-2.wav`. When that name is taken it becomes `my-take-2-2.wav`, then `-3`
/// ([`Assets::reserve`]), and a file that is already there is never written over. The copy is
/// written under a temporary name and renamed over the name it reserved, so a half-written
/// file never has the name, and it works on every file system, FAT and network shares too.
/// Nothing is left behind when it fails.
///
/// What it read comes back with the name, as the file in memory. While the caller holds it,
/// the first [`load`] of it reads nothing; the cache itself holds it weakly, so a copy that
/// never becomes a clip does not stay in memory.
pub fn import(assets: &Assets, source: &Path) -> Result<Imported, MediaError> {
    let shown = source.display().to_string();
    let bytes = fs::read(source).map_err(|source| match source.kind() {
        io::ErrorKind::NotFound => MediaError::Missing {
            path: shown.clone(),
        },
        _ => MediaError::Io {
            path: shown.clone(),
            source,
        },
    })?;
    let audio = Audio::parse(bytes).map_err(|source| MediaError::Format {
        path: shown.clone(),
        source,
    })?;
    let stem = source.file_stem().map(|stem| stem.to_string_lossy());
    let stem = asset_part(stem.as_deref().unwrap_or_default(), "audio");
    let extension = source
        .extension()
        .map(|extension| extension.to_string_lossy());
    let extension = asset_part(
        extension.as_deref().unwrap_or_default(),
        audio.container().extension(),
    );
    let wanted = AssetName::new(AUDIO_FOLDER, &stem, &extension).map_err(invalid)?;
    let io_error = |path: &str, source| MediaError::Io {
        path: path.to_string(),
        source,
    };

    // The temporary file, next to where the copy goes, so the rename stays on one disk. It
    // starts with a dot, which no asset name can, so it is never taken for one.
    let folder = audio_folder(assets)?;
    fs::create_dir_all(&folder).map_err(|source| io_error(AUDIO_FOLDER, source))?;
    static IMPORTS: AtomicU64 = AtomicU64::new(0);
    let count = IMPORTS.fetch_add(1, Ordering::Relaxed);
    let temporary = folder.join(format!(".import-{}-{count}.tmp", std::process::id()));
    let written = fs::write(&temporary, audio.file_bytes());
    let reserved = written
        .map_err(|source| io_error(&shown, source))
        .and_then(|()| {
            assets
                .reserve(&wanted)
                .map_err(|error| io_error(AUDIO_FOLDER, io::Error::other(error)))
        });
    let asset = match reserved {
        Ok(reserved) => AudioAsset(reserved),
        Err(error) => {
            remove_quietly(&temporary);
            return Err(error);
        }
    };
    let target = assets.path(asset.asset_name());
    if let Err(source) = fs::rename(&temporary, &target) {
        remove_quietly(&temporary);
        // The empty file that held the name is ours.
        remove_quietly(&target);
        return Err(io_error(&asset.project_path(), source));
    }

    let audio = Arc::new(audio);
    if let Ok(stamp) = stat(&target, &asset.project_path()) {
        let entry = Known {
            length: stamp.0,
            modified: stamp.1,
            outcome: Outcome::Plays {
                info: audio.info(),
                audio: Arc::downgrade(&audio),
            },
        };
        known().insert(target, entry);
    }
    Ok(Imported { asset, audio })
}

/// A file copied into `assets/audio/`: the name a record gives it, and the file in memory.
/// Hold it until the clip that names it is added, so the track that plays it reads nothing.
#[derive(Debug)]
pub struct Imported {
    pub asset: AudioAsset,
    pub audio: Arc<Audio>,
}

/// Removes a file this module made, on the way out of a failure that is already reported.
fn remove_quietly(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        // The failure that brought us here is the one to report. A file left behind starts
        // with a dot or is empty, and is no asset a record can name by accident.
        Err(_) => {}
    }
}

fn invalid(error: InvalidAssetName) -> MediaError {
    MediaError::Io {
        path: AUDIO_FOLDER.to_string(),
        source: io::Error::new(io::ErrorKind::InvalidInput, error),
    }
}

/// `assets/audio/` of a project.
fn audio_folder(assets: &Assets) -> Result<PathBuf, MediaError> {
    let any = AssetName::new(AUDIO_FOLDER, "any", "wav").map_err(invalid)?;
    let path = assets.path(&any);
    Ok(path.parent().map(Path::to_path_buf).unwrap_or_default())
}

/// A part of an asset name from any text: lowercase letters and digits, `-` for the rest.
fn asset_part(text: &str, fallback: &str) -> String {
    let mut part = String::new();
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_' {
            part.push(character);
        } else if !part.is_empty() && !part.ends_with('-') {
            part.push('-');
        }
    }
    let part = part.trim_end_matches('-');
    match part.is_empty() {
        true => fallback.to_string(),
        false => part.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_names_a_file_under_assets_audio_and_nothing_else() {
        let asset: AudioAsset = serde_json::from_str(r#""voice-take_2.wav""#).unwrap();
        assert_eq!(asset.to_string(), "voice-take_2.wav");
        assert_eq!(asset.project_path(), "assets/audio/voice-take_2.wav");
        assert_eq!(
            serde_json::to_string(&asset).unwrap(),
            r#""voice-take_2.wav""#
        );
        for wrong in [
            "../voice.wav",
            "Voice.wav",
            "voice",
            "a/b.wav",
            "voice.",
            ".wav",
        ] {
            let parsed = serde_json::from_str::<AudioAsset>(&format!("{wrong:?}"));
            assert!(parsed.is_err(), "{wrong} was taken");
        }
    }

    #[test]
    fn a_file_name_becomes_an_asset_name() {
        assert_eq!(asset_part("My Take (2)", "audio"), "my-take-2");
        assert_eq!(asset_part("WAV", "wav"), "wav");
        assert_eq!(asset_part("???", "audio"), "audio");
        assert_eq!(asset_part("Kick_01", "audio"), "kick_01");
    }
}
