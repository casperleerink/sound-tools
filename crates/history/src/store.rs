//! The history of one project folder: every state it was in, as git commits in a repository
//! of its own inside the folder, with where the project is now.
//!
//! A point is saved by looking at the folder, not by being told what changed: the folder is
//! the truth. So undo and redo need no hook. The state after an undo is one the history
//! already has, near where it is now, and it moves there instead of saving a new point.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::git::{Commit, Entry, GitError, ObjectId, Repository};

/// Where the history lives, inside the project folder. Hidden, and outside every folder the
/// runtime or an agent reads.
pub const HISTORY_FOLDER: &str = ".sound-tools/history";

/// The state of the history that git has no place for: which points are moments, the names
/// of versions, and where the project was lately.
const META_FILE: &str = "sound-tools.json";

/// How many places the project was at lately are remembered, so that undoing a jump goes
/// back to where it came from instead of saving a new point there.
const RECENT: usize = 32;

/// How far from where the project is a state is looked for before it is a new point.
const NEAR: usize = 64;

const TIME_TRAILER: &str = "Sound-Tools-Time: ";

/// What of the folder the history keeps. Paths are relative to the project folder.
#[derive(Clone, Debug)]
pub struct Scope {
    /// Files and folders that are part of the piece.
    pub roots: Vec<PathBuf>,
    /// Whether a path inside them is not, such as what the runtime writes itself. Paths have
    /// forward slashes.
    pub skip: fn(&str) -> bool,
}

/// One saved state of the folder.
#[derive(Clone, Debug)]
pub struct Point {
    pub id: ObjectId,
    pub parents: Vec<ObjectId>,
    /// Milliseconds since 1970.
    pub time: u64,
    /// The undo step that led here, such as "Change frequency".
    pub label: String,
    tree: ObjectId,
}

/// What [`History::record`] did.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Recorded {
    /// The folder is as the point it is at.
    Unchanged,
    /// The folder is as a point near it, such as after an undo, and it moved there.
    Moved(ObjectId),
    New(ObjectId),
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("no such point in the history")]
    UnknownPoint,
}

#[derive(Default, Serialize, Deserialize)]
struct Meta {
    /// Points that are moments, with their name.
    #[serde(default)]
    moments: BTreeMap<String, String>,
    /// Names of versions, by their first point.
    #[serde(default)]
    names: BTreeMap<String, String>,
    /// Versions set aside by hand, by their first point.
    #[serde(default)]
    aside: BTreeSet<String>,
    /// Where the project was lately, newest last.
    #[serde(default)]
    recent: Vec<String>,
}

/// A file as last seen, so an unchanged one is not read again.
#[derive(Clone, PartialEq, Eq)]
struct Seen {
    length: u64,
    modified: Option<SystemTime>,
    blob: ObjectId,
}

pub struct History {
    repository: Repository,
    root: PathBuf,
    scope: Scope,
    points: BTreeMap<ObjectId, Point>,
    /// Every point, parents before children, older before newer.
    order: Vec<ObjectId>,
    now: Option<ObjectId>,
    meta: Meta,
    seen: BTreeMap<PathBuf, Seen>,
    /// The tip of a version being merged in: the next point gets it as a second parent.
    merging: Option<ObjectId>,
}

impl History {
    /// Opens the history of the project folder `root`, making it when there is none.
    pub fn open(root: &Path, scope: Scope) -> Result<Self, HistoryError> {
        let repository = Repository::open(&root.join(HISTORY_FOLDER))?;
        let meta = match repository.read_file(Path::new(META_FILE))? {
            // A damaged file loses names and moments, not points.
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => Meta::default(),
        };
        let now = repository.read_ref("refs/heads/main")?;
        let mut history = Self {
            repository,
            root: root.to_path_buf(),
            scope,
            points: BTreeMap::new(),
            order: Vec::new(),
            now,
            meta,
            seen: BTreeMap::new(),
            merging: None,
        };
        history.load()?;
        Ok(history)
    }

