//! The history of one project folder: every state it was in, as git commits in a repository
//! of its own inside the folder, with where the project is now.
//!
//! A point is saved by looking at the folder, not by being told what changed: the folder is
//! the truth. Two states are the same when git says so, byte for byte.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::git::{Commit, Entry, GitError, ObjectId, Repository};

/// Where the history lives, inside the project folder. Hidden, and outside every folder the
/// runtime or an agent reads.
pub const HISTORY_FOLDER: &str = ".sound-tools/history";

/// What git has no place for: the names of versions and which are set aside.
const META_FILE: &str = "sound-tools.json";

const TIME_TRAILER: &str = "Sound-Tools-Time: ";

/// How many points are read at a time, newest first: the history is read as far back as it
/// is looked at, not whole.
pub const PAGE: usize = 300;

/// Undone work this small, followed by a new change, is not a version of its own: it was
/// taken back, not tried. More is kept as a version, since it was a result.
const UNDONE_POINTS: usize = 10;
const UNDONE_SPAN: Duration = Duration::from_secs(120);

/// What of the folder the history keeps. Paths are relative to the project folder.
#[derive(Clone, Debug)]
pub struct Scope {
    /// Files and folders that are part of the project.
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
    /// The change that led here, such as "Change frequency", for the undo menu.
    pub label: String,
    tree: ObjectId,
}

/// Files to write, or to remove for `None`, relative to the project folder.
pub type Files = Vec<(PathBuf, Option<Vec<u8>>)>;

/// One file that differs between two points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
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

/// The name of a version, and the latest point it was made for: a version that grew since
/// is named again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
    pub tip: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Meta {
    /// By the first point of the version.
    #[serde(default)]
    names: BTreeMap<String, Named>,
    /// Versions set aside by hand, by their first point.
    #[serde(default)]
    aside: BTreeSet<String>,
    /// Versions that were undone and changed after, by their first point, see
    /// [`History::note_undone`].
    #[serde(default)]
    undone: BTreeSet<String>,
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
    /// The points read so far: every one newer than the oldest of them.
    points: BTreeMap<ObjectId, Point>,
    /// The points read, parents before children, older before newer.
    order: Vec<ObjectId>,
    /// The points to read next, newest first, with their commits.
    waiting: BinaryHeap<(u64, ObjectId)>,
    commits: BTreeMap<ObjectId, Commit>,
    now: Option<ObjectId>,
    meta: Meta,
    seen: BTreeMap<PathBuf, Seen>,
    /// The tip of a version being merged in: the next point gets it as a second parent.
    merging: Option<ObjectId>,
    changes: u64,
}

impl History {
    /// Opens the history of the project folder `root`, making it when there is none.
    pub fn open(root: &Path, scope: Scope) -> Result<Self, HistoryError> {
        let repository = Repository::open(&root.join(HISTORY_FOLDER))?;
        let meta = match repository.read_file(Path::new(META_FILE))? {
            // A damaged file loses names, not points.
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
            waiting: BinaryHeap::new(),
            commits: BTreeMap::new(),
            now,
            meta,
            seen: BTreeMap::new(),
            merging: None,
            changes: 0,
        };
        for name in history.repository.refs_in("refs/tips")? {
            if let Some(tip) = history.repository.read_ref(&name)? {
                history.wait_for(tip)?;
            }
        }
        if let Some(now) = now {
            history.wait_for(now)?;
            history.read_through(now)?;
        }
        history.read_more(PAGE.saturating_sub(history.order.len()))?;
        Ok(history)
    }

    /// Whether older points are left to read.
    pub fn has_more(&self) -> bool {
        !self.waiting.is_empty()
    }

    /// Reads up to `count` older points.
    pub fn read_more(&mut self, count: usize) -> Result<(), HistoryError> {
        let mut read = Vec::new();
        while read.len() < count
            && let Some(id) = self.read_next()?
        {
            read.push(id);
        }
        self.take_in(read);
        Ok(())
    }

