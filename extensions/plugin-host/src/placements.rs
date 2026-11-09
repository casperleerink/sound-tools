//! Where the plugin windows of each project were on this machine, and whether they were open.
//!
//! A window's position belongs to this machine and its displays, not to the piece, so it is
//! kept next to the scan cache and not in the project folder: a project in git must not change
//! because a window moved. One file holds every project, by the path of its folder.
//!
//! The file is read again just before a write, so a runtime writes only its own project's entry
//! over what another one wrote. A file that cannot be read is written over.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sound_core::InstanceId;

use crate::window::Placement;

pub(crate) const FILE: &str = "plugin-windows.json";

pub(crate) type Placements = BTreeMap<InstanceId, Placement>;

type Projects = BTreeMap<String, Placements>;

/// What a store with no file keeps, shared by every clone of its scan cache.
pub(crate) type Kept = Arc<Mutex<Projects>>;

/// A file, or memory for a test.
pub(crate) struct PlacementStore {
    path: Option<PathBuf>,
    kept: Kept,
}

impl PlacementStore {
    pub(crate) fn new(path: Option<PathBuf>, kept: Kept) -> Self {
        Self { path, kept }
    }

    /// A write a crash left behind is taken away on the way; one that cannot be is the error.
    pub(crate) fn read(&self, project: &Path) -> Result<Placements, String> {
        let key = key(project);
        let Some(path) = &self.path else {
            let kept = self
                .kept
                .lock()
                .map_err(|_| "a writer panicked".to_string())?;
            return Ok(kept.get(&key).cloned().unwrap_or_default());
        };
        crate::scan::remove_stale_writes(path).map_err(|errors| {
            format!("what an earlier write of {FILE} left could not be removed: {errors}")
        })?;
        Ok(read_file(path).remove(&key).unwrap_or_default())
    }

    /// Nothing is written when the file would not change.
    pub(crate) fn write(&self, project: &Path, placements: &Placements) -> Result<(), String> {
        let key = key(project);
        let Some(path) = &self.path else {
            let mut kept = self
                .kept
                .lock()
                .map_err(|_| "a writer panicked".to_string())?;
            set(&mut kept, key, placements);
            return Ok(());
        };
        let before = read_file(path);
        let mut after = before.clone();
        set(&mut after, key, placements);
        if after == before {
            return Ok(());
        }
        let failed = |error: String| format!("{} was not written: {error}", path.display());
        let mut text =
            serde_json::to_string_pretty(&after).map_err(|error| failed(error.to_string()))?;
        text.push('\n');
        crate::scan::write_whole(path, text.as_bytes()).map_err(failed)
    }
}

/// The same however the folder was opened.
fn key(project: &Path) -> String {
    let path = dunce::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    path.to_string_lossy().into_owned()
}

fn set(projects: &mut Projects, key: String, placements: &Placements) {
    if placements.is_empty() {
        projects.remove(&key);
    } else {
        projects.insert(key, placements.clone());
    }
}

fn read_file(path: &Path) -> Projects {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Projects::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement(open: bool, x: i32) -> Placement {
        Placement {
            open,
            x,
            y: 40,
            display: None,
        }
    }

    fn id(text: &str) -> InstanceId {
        InstanceId::new(text).expect("an id")
    }

    fn store(folder: &Path) -> PlacementStore {
        PlacementStore::new(Some(folder.join(FILE)), Kept::default())
    }

    #[test]
    fn two_projects_keep_their_own_windows_in_one_file() {
        let folder = tempfile::tempdir().expect("a folder");
        let (night, day) = (folder.path().join("night"), folder.path().join("day"));
        std::fs::create_dir_all(&night).expect("a project");
        std::fs::create_dir_all(&day).expect("a project");
        let mut first = Placements::new();
        first.insert(id("song/piano/synth"), placement(true, 120));
        let mut second = Placements::new();
        second.insert(id("song/bass/space"), placement(false, 300));
        store(folder.path())
            .write(&night, &first)
            .expect("it writes");
        store(folder.path())
            .write(&day, &second)
            .expect("it writes");
        assert_eq!(store(folder.path()).read(&night).expect("it reads"), first);
        assert_eq!(store(folder.path()).read(&day).expect("it reads"), second);
        // The same folder by another path is the same project.
        let roundabout = night.join("..").join("night");
        assert_eq!(
            store(folder.path()).read(&roundabout).expect("it reads"),
            first
        );
    }

    #[test]
    fn nothing_is_written_when_nothing_changed_or_there_is_nothing_to_keep() {
        let folder = tempfile::tempdir().expect("a folder");
        let path = folder.path().join(FILE);
        store(folder.path())
            .write(folder.path(), &Placements::new())
            .expect("it writes nothing");
        assert!(!path.exists());

        let mut placements = Placements::new();
        placements.insert(id("track/instrument"), placement(true, 1));
        store(folder.path())
            .write(folder.path(), &placements)
            .expect("it writes");
        let written = std::fs::metadata(&path).and_then(|file| file.modified());
        std::thread::sleep(std::time::Duration::from_millis(20));
        store(folder.path())
            .write(folder.path(), &placements)
            .expect("it writes nothing");
        assert_eq!(
            std::fs::metadata(&path)
                .and_then(|file| file.modified())
                .ok(),
            written.ok()
        );
    }

    #[test]
    fn a_file_that_does_not_read_is_written_over() {
        let folder = tempfile::tempdir().expect("a folder");
        let path = folder.path().join(FILE);
        std::fs::write(&path, "[1, 2").expect("a file");
        assert_eq!(
            store(folder.path()).read(folder.path()).expect("it reads"),
            Placements::new()
        );
        let mut placements = Placements::new();
        placements.insert(id("track/instrument"), placement(true, 1));
        store(folder.path())
            .write(folder.path(), &placements)
            .expect("it writes");
        assert_eq!(
            store(folder.path()).read(folder.path()).expect("it reads"),
            placements
        );
    }

    /// A write that a crash ended between its own file and the rename leaves that file. The
    /// next read takes it away once it is old enough to be nobody's write in progress.
    #[test]
    fn a_write_a_crash_left_behind_is_taken_away() {
        let folder = tempfile::tempdir().expect("a folder");
        let old = folder.path().join(format!("{FILE}.999-0.tmp"));
        let young = folder.path().join(format!("{FILE}.999-1.tmp"));
        std::fs::write(&old, "{").expect("a file");
        std::fs::write(&young, "{").expect("a file");
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .and_then(|file| file.set_modified(long_ago))
            .expect("an old file");
        store(folder.path()).read(folder.path()).expect("it reads");
        assert!(!old.exists());
        assert!(young.exists());
    }
}