    fn load(&mut self) -> Result<(), HistoryError> {
        let mut waiting: Vec<ObjectId> = Vec::new();
        for name in self.repository.refs_in("refs/tips")? {
            waiting.extend(self.repository.read_ref(&name)?);
        }
        waiting.extend(self.now);
        while let Some(id) = waiting.pop() {
            if self.points.contains_key(&id) {
                continue;
            }
            let commit = self.repository.read_commit(id)?;
            waiting.extend(commit.parents.iter().copied());
            self.points.insert(id, point(id, commit));
        }
        self.order = topological(&self.points);
        Ok(())
    }

    /// Where the project is now. `None` before the first point.
    pub fn now(&self) -> Option<ObjectId> {
        self.now
    }

    pub fn point(&self, id: ObjectId) -> Option<&Point> {
        self.points.get(&id)
    }

    /// Every point, parents before children, older before newer.
    pub fn order(&self) -> &[ObjectId] {
        &self.order
    }

    /// The name of the moment at `id`, when it is one.
    pub fn moment(&self, id: ObjectId) -> Option<&str> {
        self.meta.moments.get(&id.to_string()).map(String::as_str)
    }

    /// The name given to the version that starts at `start`.
    pub fn name(&self, start: ObjectId) -> Option<&str> {
        self.meta.names.get(&start.to_string()).map(String::as_str)
    }

    pub fn is_aside(&self, start: ObjectId) -> bool {
        self.meta.aside.contains(&start.to_string())
    }

    /// Looks at the folder and saves it as a point when it is new, named `label`. When it is
    /// a state near where the project is, such as after an undo or a redo, it moves there.
    pub fn record(&mut self, label: &str) -> Result<Recorded, HistoryError> {
        let tree = self.snapshot()?;
        let merging = self.merging.take();
        if merging.is_none() {
            if self
                .now
                .and_then(|now| self.points.get(&now))
                .map(|point| point.tree)
                == Some(tree)
            {
                return Ok(Recorded::Unchanged);
            }
            if let Some(near) = self.near_with(tree) {
                self.move_to(near)?;
                return Ok(Recorded::Moved(near));
            }
        }
        let parents: Vec<ObjectId> = self.now.into_iter().chain(merging).collect();
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        // Git keeps seconds, and a drag makes several points a second: the trailer keeps
        // their order across a reopen.
        let commit = Commit {
            tree,
            parents: parents.clone(),
            time: millis / 1000,
            message: format!("{label}\n\n{TIME_TRAILER}{millis}"),
        };
        let id = self.repository.write_commit(&commit)?;
        self.points.insert(id, point(id, commit));
        self.order.push(id);
        for parent in &parents {
            self.repository.remove_ref(&format!("refs/tips/{parent}"))?;
        }
        self.repository.write_ref(&format!("refs/tips/{id}"), id)?;
        self.move_to(id)?;
        Ok(Recorded::New(id))
    }

    /// Makes the point the project is at a moment, named after what led there. Says whether
    /// it was not one yet.
    pub fn mark_moment(&mut self) -> Result<bool, HistoryError> {
        let Some(now) = self.now else {
            return Ok(false);
        };
        let key = now.to_string();
        if self.meta.moments.contains_key(&key) {
            return Ok(false);
        }
        let label = self.points.get(&now).map(|point| point.label.clone());
        self.meta.moments.insert(key, label.unwrap_or_default());
        self.save_meta()?;
        Ok(true)
    }

    pub fn set_name(&mut self, start: ObjectId, name: &str) -> Result<(), HistoryError> {
        let name = name.trim();
        match name.is_empty() {
            true => self.meta.names.remove(&start.to_string()),
            false => self.meta.names.insert(start.to_string(), name.to_string()),
        };
        self.save_meta()
    }

    pub fn set_aside(&mut self, start: ObjectId, aside: bool) -> Result<(), HistoryError> {
        match aside {
            true => self.meta.aside.insert(start.to_string()),
            false => self.meta.aside.remove(&start.to_string()),
        };
        self.save_meta()
    }

