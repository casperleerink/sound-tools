//! What the tree of the history shows: which points and versions are rows by default, what
//! folds away, and the lanes of the lines for the rows in view.
//!
//! A version is a line of points: a point continues the version of its first parent when it is
//! that parent's first child, else it starts a version of its own.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::git::ObjectId;
use crate::store::History;

/// A version that has not been touched while this many newer moments were made folds away.
pub const FOLD_AFTER_MOMENTS: usize = 20;

/// The points of a history, as the layout reads them.
pub struct Graph {
    nodes: Vec<Node>,
    now: Option<usize>,
    names: BTreeMap<ObjectId, String>,
    aside: BTreeSet<ObjectId>,
}

struct Node {
    id: ObjectId,
    /// Indices into the nodes, first parent first.
    parents: Vec<usize>,
    label: String,
    moment: Option<String>,
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
        let nodes = history
            .order()
            .iter()
            .filter_map(|id| history.point(*id))
            .map(|point| {
                if let Some(name) = history.name(point.id) {
                    names.insert(point.id, name.to_string());
                }
                if history.is_aside(point.id) {
                    aside.insert(point.id);
                }
                Node {
                    id: point.id,
                    parents: (point.parents.iter())
                        .filter_map(|parent| index.get(parent).copied())
                        .collect(),
                    label: point.label.clone(),
                    moment: history.moment(point.id).map(str::to_string),
                    time: point.time,
                }
            })
            .collect();
        Self {
            nodes,
            now: history.now().and_then(|now| index.get(&now).copied()),
            names,
            aside,
        }
    }
}

