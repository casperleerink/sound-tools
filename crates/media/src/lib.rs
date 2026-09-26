//! Media: the audio files of a project, and how to read them.
//!
//! No extension: audio clips, the Sampler and the Drum pad all read files through this crate,
//! and none of them depends on another. The core knows no audio files; to it a file under
//! `assets/audio/` is an asset like any other.
//!
//! - [`AudioAsset`] is how a record names a file: `"voice.wav"`, which is
//!   `assets/audio/voice.wav`. Never a path outside the project.
//! - [`import`] copies a file into `assets/audio/` under a free name.
//! - [`load`] gives the file in memory, [`Audio`], shared by everything that plays it.
//! - [`Resampler`] plays it at another sample rate than the engine's.
//!
//! `README.md` in this crate is the guide.

mod file;
mod resample;

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sound_core::{ASSETS_FOLDER, AssetName, Assets, InvalidAssetName};

pub use file::{Audio, Container, Encoding, FormatError, SAMPLE_RATES};
pub use resample::{Resampler, SCRATCH_FRAMES};

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

/// Files that are in memory, by path. Each is read once and shared by everything that plays
/// it, and let go of when the last of them lets go: the cache holds no file alive.
///
/// A file whose size or modification time changed is read again, so a file replaced under
/// the same name plays at the next change of a record that names it.
struct Loaded {
    audio: Weak<Audio>,
    length: u64,
    modified: Option<SystemTime>,
}

static LOADED: LazyLock<Mutex<HashMap<PathBuf, Loaded>>> = LazyLock::new(Mutex::default);

/// The file a record names, in memory. Read from disk the first time, then shared.
///
/// For the control side only: it reads a file. What it gives is safe to hand to the audio
/// thread inside a snapshot.
pub fn load(assets: &Assets, asset: &AudioAsset) -> Result<Arc<Audio>, MediaError> {
    let path = assets.path(asset.asset_name());
    let error_path = asset.project_path();
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Err(MediaError::Missing { path: error_path });
        }
        Err(source) => {
            return Err(MediaError::Io {
                path: error_path,
                source,
            });
        }
    };
    let (length, modified) = (metadata.len(), metadata.modified().ok());
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(entry) = loaded.get(&path)
        && (entry.length, entry.modified) == (length, modified)
        && let Some(audio) = entry.audio.upgrade()
    {
        return Ok(audio);
    }
    let bytes = fs::read(&path).map_err(|source| MediaError::Io {
        path: error_path.clone(),
        source,
    })?;
    let audio = Arc::new(Audio::parse(bytes).map_err(|source| MediaError::Format {
        path: error_path,
        source,
    })?);
    loaded.retain(|_, entry| entry.audio.strong_count() > 0);
    let entry = Loaded {
        audio: Arc::downgrade(&audio),
        length,
        modified,
    };
    loaded.insert(path, entry);
    Ok(audio)
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
/// `my-take-2.wav`. When that name is taken it becomes `my-take-2-2.wav`, then `-3`, and a
/// file that is already there is never written over. The copy is written under a temporary
/// name first and linked into place, so a half-written file never has the name.
pub fn import(assets: &Assets, source: &Path) -> Result<AudioAsset, MediaError> {
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

    let folder = audio_folder(assets)?;
    let io_error = |path: &str, source| MediaError::Io {
        path: path.to_string(),
        source,
    };
    fs::create_dir_all(&folder).map_err(|source| io_error(AUDIO_FOLDER, source))?;
    // The temporary file starts with a dot, which no asset name can, so it is never one.
    static IMPORTS: AtomicU64 = AtomicU64::new(0);
    let count = IMPORTS.fetch_add(1, Ordering::Relaxed);
    let temporary = folder.join(format!(".import-{}-{count}.tmp", std::process::id()));
    let mut file = fs::File::create(&temporary).map_err(|source| io_error(&shown, source))?;
    file.write_all(audio.file_bytes())
        .map_err(|source| io_error(&shown, source))?;
    drop(file);

    let mut number = 1_u32;
    let result = loop {
        let name = match number {
            1 => stem.clone(),
            _ => format!("{stem}-{number}"),
        };
        let asset = AudioAsset(AssetName::new(AUDIO_FOLDER, &name, &extension).map_err(invalid)?);
        // A hard link fails when the name is taken, so nothing is ever written over.
        match fs::hard_link(&temporary, assets.path(asset.asset_name())) {
            Ok(()) => break Ok(asset),
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => number += 1,
            Err(source) => break Err(io_error(&asset.project_path(), source)),
        }
    };
    if let Err(source) = fs::remove_file(&temporary) {
        // The copy is in place; a temporary file left behind is harmless.
        if result.is_err() {
            return Err(io_error(&shown, source));
        }
    }
    result
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