    /// The files to write and remove to bring the folder from where it is now to `target`, and
    /// the project is at `target` from here. Record first, so that the folder is a point.
    pub fn go_to(
        &mut self,
        target: ObjectId,
    ) -> Result<Vec<(PathBuf, Option<Vec<u8>>)>, HistoryError> {
        let to = self
            .points
            .get(&target)
            .ok_or(HistoryError::UnknownPoint)?
            .tree;
        let from = self
            .now
            .and_then(|now| self.points.get(&now))
            .map(|point| point.tree);
        let plan = self.difference(from, to)?;
        self.move_to(target)?;
        Ok(plan)
    }

    /// The files to write and remove to merge the version whose latest point is `tip` into
    /// where the project is now. A file only `tip` changed since the two split comes over. A
    /// file both changed comes over too, unless `keep_mine`. The next point is the merge, with
    /// `tip` as its second parent.
    pub fn merge(
        &mut self,
        tip: ObjectId,
        keep_mine: bool,
    ) -> Result<Vec<(PathBuf, Option<Vec<u8>>)>, HistoryError> {
        let now = self.now.ok_or(HistoryError::UnknownPoint)?;
        let theirs = self
            .points
            .get(&tip)
            .ok_or(HistoryError::UnknownPoint)?
            .tree;
        let ours = self
            .points
            .get(&now)
            .ok_or(HistoryError::UnknownPoint)?
            .tree;
        let base = self.merge_base(now, tip);
        let base = base
            .and_then(|base| self.points.get(&base))
            .map(|point| point.tree);
        let (base, ours_files, theirs_files) = (
            self.files(base)?,
            self.files(Some(ours))?,
            self.files(Some(theirs))?,
        );
        let paths: BTreeSet<&PathBuf> = base
            .keys()
            .chain(ours_files.keys())
            .chain(theirs_files.keys())
            .collect();
        let mut plan = Vec::new();
        for path in paths {
            let (old, mine, their) = (base.get(path), ours_files.get(path), theirs_files.get(path));
            if their == old || their == mine {
                continue;
            }
            if keep_mine && mine != old {
                continue;
            }
            let bytes = match their {
                Some(blob) => Some(self.repository.read_blob(*blob)?),
                None => None,
            };
            plan.push((path.clone(), bytes));
        }
        self.merging = Some(tip);
        Ok(plan)
    }

    /// The newest point both `one` and `other` come from.
    fn merge_base(&self, one: ObjectId, other: ObjectId) -> Option<ObjectId> {
        let ancestors = self.ancestors(one);
        let theirs = self.ancestors(other);
        let position = |id: &ObjectId| self.order.iter().position(|each| each == id);
        ancestors
            .intersection(&theirs)
            .max_by_key(|id| position(id))
            .copied()
    }

    fn ancestors(&self, from: ObjectId) -> BTreeSet<ObjectId> {
        let mut found = BTreeSet::new();
        let mut waiting = vec![from];
        while let Some(id) = waiting.pop() {
            if found.insert(id)
                && let Some(point) = self.points.get(&id)
            {
                waiting.extend(point.parents.iter().copied());
            }
        }
        found
    }

    /// A point near where the project is whose state is `tree`: one it was at lately, one it
    /// came from, or one that came from it.
    fn near_with(&self, tree: ObjectId) -> Option<ObjectId> {
        let matches = |id: &ObjectId| self.points.get(id).is_some_and(|point| point.tree == tree);
        let recent = self.meta.recent.iter().rev();
        if let Some(found) = recent.filter_map(|hex| ObjectId::parse(hex)).find(matches) {
            return Some(found);
        }
        let now = self.now?;
        let mut up = Some(now);
        for _ in 0..NEAR {
            let point = self.points.get(&up?)?;
            up = point.parents.first().copied();
            if let Some(parent) = up.filter(matches) {
                return Some(parent);
            }
        }
        let children = self.children();
        let mut waiting = VecDeque::from([now]);
        let mut looked = 0;
        while let Some(id) = waiting.pop_front() {
            looked += 1;
            if looked > NEAR {
                break;
            }
            for child in children.get(&id).into_iter().flatten() {
                if matches(child) {
                    return Some(*child);
                }
                waiting.push_back(*child);
            }
        }
        None
    }

