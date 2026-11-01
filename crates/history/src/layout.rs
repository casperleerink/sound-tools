//! What the tree of the history shows: which points and versions are rows by default, what
//! folds away, and the lanes of the lines for the rows in view.
//!
//! A version is a line of points: a point continues the version of its first parent when it is
//! that parent's first child that was not undone, else it starts a version of its own. A version shows its first
//! and its latest point; what lies between folds into one row, unless it is where the project
//! is now or a merge. A version that was undone shows as "undone" on the point it left.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use sound_core::history::{History, Named, ObjectId};

/// A version that has not been touched while this many newer points were made folds away.
pub const FOLD_AFTER: usize = 30;

/// The points of a history, as the layout reads them.
pub struct Graph {
    nodes: Vec<Node>,
    now: Option<usize>,
    names: BTreeMap<ObjectId, Named>,
    aside: BTreeSet<ObjectId>,
    undone: BTreeSet<ObjectId>,
}

struct Node {
    id: ObjectId,
    /// Indices into the nodes, first parent first.
    parents: Vec<usize>,
    time: u64,
}

impl Graph {
    pub fn of(history: &History) -> Self {
        let index: BTreeMap<ObjectId, usize> = (history.order().iter())
            .enumerate()
            .map(|(index, id)| (*id, index))
            .collect();
        let mut names = BTreeMap::new();
        let mut aside = BTreeSet::new();
        let mut undone = BTreeSet::new();
        let nodes = (history.order().iter())
            .filter_map(|id| history.point(*id))
            .map(|point| {
                if let Some(named) = history.name(point.id) {
                    names.insert(point.id, named.clone());
                }
                if history.is_aside(point.id) {
                    aside.insert(point.id);
                }
                if history.is_undone(point.id) {
                    undone.insert(point.id);
                }
                Node {
                    id: point.id,
                    parents: (point.parents.iter())
                        .filter_map(|parent| index.get(parent).copied())
                        .collect(),
                    time: point.time,
                }
            })
            .collect();
        Self {
            nodes,
            now: history.now().and_then(|now| index.get(&now).copied()),
            names,
            aside,
            undone,
        }
    }
}

/// A row of the tree: one point, or the points of a version folded into one.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The point, or the first of the folded ones.
    pub point: ObjectId,
    pub version: usize,
    /// Milliseconds since 1970.
    pub time: u64,
    pub now: bool,
    /// The latest point of its version, which carries the version's name.
    pub tip: bool,
    /// The points this row stands for, when it is a fold; open, the newest of them carries it.
    pub changes: Option<Fold>,
    /// Versions that start from a point of this row and fold away.
    pub more: Option<Fold>,
    /// Points that were undone from a point of this row, see
    /// [`History::note_undone`](sound_core::history::History::note_undone).
    pub undone: Option<Fold>,
}

impl Row {
    /// It stands for points folded into it, rather than for one.
    pub fn is_fold(&self) -> bool {
        self.changes.as_ref().is_some_and(|fold| !fold.open)
    }
}

/// Something folded, and the key that opens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    pub key: ObjectId,
    pub count: usize,
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub start: ObjectId,
    pub tip: ObjectId,
    /// The point it split from. `None` for the first version.
    pub split: Option<ObjectId>,
    pub name: Option<Named>,
    pub aside: bool,
    /// Taken back with undo, see
    /// [`History::note_undone`](sound_core::history::History::note_undone).
    pub undone: bool,
    /// Whether it is now, which never folds away.
    pub now: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum EdgeKind {
    /// To the first parent: the line runs in the child's lane down to the parent.
    Main,
    /// To a merged parent: the line runs in the parent's lane up to the child.
    Merge,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    child: usize,
    parent: usize,
    kind: EdgeKind,
    /// The version whose lane the line runs in.
    version: usize,
}

/// The tree, newest row first.
#[derive(Default)]
pub struct Layout {
    pub rows: Vec<Row>,
    pub versions: Vec<Version>,
    edges: Vec<Edge>,
}

/// What one row draws, in lanes of [`Layout::lanes`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawing {
    /// The lane of the row's dot, and its version.
    pub dot: usize,
    pub version: usize,
    pub lines: Vec<Line>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub shape: Shape,
    /// The version whose colour it has.
    pub version: usize,
    pub merge: bool,
}

/// Parts of a line within one row. A lane is a column; the dot is in the middle of the row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Through the whole row.
    Through(usize),
    /// From the dot down to the bottom.
    Down(usize),
    /// From the top down to the dot.
    Up(usize),
    /// From the top of one lane, curving to the dot in another.
    FromTop { from: usize, to: usize },
    /// From the bottom of one lane, curving up to the dot in another.
    FromBottom { from: usize, to: usize },
}