/// A row of the tree: one point.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub point: ObjectId,
    pub version: usize,
    /// The moment's name, or the change that led here.
    pub label: String,
    pub moment: bool,
    /// Milliseconds since 1970.
    pub time: u64,
    pub now: bool,
    /// The latest point of its version, which carries the version's name.
    pub tip: bool,
    /// Points of the version before this one that fold into it.
    pub changes: Option<Fold>,
    /// Versions that start here and fold away.
    pub more: Option<Fold>,
    /// A point that sits inside an open fold of changes.
    pub inside_fold: bool,
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
    /// The name it was given, else its first moment's. The first version has none until it
    /// is given one: it is where everything starts.
    pub name: Option<String>,
    pub aside: bool,
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
            let continues = node
                .parents
                .first()
                .copied()
                .filter(|parent| first_child.get(*parent).copied().flatten().is_none());
            match continues {
                Some(parent) => {
                    first_child[parent] = Some(index);
                    version_of[index] = version_of[parent];
                }
                None => {
                    if let Some(parent) = node.parents.first() {
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
        let tips: Vec<usize> = members
            .iter()
            .map(|points| points.last().copied().unwrap_or(0))
            .collect();
        let merged: BTreeSet<usize> = (self.nodes.iter())
            .flat_map(|node| node.parents.iter().skip(1))
            .map(|parent| version_of[*parent])
            .collect();
        let now_version = self.now.map(|now| version_of[now]);
        let moments_after = |index: usize| {
            (self.nodes.iter().skip(index + 1))
                .filter(|node| node.moment.is_some())
                .count()
        };
        // The point a version splits from, and the version that has it.
        let split = |version: usize| self.nodes[starts[version]].parents.first().copied();
        let folds = |version: usize| {
            let start = self.nodes[starts[version]].id;
            Some(version) != now_version
                && (!members[version]
                    .iter()
                    .any(|index| self.nodes[*index].moment.is_some())
                    || merged.contains(&version)
                    || self.aside.contains(&start)
                    || moments_after(tips[version]) >= FOLD_AFTER_MOMENTS)
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

        let splits_here =
            |index: usize| (0..starts.len()).filter(move |version| split(*version) == Some(index));
        let is_shown_point = |index: usize| {
            let node = &self.nodes[index];
            node.moment.is_some()
                || tips[version_of[index]] == index
                || splits_here(index).next().is_some()
                || Some(index) == self.now
                || index == 0
                || node.parents.len() > 1
        };
        // Each point's row, newest first; hidden points get the row they fold into.
        let mut rows: Vec<(usize, Row)> = Vec::new();
        for version in &shown {
            let mut run: Vec<usize> = Vec::new();
            for index in &members[*version] {
                let index = *index;
                if !is_shown_point(index) {
                    run.push(index);
                    continue;
                }
                let changes = run.first().map(|first| {
                    let key = self.nodes[*first].id;
                    Fold {
                        key,
                        count: run.len(),
                        open: open.contains(&key),
                    }
                });
                let id = self.nodes[index].id;
                let open_here = open.contains(&id);
                let folded_here: Vec<usize> = splits_here(index)
                    .filter(|each| folds(*each) && (open_here || !shown.contains(each)))
                    .collect();
                let more = (!folded_here.is_empty()).then(|| Fold {
                    key: id,
                    count: folded_here.len(),
                    open: open_here,
                });
                rows.push((
                    index,
                    self.row(index, *version, &tips, changes.clone(), more, false),
                ));
                if changes.is_some_and(|fold| fold.open) {
                    for hidden in run.drain(..) {
                        rows.push((hidden, self.row(hidden, *version, &tips, None, None, true)));
                    }
                }
                run.clear();
            }
        }
        rows.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
        let mut row_of: BTreeMap<usize, usize> = BTreeMap::new();
        for (row, (index, _)) in rows.iter().enumerate() {
            row_of.insert(*index, row);
        }
        // Hidden points of a shown version: the row of the next shown point of their version.
        for version in &shown {
            let mut pending = Vec::new();
            for index in &members[*version] {
                match row_of.get(index) {
                    Some(row) => {
                        let row = *row;
                        for hidden in pending.drain(..) {
                            row_of.insert(hidden, row);
                        }
                    }
                    None => pending.push(*index),
                }
            }
        }
        let mut edges = BTreeSet::new();
        for (index, node) in self.nodes.iter().enumerate() {
            let Some(child) = row_of.get(&index).copied() else {
                continue;
            };
            for (position, parent) in node.parents.iter().enumerate() {
                let Some(parent_row) = row_of.get(parent).copied() else {
                    continue;
                };
                if parent_row == child {
                    continue;
                }
                let (kind, version) = match position {
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
                let first_moment =
                    (members[version].iter()).find_map(|index| self.nodes[*index].moment.clone());
                let first = self.nodes[starts[version]].parents.is_empty();
                let name = (self.names.get(&start).cloned()).or(first_moment.filter(|_| !first));
                Version {
                    start,
                    tip: self.nodes[tips[version]].id,
                    name,
                    aside: self.aside.contains(&start),
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

    fn row(
        &self,
        index: usize,
        version: usize,
        tips: &[usize],
        changes: Option<Fold>,
        more: Option<Fold>,
        inside_fold: bool,
    ) -> Row {
        let node = &self.nodes[index];
        Row {
            point: node.id,
            version,
            label: node.moment.clone().unwrap_or_else(|| node.label.clone()),
            moment: node.moment.is_some(),
            time: node.time,
            now: Some(index) == self.now,
            tip: tips[version] == index,
            changes,
            more,
            inside_fold,
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
pub(crate) mod tests {
    use super::*;

    /// A graph from `(parent indices, moment)` per point, oldest first; ids are the index.
    pub(crate) fn graph(points: &[(&[usize], bool)], now: usize) -> Graph {
        let nodes = points
            .iter()
            .enumerate()
            .map(|(index, (parents, moment))| Node {
                id: id(index),
                parents: parents.to_vec(),
                label: format!("change {index}"),
                moment: moment.then(|| format!("moment {index}")),
                time: index as u64,
            })
            .collect();
        Graph {
            nodes,
            now: Some(now),
            names: BTreeMap::new(),
            aside: BTreeSet::new(),
        }
    }

    pub(crate) fn id(index: usize) -> ObjectId {
        ObjectId::parse(&format!("{index:040x}")).unwrap()
    }

    fn points(layout: &Layout) -> Vec<ObjectId> {
        layout.rows.iter().map(|row| row.point).collect()
    }

    #[test]
    fn changes_between_moments_fold_into_the_next_shown_point() {
        // 0 moment, 1 2 changes, 3 moment, 4 change (now, the tip).
        let graph = graph(
            &[
                (&[], true),
                (&[0], false),
                (&[1], false),
                (&[2], true),
                (&[3], false),
            ],
            4,
        );
        let layout = graph.layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(4), id(3), id(0)]);
        let fold = layout.rows[1].changes.clone().unwrap();
        assert_eq!((fold.key, fold.count, fold.open), (id(1), 2, false));
        // The line runs from the moment down to the first point, through the folded ones.
        let lanes = layout.lanes(0..3);
        assert_eq!(layout.drawing(1, &lanes).lines[0].shape, Shape::Up(0));

        let open = layout.rows[1].changes.clone().unwrap().key;
        let layout = graph.layout(&BTreeSet::from([open]));
        assert_eq!(points(&layout), [id(4), id(3), id(2), id(1), id(0)]);
        assert!(layout.rows[2].inside_fold);
    }

    #[test]
    fn a_try_that_never_got_a_moment_folds_under_its_split() {
        // 0 1 4 the first version, now at 4; 2 3 a try from 0 that never got a moment.
        let graph = graph(
            &[
                (&[], true),
                (&[0], true),
                (&[0], false),
                (&[2], false),
                (&[1], true),
            ],
            4,
        );
        let layout = graph.layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(4), id(1), id(0)]);
        let more = layout.rows[2].more.clone().unwrap();
        assert_eq!((more.key, more.count, more.open), (id(0), 1, false));
        let layout = graph.layout(&BTreeSet::from([id(0)]));
        assert_eq!(points(&layout), [id(4), id(3), id(1), id(0)]);
        assert_eq!(
            layout.rows[1].changes.as_ref().map(|fold| fold.count),
            Some(1)
        );
    }

    #[test]
    fn a_version_folds_after_enough_newer_moments() {
        // 0 1 and on: the first version, with a moment at every point. 2: a version from 0.
        let mut chain: Vec<[usize; 1]> = vec![[1]];
        chain.extend((1..FOLD_AFTER_MOMENTS).map(|step| [2 + step]));
        let mut points: Vec<(&[usize], bool)> = vec![(&[], true), (&[0], true), (&[0], true)];
        points.extend(chain.iter().map(|parent| (parent.as_slice(), true)));
        let shown = |count: usize| {
            let graph = graph(&points[..count], count - 1);
            graph
                .layout(&BTreeSet::new())
                .rows
                .iter()
                .any(|row| row.point == id(2))
        };
        assert!(shown(points.len() - 1));
        assert!(!shown(points.len()));
    }

    #[test]
    fn the_version_now_and_those_it_comes_from_never_fold() {
        // 0 1 2: changes with no moment, and the project is at the last.
        let graph = graph(&[(&[], false), (&[0], false), (&[1], false)], 2);
        let layout = graph.layout(&BTreeSet::new());
        assert_eq!(points(&layout), [id(2), id(0)]);
        assert!(layout.versions[0].now);
    }

    #[test]
    fn a_merged_version_folds_and_its_line_is_dashed_when_shown() {
        // 0 1 the first version; 2 a version from 0; 3 merges 2 into 1, and is now.
        let graph = graph(
            &[(&[], true), (&[0], true), (&[0], true), (&[1, 2], true)],
            3,
        );
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
        let graph = graph(
            &[
                (&[], true),
                (&[0], true),
                (&[0], true),
                (&[2], true),
                (&[2], true),
            ],
            4,
        );
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