    /// The children of every point that has any.
    pub fn children(&self) -> BTreeMap<ObjectId, Vec<ObjectId>> {
        let mut children: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();
        for id in &self.order {
            for parent in self
                .points
                .get(id)
                .into_iter()
                .flat_map(|point| &point.parents)
            {
                children.entry(*parent).or_default().push(*id);
            }
        }
        children
    }

    fn move_to(&mut self, id: ObjectId) -> Result<(), HistoryError> {
        if let Some(previous) = self.now.filter(|previous| *previous != id) {
            let previous = previous.to_string();
            self.meta.recent.retain(|each| *each != previous);
            self.meta.recent.push(previous);
            let extra = self.meta.recent.len().saturating_sub(RECENT);
            self.meta.recent.drain(..extra);
        }
        self.now = Some(id);
        self.repository.write_ref("refs/heads/main", id)?;
        self.save_meta()
    }

    fn save_meta(&self) -> Result<(), HistoryError> {
        let bytes = serde_json::to_vec_pretty(&self.meta).unwrap_or_default();
        Ok(self.repository.write_file(Path::new(META_FILE), &bytes)?)
    }

    /// The files of a tree, by path, with their blobs.
    fn files(&self, tree: Option<ObjectId>) -> Result<BTreeMap<PathBuf, ObjectId>, HistoryError> {
        let mut files = BTreeMap::new();
        let mut waiting: Vec<(PathBuf, ObjectId)> = tree
            .map(|tree| (PathBuf::new(), tree))
            .into_iter()
            .collect();
        while let Some((folder, tree)) = waiting.pop() {
            for (name, entry) in self.repository.read_tree(tree)? {
                match entry {
                    Entry::Blob(blob) => _ = files.insert(folder.join(name), blob),
                    Entry::Tree(sub) => waiting.push((folder.join(name), sub)),
                }
            }
        }
        Ok(files)
    }

    fn difference(
        &self,
        from: Option<ObjectId>,
        to: ObjectId,
    ) -> Result<Vec<(PathBuf, Option<Vec<u8>>)>, HistoryError> {
        let (old, new) = (self.files(from)?, self.files(Some(to))?);
        let mut plan = Vec::new();
        for (path, blob) in &new {
            if old.get(path) != Some(blob) {
                plan.push((path.clone(), Some(self.repository.read_blob(*blob)?)));
            }
        }
        for path in old.keys().filter(|path| !new.contains_key(*path)) {
            plan.push((path.clone(), None));
        }
        Ok(plan)
    }

    /// Writes the files of the scope as objects and gives the tree of the whole. Files whose
    /// length and time did not change since the last look are not read again.
    fn snapshot(&mut self) -> Result<ObjectId, HistoryError> {
        let mut files = BTreeMap::new();
        for root in self.scope.roots.clone() {
            self.collect(&root, &mut files)?;
        }
        self.seen.retain(|path, _| files.contains_key(path));
        // Folders as nested maps, built from the deepest up.
        let mut folders: BTreeMap<PathBuf, BTreeMap<String, Entry>> = BTreeMap::new();
        folders.insert(PathBuf::new(), BTreeMap::new());
        for (path, blob) in &files {
            let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let name = file_name(path);
            let mut ancestor = folder.clone();
            loop {
                folders.entry(ancestor.clone()).or_default();
                match ancestor.parent() {
                    Some(parent) => ancestor = parent.to_path_buf(),
                    None => break,
                }
            }
            folders
                .entry(folder)
                .or_default()
                .insert(name, Entry::Blob(*blob));
        }
        let mut deepest_first: Vec<PathBuf> = folders.keys().cloned().collect();
        deepest_first.sort_by_key(|folder| std::cmp::Reverse(folder.components().count()));
        for folder in deepest_first {
            if folder.as_os_str().is_empty() {
                continue;
            }
            let entries = folders.remove(&folder).unwrap_or_default();
            let tree = self.repository.write_tree(&entries)?;
            let parent = folder.parent().map(Path::to_path_buf).unwrap_or_default();
            folders
                .entry(parent)
                .or_default()
                .insert(file_name(&folder), Entry::Tree(tree));
        }
        let top = folders.remove(&PathBuf::new()).unwrap_or_default();
        Ok(self.repository.write_tree(&top)?)
    }