impl Graph {
    /// The tree with what `open` names unfolded: keys of [`Fold`]s.
    pub fn layout(&self, open: &BTreeSet<ObjectId>) -> Layout {
        let count = self.nodes.len();
        let mut first_child: Vec<Option<usize>> = vec![None; count];
        let mut version_of = vec![0; count];
        let mut starts: Vec<usize> = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            // Undone work is a line of its own, and what came after the undo goes on.
            let undone = self.undone.contains(&node.id);
            let continues = (node.parents.first().copied())
                .filter(|parent| !undone && first_child.get(*parent).copied().flatten().is_none());
            match continues {
                Some(parent) => {
                    first_child[parent] = Some(index);
                    version_of[index] = version_of[parent];
                }
                None => {
                    if let Some(parent) = node.parents.first().filter(|_| !undone) {
                        first_child[*parent].get_or_insert(index);
                    }
                    version_of[index] = starts.len();
                    starts.push(index);
                }
            }
        }
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); starts.len()];
        for (index, version) in version_of.iter().enumerate() {
            members[*version].push(index);
        }
        let tips: Vec<usize> = (members.iter())
            .map(|points| points.last().copied().unwrap_or(0))
            .collect();
        let merged: BTreeSet<usize> = (self.nodes.iter())
            .flat_map(|node| node.parents.iter().skip(1))
            .map(|parent| version_of[*parent])
            .collect();
        let now_version = self.now.map(|now| version_of[now]);
        let split = |version: usize| self.nodes[starts[version]].parents.first().copied();
        let folds = |version: usize| {
            let start = self.nodes[starts[version]].id;
            Some(version) != now_version
                && (merged.contains(&version)
                    || self.aside.contains(&start)
                    || self.undone.contains(&start)
                    || count - 1 - tips[version] >= FOLD_AFTER)
        };
        let mut shown: BTreeSet<usize> = (0..starts.len())
            .filter(|version| {
                !folds(*version)
                    || split(*version).is_some_and(|point| open.contains(&self.nodes[point].id))
            })
            .collect();
        // The versions a shown one comes from stay, so it hangs from something.
        let mut waiting: Vec<usize> = shown.iter().copied().collect();
        while let Some(version) = waiting.pop() {
            if let Some(point) = split(version)
                && shown.insert(version_of[point])
            {
                waiting.push(version_of[point]);
            }
        }

        let is_shown_point = |index: usize| {
            let version = version_of[index];
            starts[version] == index
                || tips[version] == index
                || Some(index) == self.now
                || self.nodes[index].parents.len() > 1
        };
        // Rows with the index they sort by, newest first. A fold sorts by its oldest point, so
        // every line from a point in it runs up to a newer row.
        let mut rows: Vec<(usize, Row)> = Vec::new();
        let mut row_of: BTreeMap<usize, usize> = BTreeMap::new();
        for version in &shown {
            let mut run: Vec<usize> = Vec::new();
            for index in members[*version].iter().copied() {
                if !is_shown_point(index) {
                    run.push(index);
                    continue;
                }
                let key = run.first().map(|first| self.nodes[*first].id);
                match key {
                    Some(key) if run.len() > 1 && !open.contains(&key) => {
                        let mut row = self.row(run[0], *version, &tips);
                        row.changes = Some(Fold {
                            key,
                            count: run.len(),
                            open: false,
                        });
                        for hidden in &run {
                            row_of.insert(*hidden, run[0]);
                        }
                        rows.push((run[0], row));
                    }
                    _ => {
                        let newest = run.last().copied();
                        for hidden in run.iter().copied() {
                            let mut row = self.row(hidden, *version, &tips);
                            if Some(hidden) == newest && run.len() > 1 {
                                row.changes = key.map(|key| Fold {
                                    key,
                                    count: run.len(),
                                    open: true,
                                });
                            }
                            row_of.insert(hidden, hidden);
                            rows.push((hidden, row));
                        }
                    }
                }
                run.clear();
                row_of.insert(index, index);
                rows.push((index, self.row(index, *version, &tips)));
            }
        }
        rows.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
        let position: BTreeMap<usize, usize> = (rows.iter().enumerate())
            .map(|(position, (index, _))| (*index, position))
            .collect();
        let row_at = |index: usize| {
            row_of
                .get(&index)
                .and_then(|row| position.get(row))
                .copied()
        };

        // Versions folded away, at the row of the point they split from.
        for version in 0..starts.len() {
            let Some(point) = split(version) else {
                continue;
            };
            let key = self.nodes[point].id;
            let folded = folds(version) && (open.contains(&key) || !shown.contains(&version));
            if let (true, Some(row)) = (folded, row_at(point)) {
                let undone = self.undone.contains(&self.nodes[starts[version]].id);
                let row = &mut rows[row].1;
                let fold = match undone {
                    true => &mut row.undone,
                    false => &mut row.more,
                };
                let fold = fold.get_or_insert(Fold {
                    key,
                    count: 0,
                    open: open.contains(&key),
                });
                // Undone work counts its points, versions count themselves.
                fold.count += match undone {
                    true => members[version].len(),
                    false => 1,
                };
            }
        }

        let mut edges = BTreeSet::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let Some(child) = row_at(index) else {
                continue;
            };
            for (place, parent) in node.parents.iter().enumerate() {
                let Some(parent_row) = row_at(*parent).filter(|row| *row != child) else {
                    continue;
                };
                let (kind, version) = match place {
                    0 => (EdgeKind::Main, version_of[index]),
                    _ => (EdgeKind::Merge, version_of[*parent]),
                };
                edges.insert(Edge {
                    child,
                    parent: parent_row,
                    kind,
                    version,
                });
            }
        }
        let versions = (0..starts.len())
            .map(|version| {
                let start = self.nodes[starts[version]].id;
                Version {
                    start,
                    tip: self.nodes[tips[version]].id,
                    split: split(version).map(|point| self.nodes[point].id),
                    name: self.names.get(&start).cloned(),
                    aside: self.aside.contains(&start),
                    undone: self.undone.contains(&start),
                    now: Some(version) == now_version,
                }
            })
            .collect();
        Layout {
            rows: rows.into_iter().map(|(_, row)| row).collect(),
            versions,
            edges: edges.into_iter().collect(),
        }
    }

    fn row(&self, index: usize, version: usize, tips: &[usize]) -> Row {
        let node = &self.nodes[index];
        Row {
            point: node.id,
            version,
            time: node.time,
            now: Some(index) == self.now,
            tip: tips[version] == index,
            changes: None,
            more: None,
            undone: None,
        }
    }
}

