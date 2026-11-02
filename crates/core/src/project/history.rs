//! The project history. Every saved state of the folder is a point: a commit of a git repository
//! of its own, in [`HISTORY_FOLDER`]. Undo, redo and going to any point write that point's files
//! back. A point continues the line of the point it came from, or starts a line of its own: a
//! version, or work taken back with undo.

mod folder;
mod git;

use std::cell::RefCell;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::GROUPING_WINDOW;
pub(crate) use folder::EXTENSIONS_FOLDER;
use folder::Kept;
pub use git::PointId;
use git::{Commit, Files, Repository};

/// Where the history repository is, inside the project folder. Hidden, so neither the agent's
/// tools nor the runtime's watchers read it.
pub const HISTORY_FOLDER: &str = ".sound-tools/history";
/// What history keeps of its lines, next to the repository. Git keeps the points.
const LINES_FILE: &str = "lines.json";

/// Groups of outside changes that follow each other within this time are one point, when no
/// request is open (see [`Project::begin_request`](super::Project::begin_request)).
///
/// An agent writes the files of one request seconds apart, far more than the quiet window that
/// groups them for live apply. It is a heuristic for an agent that cannot say where its request
/// begins and ends, such as one in a terminal.
pub const OUTSIDE_POINT_WINDOW: Duration = Duration::from_secs(15);

/// A version folds away once this many points are newer than its last point.
const FOLD_AFTER: usize = 30;

/// Work taken back with undo and then left by a change is a version from this many points on.
const VERSION_POINTS: usize = 10;
/// ... or from this much time between its first and last point.
const VERSION_TIME: Duration = Duration::from_secs(120);

/// A line of points: a version, or a run of undone work.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// Its first point.
    pub start: PointId,
    /// Its latest point.
    pub tip: PointId,
    /// What a version without a name is called: "Version 2". `None` for undone work.
    pub number: Option<u32>,
    pub name: Option<String>,
    /// A name given by hand, which naming never replaces.
    #[serde(default)]
    pub named_by_hand: bool,
    /// The tip it was named at, so a version that grew since is named again.
    pub named_at: Option<PointId>,
    /// Folded away by hand.
    #[serde(default)]
    pub set_aside: bool,
}

impl Line {
    pub fn is_version(&self) -> bool {
        self.number.is_some()
    }