    /// Every file at or below `relative`, with its blob.
    fn collect(
        &mut self,
        relative: &Path,
        files: &mut BTreeMap<PathBuf, ObjectId>,
    ) -> Result<(), HistoryError> {
        let slashed = relative.to_string_lossy().replace('\\', "/");
        if (self.scope.skip)(&slashed) || file_name(relative).ends_with(".tmp") {
            return Ok(());
        }
        let path = self.root.join(relative);
        let io_error = |source| HistoryError::Io {
            path: path.clone(),
            source,
        };
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(io_error(source)),
        };
        if metadata.is_dir() {
            let mut names: Vec<PathBuf> = Vec::new();
            for entry in fs::read_dir(&path).map_err(io_error)? {
                names.push(relative.join(entry.map_err(io_error)?.file_name()));
            }
            for child in names {
                self.collect(&child, files)?;
            }
            return Ok(());
        }
        let (length, modified) = (metadata.len(), metadata.modified().ok());
        let known = self.seen.get(relative).filter(|seen| {
            seen.length == length && seen.modified == modified && modified.is_some()
        });
        let blob = match known {
            Some(seen) => seen.blob,
            None => {
                let bytes = match fs::read(&path) {
                    Ok(bytes) => same_layout(&file_name(relative), bytes),
                    // Gone between the listing and the read.
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(source) => return Err(io_error(source)),
                };
                let blob = self.repository.write_blob(&bytes)?;
                let seen = Seen {
                    length,
                    modified,
                    blob,
                };
                self.seen.insert(relative.to_path_buf(), seen);
                blob
            }
        };
        files.insert(relative.to_path_buf(), blob);
        Ok(())
    }
}

fn point(id: ObjectId, commit: Commit) -> Point {
    let (label, time) = match commit.message.rsplit_once(TIME_TRAILER) {
        Some((label, millis)) => (label.trim_end().to_string(), millis.trim().parse().ok()),
        None => (commit.message.clone(), None),
    };
    Point {
        id,
        parents: commit.parents,
        time: time.unwrap_or(commit.time * 1000),
        label,
        tree: commit.tree,
    }
}