impl Layout {
    /// The lane of every version that has a row or a line in `visible`, older versions on the
    /// left. Scrolling changes which versions are in view, and so the lanes.
    pub fn lanes(&self, visible: Range<usize>) -> BTreeMap<usize, usize> {
        let mut versions: BTreeSet<usize> = (self.rows.get(visible.clone()).into_iter().flatten())
            .map(|row| row.version)
            .collect();
        for edge in &self.edges {
            if edge.child < visible.end && edge.parent >= visible.start {
                versions.insert(edge.version);
            }
        }
        versions
            .into_iter()
            .enumerate()
            .map(|(lane, version)| (version, lane))
            .collect()
    }

    /// What `row` draws with these lanes.
    pub fn drawing(&self, row: usize, lanes: &BTreeMap<usize, usize>) -> Drawing {
        let lane = |version: usize| lanes.get(&version).copied().unwrap_or(0);
        let version_at = |row: usize| self.rows.get(row).map_or(0, |row| row.version);
        let mut lines = Vec::new();
        for edge in &self.edges {
            if !(edge.child..=edge.parent).contains(&row) {
                continue;
            }
            let own = lane(edge.version);
            let shape = match edge.kind {
                EdgeKind::Main if row == edge.child => Shape::Down(own),
                EdgeKind::Main if row == edge.parent => match lane(version_at(edge.parent)) {
                    to if to == own => Shape::Up(own),
                    to => Shape::FromTop { from: own, to },
                },
                EdgeKind::Merge if row == edge.parent => Shape::Up(own),
                EdgeKind::Merge if row == edge.child => match lane(version_at(edge.child)) {
                    to if to == own => Shape::Down(own),
                    to => Shape::FromBottom { from: own, to },
                },
                _ => Shape::Through(own),
            };
            lines.push(Line {
                shape,
                version: edge.version,
                merge: edge.kind == EdgeKind::Merge,
            });
        }
        Drawing {
            dot: lane(version_at(row)),
            version: version_at(row),
            lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A graph from the parent indices of each point, oldest first; ids are the index.
    fn graph(points: &[&[usize]], now: usize) -> Graph {
        let nodes = (points.iter().enumerate())
            .map(|(index, parents)| Node {
                id: id(index),
                parents: parents.to_vec(),
                time: index as u64,
            })
            .collect();
        Graph {
            nodes,
            now: Some(now),
            names: BTreeMap::new(),
            aside: BTreeSet::new(),
            undone: BTreeSet::new(),
        }
    }

    fn id(index: usize) -> ObjectId {
        ObjectId::parse(&format!("{index:040x}")).unwrap()
    }

    fn points(layout: &Layout) -> Vec<ObjectId> {
        layout.rows.iter().map(|row| row.point).collect()
    }

    /// A line of `length` points, each from the one before.
    fn line(length: usize) -> Vec<Vec<usize>> {
        (0..length)
            .map(|index| index.checked_sub(1).into_iter().collect())
            .collect()
    }

    #[test]
    fn a_version_shows_its_first_and_latest_point_with_the_rest_folded_between() {
        let chain = line(6);
        let chain: Vec<&[usize]> = chain.iter().map(Vec::as_slice).collect();
        let layout = graph(&chain, 5).layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(5), id(1), id(0)]);
        let fold = layout.rows[1].changes.clone().unwrap();
        assert_eq!((fold.key, fold.count, fold.open), (id(1), 4, false));
        let lanes = layout.lanes(0..3);
        assert_eq!(layout.drawing(1, &lanes).lines.len(), 2);

        let layout = graph(&chain, 5).layout(&BTreeSet::from([id(1)]));
        assert_eq!(layout.rows.len(), 6);
        assert!(
            layout.rows[1]
                .changes
                .as_ref()
                .is_some_and(|fold| fold.open)
        );
        // Where the project is shows by itself, and a fold of one is no fold.
        let layout = graph(&chain, 3).layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(5), id(4), id(3), id(1), id(0)]);
    }

    #[test]
    fn a_version_from_a_folded_point_hangs_from_the_fold() {
        // 0 1 2 3 4 the first version; 5 a version from 2, now.
        let graph = graph(&[&[], &[0], &[1], &[2], &[3], &[2]], 5);
        let layout = graph.layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(5), id(4), id(1), id(0)]);
        let lanes = layout.lanes(0..4);
        assert_eq!(lanes.len(), 2);
        // The new version's line curves into the fold.
        let lines = layout.drawing(2, &lanes).lines;
        assert!(
            lines
                .iter()
                .any(|line| line.shape == Shape::FromTop { from: 1, to: 0 })
        );
    }

    #[test]
    fn a_version_folds_after_enough_newer_points() {
        // 0 1 and on: the first version. 2: a version from 0, then the first goes on.
        let mut points: Vec<Vec<usize>> = vec![vec![], vec![0], vec![0]];
        points
            .extend((3..3 + FOLD_AFTER).map(|index| vec![if index == 3 { 1 } else { index - 1 }]));
        let shown = |count: usize| {
            let points: Vec<&[usize]> = points[..count].iter().map(Vec::as_slice).collect();
            let graph = graph(&points, count - 1);
            let layout = graph.layout(&BTreeSet::new());
            let shown = layout.rows.iter().any(|row| row.point == id(2));
            (shown, layout.rows.iter().find_map(|row| row.more.clone()))
        };
        assert!(shown(points.len() - 1).0);
        let (shown, more) = shown(points.len());
        assert!(!shown);
        assert_eq!(more.map(|fold| (fold.key, fold.count)), Some((id(0), 1)));
    }

    #[test]
    fn undone_work_shows_on_the_point_it_left_and_opens_there() {
        // 0 1 2: undone back to 0, then 3 changed there and is now.
        let mut graph = graph(&[&[], &[0], &[1], &[0]], 3);
        graph.undone.insert(id(1));
        let layout = graph.layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(3), id(0)]);
        let undone = layout.rows[1].undone.clone().unwrap();
        assert_eq!((undone.key, undone.count), (id(0), 2));
        assert!(layout.rows[1].more.is_none());
        let layout = graph.layout(&BTreeSet::from([id(0)]));
        assert_eq!(points(&layout), [id(3), id(2), id(1), id(0)]);
    }

    #[test]
    fn a_merged_version_folds_and_its_line_is_dashed_when_shown() {
        // 0 1 the first version; 2 a version from 0; 3 merges 2 into 1, and is now.
        let graph = graph(&[&[], &[0], &[0], &[1, 2]], 3);
        let layout = graph.layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(3), id(1), id(0)]);
        let layout = graph.layout(&BTreeSet::from([id(0)]));
        assert_eq!(points(&layout), [id(3), id(2), id(1), id(0)]);
        let lanes = layout.lanes(0..4);
        let lines = layout.drawing(0, &lanes).lines;
        assert!(
            lines
                .iter()
                .any(|line| line.merge && line.shape == Shape::FromBottom { from: 1, to: 0 })
        );
    }

    #[test]
    fn lanes_follow_the_rows_in_view() {
        // 0 1 the first version; 2 3 a version from 0; 4 a version from 2, now.
        let graph = graph(&[&[], &[0], &[0], &[2], &[2]], 4);
        let layout = graph.layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(4), id(3), id(2), id(1), id(0)]);
        // At the top, the old version is out of view: only two lanes.
        assert_eq!(layout.lanes(0..2).len(), 2);
        // Scrolled down to the first points, the newest version is out of view.
        let lanes = layout.lanes(3..5);
        assert_eq!(lanes.len(), 2);
        assert!(!lanes.contains_key(&2));
        assert_eq!(layout.drawing(3, &lanes).dot, 0);
    }
}