    /// Reads points until `id` is read.
    fn read_through(&mut self, id: ObjectId) -> Result<(), HistoryError> {
        if self.points.contains_key(&id) {
            return Ok(());
        }
        let time = point(id, self.repository.read_commit(id)?).time;
        let mut read = Vec::new();
        while self.waiting.peek().is_some_and(|(next, _)| *next >= time)
            && let Some(next) = self.read_next()?
        {
            read.push(next);
        }
        self.take_in(read);
        match self.points.contains_key(&id) {
            true => Ok(()),
            false => Err(HistoryError::UnknownPoint),
        }
    }

    /// Takes the newest waiting point in, and waits for its parents.
    fn read_next(&mut self) -> Result<Option<ObjectId>, HistoryError> {
        let Some((_, id)) = self.waiting.pop() else {
            return Ok(None);
        };
        let Some(commit) = self.commits.remove(&id) else {
            return Ok(None);
        };
        for parent in &commit.parents {
            self.wait_for(*parent)?;
        }
        self.points.insert(id, point(id, commit));
        Ok(Some(id))
    }

    /// `read`, newest first, goes before the points read already.
    fn take_in(&mut self, mut read: Vec<ObjectId>) {
        if read.is_empty() {
            return;
        }
        read.reverse();
        read.append(&mut self.order);
        self.order = read;
        self.changes += 1;
    }

    fn wait_for(&mut self, id: ObjectId) -> Result<(), HistoryError> {
        if self.points.contains_key(&id) || self.commits.contains_key(&id) {
            return Ok(());
        }
        let commit = self.repository.read_commit(id)?;
        let time = point(id, commit.clone()).time;
        self.commits.insert(id, commit);
        self.waiting.push((time, id));
        Ok(())
    }

    /// The tree of a point, read or not.
    fn tree_of(&self, id: ObjectId) -> Result<ObjectId, HistoryError> {
        match self.points.get(&id) {
            Some(point) => Ok(point.tree),
            None => Ok(self.repository.read_commit(id)?.tree),
        }
    }

