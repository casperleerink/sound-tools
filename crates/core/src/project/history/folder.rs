//! The files of the project folder that history keeps: `project.json`, `state/` and the
//! project's own tool code in `extensions/`. Assets are left out: they are never written over.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::git::{Files, Repository};

pub(crate) const EXTENSIONS_FOLDER: &str = "extensions";
const KEPT: [&str; 3] = ["project.json", "state", EXTENSIONS_FOLDER];

/// What history leaves out of the folder: hidden files anywhere, such as an editor's, Bun's
/// `node_modules`, and what the runtime writes at the top of `extensions/`, named in `skipped`.
pub(crate) struct Kept {
    pub skipped: Vec<&'static str>,
}

impl Kept {
    /// Whether `relative`, a path inside the project folder, is or holds a file history keeps.
    pub(crate) fn holds(&self, relative: &Path) -> bool {
        let Some(top) = relative.components().next().map(|part| part.as_os_str()) else {
            // The project folder itself.
            return true;
        };
        KEPT.iter().any(|kept| top.to_str() == Some(kept))
            && !self.skips(&relative.to_string_lossy().replace('\\', "/"))
    }

    fn skips(&self, path: &str) -> bool {
        let mut names = path.split('/');
        let top = names.next();
        let names: Vec<&str> = names.collect();
        let hidden = (names.iter()).any(|name| name.starts_with('.') || *name == "node_modules");
        let generated = top == Some(EXTENSIONS_FOLDER)
            && names.first().is_some_and(|first| self.skipped.contains(first));
        hidden || generated
    }

    /// Reads every kept file and writes its blob, so the folder can be one tree.
    pub(crate) fn read(&self, root: &Path, repository: &Repository) -> io::Result<Files> {
        let mut files = Files::new();
        for top in KEPT {
            self.read_into(root, Path::new(top), repository, &mut files)?;
        }
        Ok(files)
    }

    fn read_into(
        &self,
        root: &Path,
        relative: &Path,
        repository: &Repository,
        files: &mut Files,
    ) -> io::Result<()> {
        let Some(path) = relative.to_str().map(|path| path.replace('\\', "/")) else {
            return Ok(());
        };
        if self.skips(&path) {
            return Ok(());
        }
        let full = root.join(relative);
        let kind = match fs::symlink_metadata(&full) {
            Ok(metadata) => metadata.file_type(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if kind.is_file() {
            // A file deleted between the listing and the read is no file.
            match fs::read(&full) {
                Ok(bytes) => _ = files.insert(path, repository.write_blob(&bytes)?),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        } else if kind.is_dir() {
            for entry in fs::read_dir(&full)? {
                self.read_into(root, &relative.join(entry?.file_name()), repository, files)?;
            }
        }
        Ok(())
    }
}

/// Makes the kept files of the folder, which are `now`, the files `wanted`, byte for byte, and
/// removes folders that end up empty. Returns every path it wrote or removed.
pub(crate) fn write(
    root: &Path,
    repository: &Repository,
    now: &Files,
    wanted: &Files,
) -> io::Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for (path, blob) in wanted {
        if now.get(path) != Some(blob) {
            let full = root.join(path);
            if let Some(folder) = full.parent() {
                fs::create_dir_all(folder)?;
            }
            super::super::storage::write_atomically(&full, &repository.read_blob(*blob)?)?;
            changed.push(full);
        }
    }
    for path in now.keys().filter(|path| !wanted.contains_key(*path)) {
        let full = root.join(path);
        match fs::remove_file(&full) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        // Up to the kept folder at the top, while empty.
        let mut folder = full.parent();
        while let Some(inside) = folder.filter(|folder| folder.starts_with(root) && folder != &root)
        {
            if inside.parent() == Some(root) || fs::remove_dir(inside).is_err() {
                break;
            }
            folder = inside.parent();
        }
        changed.push(full);
    }
    Ok(changed)
}