    /// The name, or "Version N".
    pub fn title(&self) -> String {
        match (&self.name, self.number) {
            (Some(name), _) => name.clone(),
            (None, Some(number)) => format!("Version {number}"),
            (None, None) => "Undone".to_string(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SavedLines {
    lines: Vec<Line>,
    next_number: u32,
}

/// One point, as [`History::points`] reads it.
#[derive(Clone, Debug)]
pub struct Point {
    pub id: PointId,
    /// The point it came from, then a merged version's tip.
    pub parents: Vec<PointId>,
    pub time: SystemTime,
    /// What made it, such as an edit or an agent request.
    pub label: String,
    /// The start of its line.
    pub line: PointId,
}

/// What came since the last point, which decides what the next one starts.
#[derive(Copy, Clone, Debug, PartialEq)]
enum Since {
    Nothing,
    /// Undo, from this point on.
    Undid {
        from: PointId,
    },
    /// A point chosen in the history, or undo that keeps a version.
    WentBack,
}

/// Who saves a point.
#[derive(Copy, Clone)]
pub(crate) enum Origin {
    /// A finished edit of the window or a tool.
    Edit,
    /// Changes to the files from outside, heard at this time.
    Outside(Instant),
}

/// A request of [`Project::begin_request`](super::Project::begin_request), until the watcher is
/// quiet after its end.
struct Request {
    label: String,
    ended: Option<Instant>,
}

pub struct History {
    repository: Repository,
    root: PathBuf,
    kept: Kept,
    head: PointId,
    head_tree: PointId,
    lines: Vec<Line>,
    next_number: u32,
    since: Since,
    /// When the head is a point of outside changes that nothing came after: when its last group
    /// was heard. The next group joins it.
    last_outside: Option<Instant>,
    request: Option<Request>,
    commits: RefCell<HashMap<PointId, Rc<Commit>>>,
    /// Counts every change, for a view that reads again only after one.
    generation: u64,
}

impl History {
    /// Opens the history of the project folder `root`, or begins one there with the folder as
    /// it is as its start. A folder that changed since the last point is saved as one point.
    /// `skipped` are the files the runtime writes at the top of `extensions/`.
    pub(crate) fn open(root: &Path, skipped: Vec<&'static str>) -> io::Result<Self> {
        let folder = root.join(HISTORY_FOLDER);
        let repository = Repository::open(&folder)?;
        // A project folder in git of its own leaves the history out.
        if let Some(hidden) = folder.parent() {
            let ignore = hidden.join(".gitignore");
            if !ignore.exists() {
                super::storage::write_atomically(&ignore, b"*\n")?;
            }
        }
        let kept = Kept { skipped };
        let mut history = match repository.head()? {
            Some(head) => {
                let commit = repository.read_commit(head)?;
                let saved: SavedLines = match std::fs::read(folder.join(LINES_FILE)) {
                    Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => SavedLines::default(),
                    Err(error) => return Err(error),
                };
                Self {
                    root: root.to_path_buf(),
                    head,
                    head_tree: commit.tree,
                    lines: saved.lines,
                    next_number: saved.next_number.max(1),
                    ..Self::empty(repository, root, kept, head, commit.tree)
                }
            }
            None => {
                let files = kept.read(root, &repository)?;
                let tree = repository.write_tree(&files)?;
                let commit = Commit {
                    tree,
                    parents: Vec::new(),
                    time: now_millis(),
                    label: "Project start".to_string(),
                };
                let head = repository.write_commit(&commit)?;
                let mut history = Self::empty(repository, root, kept, head, tree);
                history.new_version(head);
                history
            }
        };
        if history.lines.is_empty() {
            // Lines lost: the head is all that is shown.
            history.new_version(history.head);
        }
        history.save(Origin::Edit, "Changed while the app was closed")?;
        history.store()?;
        Ok(history)
    }

    fn empty(
        repository: Repository,
        root: &Path,
        kept: Kept,
        head: PointId,
        tree: PointId,
    ) -> Self {
        Self {
            repository,
            root: root.to_path_buf(),
            kept,
            head,
            head_tree: tree,
            lines: Vec::new(),
            next_number: 1,
            since: Since::Nothing,
            last_outside: None,
            request: None,
            commits: RefCell::default(),
            generation: 0,
        }
    }

    /// The repository, for an agent told where to look.
    pub fn folder(&self) -> &Path {
        self.repository.folder()
    }

    /// The point the files are at.
    pub fn current(&self) -> PointId {
        self.head
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    pub fn line(&self, start: PointId) -> Option<&Line> {
        self.lines.iter().find(|line| line.start == start)
    }

    /// Goes up with every change.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether `path`, inside the project folder, is or holds a file history keeps.
    pub(crate) fn keeps(&self, path: &Path) -> bool {
        path.strip_prefix(&self.root)
            .is_ok_and(|relative| self.kept.holds(relative))
    }

    fn commit(&self, id: PointId) -> io::Result<Rc<Commit>> {
        if let Some(commit) = self.commits.borrow().get(&id) {
            return Ok(commit.clone());
        }
        let commit = Rc::new(self.repository.read_commit(id)?);
        self.commits.borrow_mut().insert(id, commit.clone());
        Ok(commit)
    }

    /// The first `count` points, newest first, each with its line. Older ones are read only
    /// when asked for.
    pub fn points(&self, count: usize) -> io::Result<Vec<Point>> {
        self.walk()?.take(count).collect()
    }

    /// Every point, newest first, read as the walk goes. A child is always newer than its
    /// parents, so a point's line is known from its child in that line by the time it comes.
    fn walk(&self) -> io::Result<impl Iterator<Item = io::Result<Point>> + '_> {
        let tips: HashMap<PointId, PointId> = (self.lines.iter())
            .map(|line| (line.tip, line.start))
            .collect();
        let mut heap = BinaryHeap::new();
        let mut queued = HashSet::new();
        for tip in tips.keys().copied().chain([self.head]) {
            if queued.insert(tip) {
                heap.push((self.commit(tip)?.time, tip));
            }
        }
        let mut inherited: HashMap<PointId, PointId> = HashMap::new();
        Ok(std::iter::from_fn(move || {
            let (_, id) = heap.pop()?;
            let point = self.commit(id).and_then(|commit| {
                let line = (tips.get(&id).or(inherited.get(&id)).copied()).unwrap_or(id);
                // Only the start of a line is its own line's name.
                if let Some(parent) = commit.parents.first()
                    && line != id
                {
                    inherited.insert(*parent, line);
                }
                for parent in &commit.parents {
                    if queued.insert(*parent) {
                        heap.push((self.commit(*parent)?.time, *parent));
                    }
                }
                Ok(Point {
                    id,
                    parents: commit.parents.clone(),
                    time: UNIX_EPOCH + Duration::from_millis(commit.time),
                    label: commit.label.clone(),
                    line,
                })
            });
            Some(point)
        }))
    }

    /// The line `point` is in, read as far back as it takes.
    pub fn line_of(&self, point: PointId) -> io::Result<Option<&Line>> {
        for found in self.walk()? {
            let found = found?;
            if found.id == point {
                return Ok(self.line(found.line));
            }
        }
        Ok(None)
    }

    /// Whether the version is folded away: it is not the current one and was merged, set
    /// aside by hand, or has [`FOLD_AFTER`] newer points since its last point.
    pub fn folded_away(&self, line: &Line) -> io::Result<bool> {
        if self
            .line_of(self.head)?
            .is_some_and(|current| current.start == line.start)
        {
            return Ok(false);
        }
        if line.set_aside {
            return Ok(true);
        }
        let newer = self.points(FOLD_AFTER + 1)?;
        let Some(index) = newer.iter().position(|point| point.id == line.tip) else {
            return Ok(true);
        };
        let merged = (newer[..index].iter()).any(|point| point.parents[1..].contains(&line.tip));
        Ok(merged || index >= FOLD_AFTER)
    }

    /// The label of the current point, which undo takes back. `None` at the start.
    pub(crate) fn undo_label(&self) -> Option<String> {
        let commit = self.commit(self.head).ok()?;
        (!commit.parents.is_empty()).then(|| commit.label.clone())
    }

    pub(crate) fn redo_label(&self) -> Option<String> {
        let child = self.newest_child(self.head).ok()??;
        Some(self.commit(child).ok()?.label.clone())
    }

    /// Where undo goes: the point the current one came from.
    pub(crate) fn undo_target(&self) -> io::Result<Option<PointId>> {
        Ok(self.commit(self.head)?.parents.first().copied())
    }

    /// Where redo goes: the newest point that came from the current one.
    pub(crate) fn newest_child(&self, of: PointId) -> io::Result<Option<PointId>> {
        let time = self.commit(of)?.time;
        let mut newest: Option<(u64, PointId)> = None;
        // Children are newer than their parent, so the walk stops at its time.
        let mut heap: BinaryHeap<(u64, PointId)> = BinaryHeap::new();
        let mut queued = HashSet::new();
        for line in &self.lines {
            if queued.insert(line.tip) {
                heap.push((self.commit(line.tip)?.time, line.tip));
            }
        }
        while let Some((point_time, id)) = heap.pop() {
            if point_time <= time {
                break;
            }
            let commit = self.commit(id)?;
            if commit.parents.first() == Some(&of) && newest.is_none_or(|(at, _)| point_time > at) {
                newest = Some((point_time, id));
            }
            for parent in &commit.parents {
                if queued.insert(*parent) {
                    heap.push((self.commit(*parent)?.time, *parent));
                }
            }
        }
        Ok(newest.map(|(_, id)| id))
    }

    /// Reads the folder and saves it as a point when it differs from the current one. Returns
    /// whether the folder changed. Outside changes join the point before when that is one of
    /// them, heard less than [`OUTSIDE_POINT_WINDOW`] before or in the same request: the point
    /// is replaced, and when the folder is back where that point began, it goes.
    pub(crate) fn save(&mut self, origin: Origin, label: &str) -> io::Result<bool> {
        let files = self.kept.read(&self.root, &self.repository)?;
        let tree = self.repository.write_tree(&files)?;
        let at = match origin {
            Origin::Edit => {
                self.last_outside = None;
                if tree == self.head_tree {
                    return Ok(false);
                }
                self.add_point(tree, label, None)?;
                return Ok(true);
            }
            Origin::Outside(at) => at,
        };
        let quiet_since_end = (self.request.as_ref())
            .and_then(|request| request.ended)
            .is_some_and(|ended| at.saturating_duration_since(ended) >= GROUPING_WINDOW);
        if quiet_since_end {
            self.request = None;
            self.last_outside = None;
        }
        let joins = self.last_outside.is_some_and(|last| {
            self.request.is_some() || at.saturating_duration_since(last) < OUTSIDE_POINT_WINDOW
        });
        if tree == self.head_tree {
            self.last_outside = joins.then_some(at);
            return Ok(false);
        }
        if joins {
            let kept = self.replace_head(tree)?;
            self.last_outside = kept.then_some(at);
        } else {
            let label = (self.request.as_ref()).map_or(label, |request| request.label.as_str());
            let label = label.to_string();
            self.add_point(tree, &label, None)?;
            self.last_outside = Some(at);
        }
        Ok(true)
    }

    /// Puts `tree` in place of the head, a point of outside changes that nothing came after.
    /// When `tree` is where that point began, the point goes. Returns whether a point is left.
    fn replace_head(&mut self, tree: PointId) -> io::Result<bool> {
        let old = self.head;
        let commit = self.commit(old)?;
        let Some(parent) = commit.parents.first().copied() else {
            return Ok(false);
        };
        if self.commit(parent)?.tree == tree {
            self.lines.retain(|line| line.start != old);
            for line in &mut self.lines {
                if line.tip == old {
                    line.tip = parent;
                }
            }
            self.move_head(parent)?;
            self.store()?;
            return Ok(false);
        }
        let new = self.repository.write_commit(&Commit {
            tree,
            parents: commit.parents.clone(),
            time: now_millis().max(commit.time),
            label: commit.label.clone(),
        })?;
        for line in &mut self.lines {
            if line.tip == old {
                line.tip = new;
            }
            if line.start == old {
                line.start = new;
            }
        }
        self.move_head(new)?;
        self.store()?;
        Ok(true)
    }

    /// Writes a point of `tree` after the head, and `merged` as its second parent.
    fn add_point(&mut self, tree: PointId, label: &str, merged: Option<PointId>) -> io::Result<()> {
        self.settle_undone()?;
        let parent = self.head;
        let mut time = now_millis().max(self.commit(parent)?.time + 1);
        if let Some(merged) = merged {
            time = time.max(self.commit(merged)?.time + 1);
        }
        let commit = Commit {
            tree,
            parents: [parent].into_iter().chain(merged).collect(),
            time,
            label: label.to_string(),
        };
        let id = self.repository.write_commit(&commit)?;
        // Undone work that is changed from becomes a version.
        let undone = self.undone_line_holding(parent)?;
        if let Some(index) = undone {
            let number = self.take_number();
            self.lines[index].number = Some(number);
        }
        match self.lines.iter_mut().find(|line| line.tip == parent) {
            Some(line) => line.tip = id,
            None => self.new_version(id),
        }
        self.since = Since::Nothing;
        self.move_head(id)?;
        self.store()
    }

    /// The undone line `point` is in. Undone runs are short, so each is walked whole.
    fn undone_line_holding(&self, point: PointId) -> io::Result<Option<usize>> {
        for (index, line) in self.lines.iter().enumerate() {
            if line.is_version() {
                continue;
            }
            let mut at = line.tip;
            loop {
                if at == point {
                    return Ok(Some(index));
                }
                if at == line.start {
                    break;
                }
                match self.commit(at)?.parents.first() {
                    Some(parent) => at = *parent,
                    None => break,
                }
            }
        }
        Ok(None)
    }

    /// After undo, small work that was taken back becomes an undone line of its own, so the
    /// next point continues the line. Big work stays, and the next point is a version.
    fn settle_undone(&mut self) -> io::Result<()> {
        let Since::Undid { from } = self.since else {
            return Ok(());
        };
        let Some(line) = self.lines.iter().position(|line| line.tip == from) else {
            return Ok(());
        };
        let mut run = Vec::new();
        let mut at = from;
        while at != self.head {
            run.push(at);
            if run.len() >= VERSION_POINTS {
                return Ok(());
            }
            match self.commit(at)?.parents.first() {
                Some(parent) => at = *parent,
                None => return Ok(()),
            }
        }
        let Some((&first, later)) = run.split_last() else {
            return Ok(());
        };
        // A line that begins inside the run: not one run of work.
        if later.contains(&self.lines[line].start) {
            return Ok(());
        }
        let span = self.commit(from)?.time - self.commit(first)?.time;
        if span > VERSION_TIME.as_millis() as u64 {
            return Ok(());
        }
        if self.lines[line].start == first {
            self.lines[line].number = None;
        } else {
            self.lines[line].tip = self.head;
            self.lines.push(Line {
                start: first,
                tip: from,
                number: None,
                name: None,
                named_by_hand: false,
                named_at: None,
                set_aside: false,
            });
        }
        Ok(())
    }

    fn take_number(&mut self) -> u32 {
        let number = self.next_number;
        self.next_number += 1;
        number
    }

    fn new_version(&mut self, start: PointId) {
        let number = self.take_number();
        self.lines.push(Line {
            start,
            tip: start,
            number: Some(number),
            name: None,
            named_by_hand: false,
            named_at: None,
            set_aside: false,
        });
    }

    fn move_head(&mut self, to: PointId) -> io::Result<()> {
        self.head_tree = self.commit(to)?.tree;
        self.head = to;
        self.repository.set_head(to)?;
        self.generation += 1;
        Ok(())
    }

    /// Writes the lines and the refs plain git shows them by.
    fn store(&mut self) -> io::Result<()> {
        self.generation += 1;
        let saved = SavedLines {
            lines: self.lines.clone(),
            next_number: self.next_number,
        };
        let bytes = serde_json::to_vec_pretty(&saved).map_err(io::Error::other)?;
        super::storage::write_atomically(&self.repository.folder().join(LINES_FILE), &bytes)?;
        let versions = (self.lines.iter())
            .filter_map(|line| Some((format!("{}", line.number?), line.tip)))
            .collect();
        let undone = (self.lines.iter())
            .filter(|line| !line.is_version())
            .map(|line| (line.start.short(), line.tip))
            .collect();
        self.repository.set_refs("versions", &versions)?;
        self.repository.set_refs("undone", &undone)
    }

    /// Writes the files of `to` into the folder and makes it the current point, which is no
    /// new point. Returns every path written or removed. Save what the folder holds first.
    pub(crate) fn go(&mut self, to: PointId, going: Going) -> io::Result<Vec<PathBuf>> {
        let now = self.kept.read(&self.root, &self.repository)?;
        let wanted = self.repository.read_tree(self.commit(to)?.tree)?;
        let changed = folder::write(&self.root, &self.repository, &now, &wanted)?;
        self.since = match (going, self.since) {
            (Going::Undo, Since::Nothing) => Since::Undid { from: self.head },
            (Going::Redo, Since::Undid { from }) if from == to => Since::Nothing,
            (Going::Undo | Going::Redo, since) => since,
            (Going::Back, _) => Since::WentBack,
        };
        self.last_outside = None;
        self.move_head(to)?;
        Ok(changed)
    }

    /// Merges the version that begins at `start` into the current point, file by file: a file
    /// only that version changed comes over, and where both changed one, that version's wins,
    /// or with `keep_mine` the current one. The merge is a point with two parents. Returns
    /// every path written or removed.
    pub(crate) fn merge(&mut self, start: PointId, keep_mine: bool) -> io::Result<Vec<PathBuf>> {
        let Some(line) = self.line(start).cloned() else {
            return Ok(Vec::new());
        };
        let ours = self.files_at(Some(self.head))?;
        let theirs = self.files_at(Some(line.tip))?;
        let base = self.files_at(self.merge_base(self.head, line.tip)?)?;
        let mut merged = ours.clone();
        for path in ours.keys().chain(theirs.keys()) {
            let (mine, other, before) = (ours.get(path), theirs.get(path), base.get(path));
            if other == before || (keep_mine && mine != before) {
                continue;
            }
            match other {
                Some(blob) => _ = merged.insert(path.clone(), *blob),
                None => _ = merged.remove(path),
            }
        }
        let tree = self.repository.write_tree(&merged)?;
        let now = self.kept.read(&self.root, &self.repository)?;
        self.last_outside = None;
        self.add_point(tree, &format!("Merge {}", line.title()), Some(line.tip))?;
        folder::write(&self.root, &self.repository, &now, &merged)
    }

    fn files_at(&self, point: Option<PointId>) -> io::Result<Files> {
        match point {
            Some(point) => self.repository.read_tree(self.commit(point)?.tree),
            None => Ok(Files::new()),
        }
    }

    /// The newest point both `a` and `b` came from.
    fn merge_base(&self, a: PointId, b: PointId) -> io::Result<Option<PointId>> {
        let mut reached: HashMap<PointId, u8> = HashMap::from([(a, 1), (b, 2)]);
        let mut heap = BinaryHeap::from([(self.commit(a)?.time, a), (self.commit(b)?.time, b)]);
        while let Some((_, id)) = heap.pop() {
            let from = reached.get(&id).copied().unwrap_or(0);
            if from == 3 {
                return Ok(Some(id));
            }
            for parent in self.commit(id)?.parents.iter() {
                let known = reached.entry(*parent).or_insert(0);
                if *known | from != *known {
                    *known |= from;
                    heap.push((self.commit(*parent)?.time, *parent));
                }
            }
        }
        Ok(None)
    }

    /// Makes undone work a version.
    pub(crate) fn keep_as_version(&mut self, start: PointId) -> io::Result<()> {
        let number = self.next_number;
        let Some(line) = self.line_mut(start).filter(|line| !line.is_version()) else {
            return Ok(());
        };
        line.number = Some(number);
        self.next_number += 1;
        self.store()
    }

    /// Folds a version away by hand, or brings it back.
    pub(crate) fn set_aside(&mut self, start: PointId, aside: bool) -> io::Result<()> {
        if let Some(line) = self.line_mut(start) {
            line.set_aside = aside;
        }
        self.store()
    }

    /// Names a version by hand. `None` gives naming back.
    pub(crate) fn rename(&mut self, start: PointId, name: Option<String>) -> io::Result<()> {
        let name = name.map(|name| name.trim().to_string());
        if let Some(line) = self.line_mut(start) {
            line.named_by_hand = name.as_ref().is_some_and(|name| !name.is_empty());
            line.name = name.filter(|_| line.named_by_hand);
            line.named_at = None;
        }
        self.store()
    }

    /// The name naming found for a version at its tip `at`. Not over a name given by hand.
    pub(crate) fn name(&mut self, start: PointId, name: String, at: PointId) -> io::Result<()> {
        if let Some(line) = self.line_mut(start).filter(|line| !line.named_by_hand) {
            line.name = Some(name);
            line.named_at = Some(at);
        }
        self.store()
    }

    fn line_mut(&mut self, start: PointId) -> Option<&mut Line> {
        self.lines.iter_mut().find(|line| line.start == start)
    }

    /// The files that differ between two points, with their bytes on each side. For naming a
    /// version from what it changed.
    pub fn changes(&self, from: Option<PointId>, to: PointId) -> io::Result<Vec<FileChange>> {
        let before = self.files_at(from)?;
        let after = self.files_at(Some(to))?;
        let mut paths: Vec<&String> = before.keys().chain(after.keys()).collect();
        paths.sort();
        paths.dedup();
        let read = |blob: Option<&PointId>| {
            blob.map(|blob| self.repository.read_blob(*blob))
                .transpose()
        };
        let mut changes = Vec::new();
        for path in paths {
            let (old, new) = (before.get(path), after.get(path));
            if old != new {
                changes.push(FileChange {
                    path: path.clone(),
                    before: read(old)?,
                    after: read(new)?,
                });
            }
        }
        Ok(changes)
    }

    /// See [`Project::begin_request`](super::Project::begin_request).
    pub(crate) fn begin_request(&mut self, label: &str) {
        self.request = Some(Request {
            label: label.to_string(),
            ended: None,
        });
        // The request starts its own point, even right after other outside changes.
        self.last_outside = None;
    }

    /// See [`Project::end_request_at`](super::Project::end_request_at).
    pub(crate) fn end_request_at(&mut self, at: Instant) {
        if let Some(request) = &mut self.request {
            request.ended.get_or_insert(at);
        }
    }
}

/// How [`History::go`] moves.
#[derive(Copy, Clone)]
pub(crate) enum Going {
    Undo,
    Redo,
    /// A point chosen in the history, or undo that keeps a version.
    Back,
}

/// A file two points hold differently. `None` is no file.
pub struct FileChange {
    pub path: String,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

fn now_millis() -> u64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    since_epoch.as_millis() as u64
}