    fn parents_of(&self, id: ObjectId) -> Vec<ObjectId> {
        match self.points.get(&id) {
            Some(point) => point.parents.clone(),
            None => (self.repository.read_commit(id)).map_or(Vec::new(), |commit| commit.parents),
        }
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

    /// A number that changes with every change of the history, for a view that follows it.
    pub fn changes(&self) -> u64 {
        self.changes
    }

    /// The name given to the version that starts at `start`.
    pub fn name(&self, start: ObjectId) -> Option<&Named> {
        self.meta.names.get(&start.to_string())
    }

    pub fn is_aside(&self, start: ObjectId) -> bool {
        self.meta.aside.contains(&start.to_string())
    }

    pub fn is_undone(&self, start: ObjectId) -> bool {
        self.meta.undone.contains(&start.to_string())
    }

    /// The project went back by undo from the version that starts at `start`, and changed
    /// something: when that version is small, it was taken back rather than tried, and it
    /// shows as undone instead of as a version.
    pub fn note_undone(&mut self, start: ObjectId) -> Result<(), HistoryError> {
        let mut children: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();
        // The first child of each point continues its version.
        for id in self.order.iter().rev() {
            if let Some(parent) = self.points.get(id).and_then(|point| point.parents.first()) {
                children.insert(*parent, *id);
            }
        }
        let (mut count, mut last) = (1, start);
        while let Some(next) = children.get(&last) {
            (count, last) = (count + 1, *next);
            if count >= UNDONE_POINTS {
                return Ok(());
            }
        }
        let time = |id: ObjectId| self.points.get(&id).map_or(0, |point| point.time);
        if time(last).saturating_sub(time(start)) >= UNDONE_SPAN.as_millis() as u64 {
            return Ok(());
        }
        self.meta.undone.insert(start.to_string());
        self.save_meta()
    }

    /// Looks at the folder and saves it as a new point after the one it is at, named `label`,
    /// when it differs from that one. Gives the new point.
    pub fn record(&mut self, label: &str) -> Result<Option<ObjectId>, HistoryError> {
        let tree = self.snapshot()?;
        let merging = self.merging.take();
        if merging.is_none() && self.now_tree() == Some(tree) {
            return Ok(None);
        }
        let parents: Vec<ObjectId> = self.now.into_iter().chain(merging).collect();
        self.add(tree, parents, label).map(Some)
    }

    /// Like [`Self::record`], but the folder takes the place of the point the project is at,
    /// which must be the latest of its version: for changes that belong to the one before,
    /// such as the writes of one request of an agent. A folder that is back to where that
    /// point came from leaves no point.
    pub fn amend(&mut self, label: &str) -> Result<Option<ObjectId>, HistoryError> {
        let Some(now) = self.now else {
            return self.record(label);
        };
        let has_children = (self.points.values()).any(|point| point.parents.contains(&now));
        let parents = self.points.get(&now).map(|point| point.parents.clone());
        let parents = parents.unwrap_or_default();
        if has_children || parents.is_empty() || self.merging.is_some() {
            return self.record(label);
        }
        let tree = self.snapshot()?;
        if self.now_tree() == Some(tree) {
            return Ok(None);
        }
        self.points.remove(&now);
        self.order.retain(|id| *id != now);
        self.repository.remove_ref(&format!("refs/tips/{now}"))?;
        let first = parents[0];
        if self.points.get(&first).map(|point| point.tree) == Some(tree) {
            self.repository
                .write_ref(&format!("refs/tips/{first}"), first)?;
            self.move_to(first)?;
            return Ok(None);
        }
        self.now = Some(first);
        self.add(tree, parents, label).map(Some)
    }

    fn add(
        &mut self,
        tree: ObjectId,
        parents: Vec<ObjectId>,
        label: &str,
    ) -> Result<ObjectId, HistoryError> {
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
        // Undone work that is changed on is kept: it is a version from here.
        let mut line = parents.first().copied();
        for _ in 0..UNDONE_POINTS {
            let Some(point) = line else {
                break;
            };
            if self.meta.undone.remove(&point.to_string()) {
                self.save_meta()?;
                break;
            }
            line = self.parents_of(point).first().copied();
        }
        Ok(id)
    }

    /// Where undo goes: the point the one now came from.
    pub fn undo_target(&self) -> Option<ObjectId> {
        self.points.get(&self.now?)?.parents.first().copied()
    }

    /// Where redo goes: the newest point that came from the one now.
    pub fn redo_target(&self) -> Option<ObjectId> {
        let now = self.now?;
        (self.order.iter().rev())
            .find(|id| {
                (self.points.get(id)).is_some_and(|point| point.parents.first() == Some(&now))
            })
            .copied()
    }

    pub fn set_name(
        &mut self,
        start: ObjectId,
        name: &str,
        tip: ObjectId,
    ) -> Result<(), HistoryError> {
        let named = Named {
            name: name.trim().to_string(),
            tip: tip.to_string(),
        };
        self.meta.names.insert(start.to_string(), named);
        self.save_meta()
    }

    pub fn set_aside(&mut self, start: ObjectId, aside: bool) -> Result<(), HistoryError> {
        match aside {
            true => self.meta.aside.insert(start.to_string()),
            false => self.meta.aside.remove(&start.to_string()),
        };
        self.save_meta()
    }

    /// The files that bring the folder from where it is now to `target`, and the project is
    /// at `target` from here. Record first, so that the folder is a point.
    pub fn go_to(&mut self, target: ObjectId) -> Result<Files, HistoryError> {
        self.read_through(target)?;
        let to = self.tree_of(target)?;
        let changes = self.difference(self.now_tree(), Some(to))?;
        self.move_to(target)?;
        Ok(changes
            .into_iter()
            .map(|change| (change.path, change.after))
            .collect())
    }

    /// The files that merge the version whose latest point is `tip` into where the project is
    /// now. A file only `tip` changed since the two split comes over. A file both changed
    /// comes over too, unless `keep_mine`. The next point is the merge, with `tip` as its
    /// second parent.
    pub fn merge(&mut self, tip: ObjectId, keep_mine: bool) -> Result<Files, HistoryError> {
        let now = self.now.ok_or(HistoryError::UnknownPoint)?;
        let (theirs, ours) = (self.tree_of(tip)?, self.tree_of(now)?);
        let base = match self.merge_base(now, tip) {
            Some(base) => Some(self.tree_of(base)?),
            None => None,
        };
        let (base, ours, theirs) = (
            self.files(base)?,
            self.files(Some(ours))?,
            self.files(Some(theirs))?,
        );
        let paths: BTreeSet<&PathBuf> = base
            .keys()
            .chain(ours.keys())
            .chain(theirs.keys())
            .collect();
        let mut files = Vec::new();
        for path in paths {
            let (old, mine, their) = (base.get(path), ours.get(path), theirs.get(path));
            if their == old || their == mine || (keep_mine && mine != old) {
                continue;
            }
            files.push((path.clone(), self.read(their.copied())?));
        }
        self.merging = Some(tip);
        Ok(files)
    }

    /// What differs from the point `from`, or from an empty folder, to `to`.
    pub fn difference_between(
        &self,
        from: Option<ObjectId>,
        to: ObjectId,
    ) -> Result<Vec<FileChange>, HistoryError> {
        let from = match from {
            Some(from) => Some(self.tree_of(from)?),
            None => None,
        };
        self.difference(from, Some(self.tree_of(to)?))
    }

    /// Forgets every point: the folder as it is becomes the first, named `label`.
    pub fn start_again(&mut self, label: &str) -> Result<(), HistoryError> {
        for name in self.repository.refs_in("refs/tips")? {
            self.repository.remove_ref(&name)?;
        }
        self.points.clear();
        self.order.clear();
        self.waiting.clear();
        self.commits.clear();
        self.now = None;
        self.merging = None;
        self.meta = Meta::default();
        self.save_meta()?;
        self.record(label)?;
        Ok(())
    }

    /// The newest point both `one` and `other` come from.
    fn merge_base(&self, one: ObjectId, other: ObjectId) -> Option<ObjectId> {
        let ancestors = self.ancestors(one);
        let theirs = self.ancestors(other);
        let time = |id: &ObjectId| match self.points.get(id) {
            Some(point) => point.time,
            None => (self.repository.read_commit(*id)).map_or(0, |commit| point(*id, commit).time),
        };
        ancestors
            .intersection(&theirs)
            .max_by_key(|id| time(id))
            .copied()
    }

    fn ancestors(&self, from: ObjectId) -> BTreeSet<ObjectId> {
        let mut found = BTreeSet::new();
        let mut waiting = vec![from];
        while let Some(id) = waiting.pop() {
            if found.insert(id) {
                waiting.extend(self.parents_of(id));
            }
        }
        found
    }

    fn now_tree(&self) -> Option<ObjectId> {
        self.points.get(&self.now?).map(|point| point.tree)
    }

    fn move_to(&mut self, id: ObjectId) -> Result<(), HistoryError> {
        self.now = Some(id);
        self.changes += 1;
        Ok(self.repository.write_ref("refs/heads/main", id)?)
    }

    fn save_meta(&mut self) -> Result<(), HistoryError> {
        self.changes += 1;
        let bytes = serde_json::to_vec_pretty(&self.meta).unwrap_or_default();
        Ok(self.repository.write_file(Path::new(META_FILE), &bytes)?)
    }

    fn read(&self, blob: Option<ObjectId>) -> Result<Option<Vec<u8>>, HistoryError> {
        Ok(match blob {
            Some(blob) => Some(self.repository.read_blob(blob)?),
            None => None,
        })
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
        to: Option<ObjectId>,
    ) -> Result<Vec<FileChange>, HistoryError> {
        let (old, new) = (self.files(from)?, self.files(to)?);
        let paths: BTreeSet<&PathBuf> = old.keys().chain(new.keys()).collect();
        let mut changes = Vec::new();
        for path in paths {
            let (before, after) = (old.get(path).copied(), new.get(path).copied());
            if before != after {
                changes.push(FileChange {
                    path: path.clone(),
                    before: self.read(before)?,
                    after: self.read(after)?,
                });
            }
        }
        Ok(changes)
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
            let mut ancestor = folder.clone();
            while let Some(parent) = ancestor.parent() {
                folders.entry(ancestor.clone()).or_default();
                ancestor = parent.to_path_buf();
            }
            (folders.entry(folder).or_default()).insert(file_name(path), Entry::Blob(*blob));
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
            (folders.entry(parent).or_default()).insert(file_name(&folder), Entry::Tree(tree));
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
                    Ok(bytes) => bytes,
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

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
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

        fn read(&self, path: &str) -> Option<String> {
            fs::read_to_string(self.folder.path().join(path)).ok()
        }

        fn history(&self) -> History {
            let scope = Scope {
                roots: vec![PathBuf::from("project.json"), PathBuf::from("state")],
                skip: |path| path == "state/skipped.json",
            };
            History::open(self.folder.path(), scope).unwrap()
        }

        /// What `go_to` or `merge` gives, written as the project would.
        fn apply(&self, files: Files) {
            for (path, bytes) in files {
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

    #[test]
    fn a_change_is_a_new_point_and_the_same_folder_is_none() {
        let folder = Folder::new();
        folder.write("project.json", "{}");
        let mut history = folder.history();
        let first = history.record("Opened").unwrap().unwrap();
        assert_eq!(history.record("Nothing").unwrap(), None);
        folder.write("state/a.json", "1");
        // Neither what is skipped nor what is outside the scope is part of a point.
        folder.write("state/skipped.json", "x");
        folder.write("assets/take.wav", "x");
        let second = history.record("Add a").unwrap().unwrap();
        folder.write("state/skipped.json", "y");
        assert_eq!(history.record("Nothing").unwrap(), None);
        assert_eq!(history.point(second).unwrap().parents, [first]);
        assert_eq!(history.point(second).unwrap().label, "Add a");
    }

    #[test]
    fn undo_goes_to_the_parent_and_redo_to_the_newest_child() {
        let folder = Folder::new();
        folder.write("state/a.json", "1");
        let mut history = folder.history();
        let first = history.record("Opened").unwrap().unwrap();
        folder.write("state/a.json", "2");
        let second = history.record("Two").unwrap().unwrap();
        assert_eq!(history.undo_target(), Some(first));
        folder.apply(history.go_to(first).unwrap());
        assert_eq!(folder.read("state/a.json").as_deref(), Some("1"));
        assert_eq!(history.redo_target(), Some(second));
        // A change after an undo starts a version, and redo now goes there.
        folder.write("state/a.json", "3");
        let third = history.record("Three").unwrap().unwrap();
        assert_eq!(history.point(third).unwrap().parents, [first]);
        folder.apply(history.go_to(first).unwrap());
        assert_eq!(history.redo_target(), Some(third));
        // The same state again is a new point: git compares, nothing guesses.
        folder.write("state/a.json", "2");
        let again = history.record("Two again").unwrap().unwrap();
        assert_ne!(again, second);
    }

    #[test]
    fn an_amend_takes_the_place_of_the_last_point_and_one_that_goes_back_leaves_none() {
        let folder = Folder::new();
        folder.write("state/a.json", "1");
        let mut history = folder.history();
        let first = history.record("Opened").unwrap().unwrap();
        folder.write("state/b.json", "b");
        history.record("Agent").unwrap();
        folder.write("state/c.json", "c");
        let joined = history.amend("Agent").unwrap().unwrap();
        assert_eq!(history.order(), [first, joined]);
        assert_eq!(history.point(joined).unwrap().parents, [first]);
        folder.remove("state/b.json");
        folder.remove("state/c.json");
        assert_eq!(history.amend("Agent").unwrap(), None);
        assert_eq!(history.now(), Some(first));
        assert_eq!(folder.history().order(), [first]);
    }

    #[test]
    fn going_to_a_point_gives_the_files_that_differ_and_reopens_there() {
        let folder = Folder::new();
        folder.write("state/a.json", "1");
        folder.write("state/song/instance.json", "song");
        let mut history = folder.history();
        let first = history.record("Opened").unwrap().unwrap();
        folder.write("state/a.json", "2");
        folder.remove("state/song/instance.json");
        folder.write("state/b.json", "b");
        history.record("Changes").unwrap();
        let files = history.go_to(first).unwrap();
        assert_eq!(files.len(), 3);
        folder.apply(files);
        assert_eq!(folder.read("state/a.json").as_deref(), Some("1"));
        assert_eq!(
            folder.read("state/song/instance.json").as_deref(),
            Some("song")
        );
        assert_eq!(folder.read("state/b.json"), None);
        assert_eq!(history.record("After").unwrap(), None);
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
        let base = history.record("Opened").unwrap().unwrap();
        // The other version changes a and b.
        folder.write("state/a.json", "a theirs");
        folder.write("state/b.json", "b theirs");
        let theirs = history.record("Theirs").unwrap().unwrap();
        folder.apply(history.go_to(base).unwrap());
        // This version changes b and c.
        folder.write("state/b.json", "b mine");
        folder.write("state/c.json", "c mine");
        let mine = history.record("Mine").unwrap().unwrap();

        folder.apply(history.merge(theirs, true).unwrap());
        assert_eq!(folder.read("state/a.json").as_deref(), Some("a theirs"));
        assert_eq!(folder.read("state/b.json").as_deref(), Some("b mine"));
        assert_eq!(folder.read("state/c.json").as_deref(), Some("c mine"));
        let merged = history.record("Merged").unwrap().unwrap();
        assert_eq!(history.point(merged).unwrap().parents, [mine, theirs]);

        folder.apply(history.go_to(mine).unwrap());
        folder.apply(history.merge(theirs, false).unwrap());
        assert_eq!(folder.read("state/b.json").as_deref(), Some("b theirs"));
    }

    #[test]
    fn a_long_history_is_read_newest_first_as_far_as_it_is_looked_at() {
        let folder = Folder::new();
        let mut history = folder.history();
        let mut points = Vec::new();
        for step in 0..PAGE + 10 {
            folder.write("state/a.json", &step.to_string());
            points.push(history.record("Step").unwrap().unwrap());
        }
        let mut reopened = folder.history();
        assert_eq!(reopened.order(), &points[10..]);
        assert!(reopened.has_more());
        // Going to a point not read yet reads back to it.
        folder.apply(reopened.go_to(points[3]).unwrap());
        assert_eq!(folder.read("state/a.json").as_deref(), Some("3"));
        reopened.read_more(PAGE).unwrap();
        assert_eq!(reopened.order(), points.as_slice());
        assert!(!reopened.has_more());
    }

    #[test]
    fn small_undone_work_is_noted_and_changing_on_from_it_keeps_it() {
        let folder = Folder::new();
        folder.write("state/a.json", "0");
        let mut history = folder.history();
        let first = history.record("Opened").unwrap().unwrap();
        folder.write("state/a.json", "1");
        let undone = history.record("One").unwrap().unwrap();
        folder.apply(history.go_to(first).unwrap());
        history.note_undone(undone).unwrap();
        assert!(history.is_undone(undone));
        folder.apply(history.go_to(undone).unwrap());
        folder.write("state/a.json", "2");
        history.record("Two").unwrap();
        assert!(!history.is_undone(undone));
    }

    #[test]
    fn names_and_order_survive_a_reopen() {
        let folder = Folder::new();
        folder.write("state/a.json", "1");
        let mut history = folder.history();
        let first = history.record("Opened").unwrap().unwrap();
        // Several points in one second keep their order.
        let mut points = vec![first];
        for step in 2..6 {
            folder.write("state/a.json", &step.to_string());
            points.push(history.record("Step").unwrap().unwrap());
        }
        history.set_name(first, " Sketch ", points[4]).unwrap();
        let reopened = folder.history();
        let named = reopened.name(first).unwrap();
        assert_eq!(
            (named.name.as_str(), named.tip.as_str()),
            ("Sketch", &*points[4].to_string())
        );
        assert_eq!(reopened.order(), points.as_slice());
    }
}
