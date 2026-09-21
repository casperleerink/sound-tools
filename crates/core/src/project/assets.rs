//! Project assets: files the project owns that are not records.
//!
//! A record is typed state the core reads, writes and undoes. An asset is opaque bytes that
//! belong to an extension: a raw take, the state of a hosted plugin, later a sample. The core
//! never reads inside one. It gives the two safe ways to put bytes in the folder, and one
//! validated name that a record can hold and that can never point outside `assets/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The folder of every asset, under the project folder.
pub const ASSETS_FOLDER: &str = "assets";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid asset name {part:?} in {whole:?}: a part uses lowercase letters, digits, `-` and `_`"
)]
pub struct InvalidAssetName {
    part: String,
    whole: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error(transparent)]
    InvalidName(#[from] InvalidAssetName),
    #[error("{path}: {source}")]
    Io {
        /// Relative to the project folder, for example `assets/takes/take-1.json`.
        path: String,
        source: io::Error,
    },
}

/// Where an asset lives: `<folder>/<name>.<extension>` under `assets/`.
///
/// Every part is checked, so a name that comes out of a record can never reach outside the
/// project folder. A record holds the name as a string, like every other reference.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetName {
    folder: String,
    name: String,
    extension: String,
}

impl AssetName {
    pub fn new(folder: &str, name: &str, extension: &str) -> Result<Self, InvalidAssetName> {
        let whole = format!("{folder}/{name}.{extension}");
        for part in [folder, name, extension] {
            if part.is_empty() || !part.chars().all(is_name_character) {
                return Err(InvalidAssetName {
                    part: part.to_string(),
                    whole,
                });
            }
        }
        Ok(Self {
            folder: folder.to_string(),
            name: name.to_string(),
            extension: extension.to_string(),
        })
    }

    /// The same asset with `-<number>` after the name, for the numbering of [`Assets::create`].
    fn numbered(&self, number: u32) -> Self {
        Self {
            folder: self.folder.clone(),
            name: format!("{}-{number}", self.name),
            extension: self.extension.clone(),
        }
    }

    /// The path under `assets/`, for example `takes/take-1.json`.
    pub fn as_str(&self) -> String {
        format!("{}/{}.{}", self.folder, self.name, self.extension)
    }

    /// The name without the folder and the extension, for example `take-1`.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl std::fmt::Display for AssetName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.as_str())
    }
}

fn is_name_character(character: char) -> bool {
    character.is_ascii_lowercase() || character.is_ascii_digit() || "-_".contains(character)
}

/// The `assets/` folder of one project. Cheap to clone: it is one path.
#[derive(Clone)]
pub struct Assets {
    folder: PathBuf,
}

impl Assets {
    /// The assets of the project in `root`. [`Project::assets`](super::Project::assets) gives
    /// the one of an open project; this is for a test that writes an asset without one.
    pub fn new(root: &Path) -> Self {
        Self {
            folder: root.join(ASSETS_FOLDER),
        }
    }

    pub fn path(&self, name: &AssetName) -> PathBuf {
        self.folder
            .join(&name.folder)
            .join(format!("{}.{}", name.name, name.extension))
    }

    /// The bytes of an asset, or `None` when it is not there.
    pub fn read(&self, name: &AssetName) -> Result<Option<Vec<u8>>, AssetError> {
        let path = self.path(name);
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(self.io_error(name, source)),
        }
    }

    /// Replaces an asset. A temporary file is renamed into place, so a failed write leaves the
    /// file that was there complete, as [`Project::write`](super::Project::write) does for a
    /// record. There is no `fsync`: surviving a power loss is left to git and snapshots.
    pub fn write(&self, name: &AssetName, bytes: &[u8]) -> Result<(), AssetError> {
        let path = self.path(name);
        super::storage::write_atomically(&path, bytes).map_err(|source| self.io_error(name, source))
    }

    /// Writes bytes under the first free name of `<name>-1`, `<name>-2` and so on, and gives
    /// the name it took.
    ///
    /// The file is created and never opened, so nothing that is already there can be lost, also
    /// not by two runtimes at once. Use this for an asset that must never be written over, such
    /// as a recorded performance.
    pub fn create(&self, name: &AssetName, bytes: &[u8]) -> Result<AssetName, AssetError> {
        let folder = self.folder.join(&name.folder);
        if let Err(source) = fs::create_dir_all(&folder) {
            return Err(self.io_error(name, source));
        }
        let mut number = 1_u32;
        loop {
            let candidate = name.numbered(number);
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.path(&candidate));
            match file {
                Ok(mut file) => {
                    use std::io::Write as _;
                    return match file.write_all(bytes) {
                        Ok(()) => Ok(candidate),
                        Err(source) => Err(self.io_error(&candidate, source)),
                    };
                }
                // Taken, by this session or an earlier one. It is never written to.
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => number += 1,
                Err(source) => return Err(self.io_error(&candidate, source)),
            }
        }
    }

    fn io_error(&self, name: &AssetName, source: io::Error) -> AssetError {
        AssetError::Io {
            path: format!("{ASSETS_FOLDER}/{name}"),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> (tempfile::TempDir, Assets) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let assets = Assets::new(folder.path());
        (folder, assets)
    }

    #[test]
    fn a_name_cannot_reach_outside_the_assets_folder() {
        for (folder, name, extension) in [
            ("..", "take-1", "json"),
            ("takes", "../../secret", "json"),
            ("takes", "take-1", "js/on"),
            ("takes", "", "json"),
            ("takes", "Take-1", "json"),
        ] {
            assert!(
                AssetName::new(folder, name, extension).is_err(),
                "{folder}/{name}.{extension} was accepted"
            );
        }
        let name = AssetName::new("takes", "take-1", "json").expect("a valid name");
        assert_eq!(name.as_str(), "takes/take-1.json");
        assert_eq!(name.name(), "take-1");
    }

    #[test]
    fn writing_replaces_and_reading_a_missing_asset_is_none() {
        let (folder, assets) = assets();
        let name = AssetName::new("plugin-state", "piano", "clap").expect("a valid name");
        assert_eq!(assets.read(&name).expect("a read"), None);
        assets.write(&name, b"first").expect("a write");
        assets.write(&name, b"second").expect("a write");
        assert_eq!(
            assets.read(&name).expect("a read").as_deref(),
            Some(&b"second"[..])
        );
        let path = folder.path().join("assets/plugin-state/piano.clap");
        assert!(path.exists());
        assert!(
            !folder
                .path()
                .join("assets/plugin-state/piano.clap.tmp")
                .exists()
        );
    }

    #[test]
    fn creating_never_writes_over_an_asset_that_is_there() {
        let (_folder, assets) = assets();
        let take = AssetName::new("takes", "take", "json").expect("a valid name");
        let first = assets.create(&take, b"one").expect("a take");
        let second = assets.create(&take, b"two").expect("a take");
        assert_eq!(first.as_str(), "takes/take-1.json");
        assert_eq!(second.as_str(), "takes/take-2.json");
        assert_eq!(
            assets.read(&first).expect("a read").as_deref(),
            Some(&b"one"[..])
        );
    }
}