/// JSON in one layout, so a file the runtime wrote again with the same content, as an undo
/// does, is the same point as the one an agent wrote in its own layout. Numbers stay as they
/// are written: a record that holds `0` may not take `0.0`. A file that is no JSON stays as it
/// is.
fn same_layout(name: &str, bytes: Vec<u8>) -> Vec<u8> {
    if !name.ends_with(".json") {
        return bytes;
    }
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) => match serde_json::to_vec_pretty(&value) {
            Ok(mut pretty) => {
                pretty.push(b'\n');
                pretty
            }
            Err(_) => bytes,
        },
        Err(_) => bytes,
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Parents before children; among the points that may come next, the oldest first.
fn topological(points: &BTreeMap<ObjectId, Point>) -> Vec<ObjectId> {
    let mut waiting_for: BTreeMap<ObjectId, usize> = BTreeMap::new();
    let mut children: BTreeMap<ObjectId, Vec<ObjectId>> = BTreeMap::new();
    for (id, point) in points {
        let known = point
            .parents
            .iter()
            .filter(|parent| points.contains_key(*parent));
        waiting_for.insert(*id, known.clone().count());
        for parent in known {
            children.entry(*parent).or_default().push(*id);
        }
    }
    let time = |id: &ObjectId| points.get(id).map_or(0, |point| point.time);
    let mut ready: BinaryHeap<std::cmp::Reverse<(u64, ObjectId)>> = waiting_for
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| std::cmp::Reverse((time(id), *id)))
        .collect();
    let mut order = Vec::new();
    while let Some(std::cmp::Reverse((_, id))) = ready.pop() {
        order.push(id);
        for child in children.get(&id).into_iter().flatten() {
            if let Some(count) = waiting_for.get_mut(child) {
                *count -= 1;
                if *count == 0 {
                    ready.push(std::cmp::Reverse((time(child), *child)));
                }
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder {
        folder: tempfile::TempDir,
    }

    impl Folder {
        fn new() -> Self {
            Self {
                folder: tempfile::tempdir().unwrap(),
            }
        }

        fn write(&self, path: &str, text: &str) {
            let path = self.folder.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn remove(&self, path: &str) {
            fs::remove_file(self.folder.path().join(path)).unwrap();
        }

        /// A file's text, without the layout a point gives JSON.
        fn read(&self, path: &str) -> Option<String> {
            let text = fs::read_to_string(self.folder.path().join(path)).ok()?;
            Some(text.trim().trim_matches('"').to_string())
        }

        fn history(&self) -> History {
            let scope = Scope {
                roots: vec![PathBuf::from("project.json"), PathBuf::from("state")],
                skip: |path| path == "state/skipped.json",
            };
            History::open(self.folder.path(), scope).unwrap()
        }

        /// What `go_to` or `merge` gives, written as the project would.
        fn apply(&self, plan: Vec<(PathBuf, Option<Vec<u8>>)>) {
            for (path, bytes) in plan {
                let path = self.folder.path().join(path);
                match bytes {
                    Some(bytes) => {
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(path, bytes).unwrap();
                    }
                    None => fs::remove_file(path).unwrap(),
                }
            }
        }
    }

    fn new(recorded: Recorded) -> ObjectId {
        match recorded {
            Recorded::New(id) => id,
            other => panic!("expected a new point, got {other:?}"),
        }
    }

    #[test]
    fn a_change_is_a_new_point_and_the_same_folder_is_none() {
        let folder = Folder::new();
        folder.write("project.json", "{}");
        let mut history = folder.history();
        let first = new(history.record("Opened").unwrap());
        assert_eq!(history.record("Nothing").unwrap(), Recorded::Unchanged);
        folder.write("state/a.json", "1");
        // Neither what is skipped nor what is outside the scope is part of a point.
        folder.write("state/skipped.json", "x");
        folder.write("assets/take.wav", "x");
        let second = new(history.record("Add a").unwrap());
        folder.write("state/skipped.json", "y");
        assert_eq!(history.record("Nothing").unwrap(), Recorded::Unchanged);
        assert_eq!(history.point(second).unwrap().parents, [first]);
        assert_eq!(history.point(second).unwrap().label, "Add a");
    }

    #[test]
    fn the_same_json_in_another_layout_is_the_same_point() {
        let folder = Folder::new();
        folder.write(
            "state/a.json",
            "{\"tool\": \"x\",\n  \"state\": {\"bpm\": 92.5}}",
        );
        let mut history = folder.history();
        new(history.record("Opened").unwrap());
        folder.write("state/a.json", "{\"tool\":\"x\",\"state\":{\"bpm\":92.5}}");
        assert_eq!(
            history.record("Written again").unwrap(),
            Recorded::Unchanged
        );
    }

    #[test]
    fn undo_and_redo_move_instead_of_adding_points() {
        let folder = Folder::new();
        folder.write("state/a.json", "1");
        let mut history = folder.history();
        let first = new(history.record("Opened").unwrap());
        folder.write("state/a.json", "2");
        let second = new(history.record("Two").unwrap());
        // An undo brings the file back.
        folder.write("state/a.json", "1");
        assert_eq!(history.record("Undo").unwrap(), Recorded::Moved(first));
        folder.write("state/a.json", "2");
        assert_eq!(history.record("Redo").unwrap(), Recorded::Moved(second));
        // A new change after an undo starts a version: a second child of the first point.
        folder.write("state/a.json", "1");
        history.record("Undo").unwrap();
        folder.write("state/a.json", "3");
        let third = new(history.record("Three").unwrap());
        assert_eq!(history.point(third).unwrap().parents, [first]);
        assert_eq!(history.children()[&first], [second, third]);
    }

    #[test]
    fn going_to_a_point_gives_the_files_that_differ_and_reopens_there() {
        let folder = Folder::new();
        folder.write("state/a.json", "1");
        folder.write("state/song/instance.json", "\"song\"");
        let mut history = folder.history();
        let first = new(history.record("Opened").unwrap());
        folder.write("state/a.json", "2");
        folder.remove("state/song/instance.json");
        folder.write("state/b.json", "b");
        new(history.record("Changes").unwrap());
        let plan = history.go_to(first).unwrap();
        assert_eq!(plan.len(), 3);
        folder.apply(plan);
        assert_eq!(folder.read("state/a.json").as_deref(), Some("1"));
        assert_eq!(
            folder.read("state/song/instance.json").as_deref(),
            Some("song")
        );
        assert_eq!(folder.read("state/b.json"), None);
        assert_eq!(history.record("After").unwrap(), Recorded::Unchanged);
        let reopened = folder.history();
        assert_eq!(reopened.now(), Some(first));
        assert_eq!(reopened.order().len(), 2);
    }

    #[test]
    fn a_merge_takes_what_the_other_version_changed_and_has_two_parents() {
        let folder = Folder::new();
        folder.write("state/a.json", "a");
        folder.write("state/b.json", "b");
        folder.write("state/c.json", "c");
        let mut history = folder.history();
        let base = new(history.record("Opened").unwrap());
        // The other version changes a and b.
        folder.write("state/a.json", "\"a theirs\"");
        folder.write("state/b.json", "\"b theirs\"");
        let theirs = new(history.record("Theirs").unwrap());
        folder.apply(history.go_to(base).unwrap());
        // This version changes b and c.
        folder.write("state/b.json", "\"b mine\"");
        folder.write("state/c.json", "\"c mine\"");
        let mine = new(history.record("Mine").unwrap());

        let plan = history.merge(theirs, true).unwrap();
        folder.apply(plan);
        assert_eq!(folder.read("state/a.json").as_deref(), Some("a theirs"));
        assert_eq!(folder.read("state/b.json").as_deref(), Some("b mine"));
        assert_eq!(folder.read("state/c.json").as_deref(), Some("c mine"));
        let merged = new(history.record("Merged").unwrap());
        assert_eq!(history.point(merged).unwrap().parents, [mine, theirs]);

        folder.apply(history.go_to(mine).unwrap());
        folder.apply(history.merge(theirs, false).unwrap());
        assert_eq!(folder.read("state/b.json").as_deref(), Some("b theirs"));
    }

    #[test]
    fn moments_names_and_order_survive_a_reopen() {
        let folder = Folder::new();
        folder.write("state/a.json", "1");
        let mut history = folder.history();
        let first = new(history.record("Opened").unwrap());
        assert!(history.mark_moment().unwrap());
        assert!(!history.mark_moment().unwrap());
        history.set_name(first, "Sketch").unwrap();
        // Several points in one second keep their order.
        let mut points = vec![first];
        for step in 2..6 {
            folder.write("state/a.json", &step.to_string());
            points.push(new(history.record("Step").unwrap()));
        }
        let reopened = folder.history();
        assert_eq!(reopened.moment(first), Some("Opened"));
        assert_eq!(reopened.name(first), Some("Sketch"));
        assert_eq!(reopened.order(), points.as_slice());
    }
}
