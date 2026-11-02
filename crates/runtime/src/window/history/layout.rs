//! Which points of the history are rows, and the lanes, folds and edges between them. Pure, so
//! the rules are tested without a window.
//!
//! A line is drawn when it is a version that is not folded away, holds the current point, was
//! shown by hand, or has a drawn line splitting off it. Its rows are the points that matter:
//! where it starts, where a drawn line splits off it, its latest point, the current point,
//! merges, the project start, and its oldest point read so far. The points between two rows of
//! a line are a fold, unless it was opened. A fold of one point is a row.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use sound_core::{Line, Point, PointId};

/// What the history holds, read once per change of it.
pub(super) struct Page {
    /// Newest first.
    pub(super) points: Vec<Point>,
    pub(super) lines: Vec<Line>,
    pub(super) current: PointId,
    /// The starts of the versions that are folded away.
    pub(super) folded_away: HashSet<PointId>,
}

/// What the composer opened in the history.
#[derive(Default)]
pub(super) struct Opened {
    /// Folds, by their newest point.
    pub(super) folds: HashSet<PointId>,
    /// Lines that would be hidden, by their start.
    pub(super) lines: HashSet<PointId>,
}

#[derive(Debug, PartialEq)]
pub(super) enum Name {
    None,
    /// The title of the line, on its latest point.
    Line(String),
    ProjectStart,
}

#[derive(Debug)]
pub(super) struct Row {
    pub(super) point: PointId,
    /// The start of its line.
    pub(super) line: PointId,
    pub(super) lane: usize,
    /// Which of the lane colours, the same for a line wherever it is.
    pub(super) color: usize,
    /// The latest point of its line.
    pub(super) tip: bool,
    pub(super) current: bool,
    pub(super) name: Name,
    pub(super) time: SystemTime,
    /// Versions folded away that split off here, by their start.
    pub(super) hidden_versions: Vec<PointId>,
    /// Undone work that leaves here, by its start, and how many points it has.
    pub(super) undone: Vec<PointId>,
    pub(super) undone_points: usize,
    /// Lines opened by hand that split off here.
    pub(super) opened: Vec<PointId>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) enum Stroke {
    Solid,
    /// Over a fold.
    Dotted,
    /// To a merged version.
    Dashed,
}

/// A stroke from one row down to another.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) struct Edge {
    /// Row and lane.
    pub(super) from: (usize, usize),
    pub(super) to: (usize, usize),
    pub(super) color: usize,
    pub(super) stroke: Stroke,
}

/// Points of a line between two of its rows.
#[derive(Debug, PartialEq)]
pub(super) struct Fold {
    /// Its newest point, which names it in [`Opened::folds`].
    pub(super) first: PointId,
    pub(super) lane: usize,
    /// The rows above and below it.
    pub(super) rows: (usize, usize),
    pub(super) count: usize,
    pub(super) open: bool,
}

#[derive(Debug, Default)]
pub(super) struct Layout {
    pub(super) rows: Vec<Row>,
    pub(super) lanes: usize,
    pub(super) edges: Vec<Edge>,
    pub(super) folds: Vec<Fold>,
}

impl Layout {
    pub(super) fn new(page: &Page, opened: &Opened) -> Self {
        let points = &page.points;
        let index: HashMap<PointId, usize> = points
            .iter()
            .enumerate()
            .map(|(at, point)| (point.id, at))
            .collect();
        let line_of = |id: &PointId| index.get(id).map(|at| points[*at].line);
        let mut of_line: HashMap<PointId, Vec<usize>> = HashMap::new();
        for (at, point) in points.iter().enumerate() {
            of_line.entry(point.line).or_default().push(at);
        }
        let known: HashMap<PointId, (usize, &Line)> = (page.lines.iter().enumerate())
            .map(|(at, line)| (line.start, (at, line)))
            .collect();
        // The line a line splits off.
        let parent_of = |start: &PointId| {
            let start = &points[*index.get(start)?];
            line_of(start.parents.first()?)
        };

        let mut drawn: HashSet<PointId> = (of_line.keys())
            .filter(|start| match known.get(start) {
                Some((_, line)) => {
                    opened.lines.contains(start)
                        || (line.is_version() && !page.folded_away.contains(start))
                }
                None => true,
            })
            .copied()
            .collect();
        drawn.extend(line_of(&page.current));
        let mut unseen: Vec<PointId> = drawn.iter().copied().collect();
        while let Some(start) = unseen.pop() {
            if let Some(parent) = parent_of(&start)
                && drawn.insert(parent)
            {
                unseen.push(parent);
            }
        }

        let splits: HashSet<PointId> = (drawn.iter())
            .filter_map(|start| points[*index.get(start)?].parents.first().copied())
            .collect();
        let matters = |at: usize, of: &[usize]| {
            let point = &points[at];
            Some(&at) == of.first()
                || Some(&at) == of.last()
                || point.id == point.line
                || point.id == page.current
                || point.parents.len() != 1
                || splits.contains(&point.id)
        };
        // Rows, and the folds between them: (first point, count, open, line, row point above,
        // row point below).
        let mut is_row = vec![false; points.len()];
        let mut runs = Vec::new();
        for start in &drawn {
            let of = &of_line[start];
            let mut above = None;
            let mut run: Vec<usize> = Vec::new();
            for &at in of {
                if !matters(at, of) {
                    run.push(at);
                    continue;
                }
                is_row[at] = true;
                if let [only] = run[..] {
                    is_row[only] = true;
                } else if let (Some(&first), Some(above)) = (run.first(), above) {
                    let open = opened.folds.contains(&points[first].id);
                    if open {
                        run.iter().for_each(|at| is_row[*at] = true);
                    }
                    runs.push((first, run.len(), open, *start, above, at));
                }
                above = Some(at);
                run.clear();
            }
        }
        let closed_below: HashSet<usize> = (runs.iter())
            .filter(|run| !run.2)
            .map(|run| run.4)
            .collect();

        let row_points: Vec<usize> = (0..points.len()).filter(|at| is_row[*at]).collect();
        let row_of: HashMap<PointId, usize> = (row_points.iter().enumerate())
            .map(|(row, at)| (points[*at].id, row))
            .collect();
        let row_of_point = |id: &PointId| row_of.get(id).copied();
        let color = |start: &PointId| known.get(start).map_or(0, |(at, _)| *at);

        // Top to bottom, each line takes the first lane free at its first row, and keeps it
        // down to the row of the line it splits off.
        let mut lane_of: HashMap<PointId, usize> = HashMap::new();
        let mut busy_until: Vec<usize> = Vec::new();
        for (row, at) in row_points.iter().enumerate() {
            let start = points[*at].line;
            if lane_of.contains_key(&start) {
                continue;
            }
            let of = &of_line[&start];
            let oldest = &points[of[of.len() - 1]];
            let last = (of.iter().rev()).find_map(|at| row_of_point(&points[*at].id));
            let split = oldest.parents.first().and_then(row_of_point);
            let bottom = last.max(split).unwrap_or(row);
            let lane = match busy_until.iter().position(|until| *until < row) {
                Some(free) => free,
                None => {
                    busy_until.push(0);
                    busy_until.len() - 1
                }
            };
            busy_until[lane] = bottom;
            lane_of.insert(start, lane);
        }

        let mut rows: Vec<Row> = (row_points.iter())
            .map(|&at| {
                let point = &points[at];
                let tip = of_line[&point.line].first() == Some(&at);
                let name = match known.get(&point.line) {
                    _ if point.parents.is_empty() => Name::ProjectStart,
                    Some((_, line)) if tip => Name::Line(line.title()),
                    _ => Name::None,
                };
                Row {
                    point: point.id,
                    line: point.line,
                    lane: lane_of[&point.line],
                    color: color(&point.line),
                    tip,
                    current: point.id == page.current,
                    name,
                    time: point.time,
                    hidden_versions: Vec::new(),
                    undone: Vec::new(),
                    undone_points: 0,
                    opened: Vec::new(),
                }
            })
            .collect();
        for (start, of) in &of_line {
            let oldest = &points[of[of.len() - 1]];
            if oldest.id != *start {
                continue;
            }
            let Some(row) = oldest.parents.first().and_then(row_of_point) else {
                continue;
            };
            let row = &mut rows[row];
            match known.get(start) {
                _ if drawn.contains(start) => {
                    if opened.lines.contains(start) {
                        row.opened.push(*start);
                    }
                }
                Some((_, line)) if line.is_version() => row.hidden_versions.push(*start),
                Some(_) => {
                    row.undone.push(*start);
                    row.undone_points += of.len();
                }
                None => {}
            }
        }

        let mut edges = Vec::new();
        for start in &drawn {
            let (lane, color) = (lane_of[start], color(start));
            let of = &of_line[start];
            let shown: Vec<usize> = of.iter().copied().filter(|at| is_row[*at]).collect();
            for pair in shown.windows(2) {
                let folded = closed_below.contains(&pair[0]);
                edges.push(Edge {
                    from: (row_of[&points[pair[0]].id], lane),
                    to: (row_of[&points[pair[1]].id], lane),
                    color,
                    stroke: if folded {
                        Stroke::Dotted
                    } else {
                        Stroke::Solid
                    },
                });
            }
            let oldest = &points[of[of.len() - 1]];
            if let Some(parent) = oldest.parents.first()
                && let (Some(to), Some(parent_line)) = (row_of_point(parent), line_of(parent))
            {
                edges.push(Edge {
                    from: (row_of[&oldest.id], lane),
                    to: (to, lane_of[&parent_line]),
                    color,
                    stroke: Stroke::Solid,
                });
            }
        }
        for (row, &at) in row_points.iter().enumerate() {
            for merged in points[at].parents.iter().skip(1) {
                if let (Some(to), Some(line)) = (row_of_point(merged), line_of(merged)) {
                    edges.push(Edge {
                        from: (row, rows[row].lane),
                        to: (to, lane_of[&line]),
                        color: color(&line),
                        stroke: Stroke::Dashed,
                    });
                }
            }
        }

        let mut folds: Vec<Fold> = (runs.into_iter())
            .map(|(first, count, open, start, above, below)| Fold {
                first: points[first].id,
                lane: lane_of[&start],
                rows: (row_of[&points[above].id], row_of[&points[below].id]),
                count,
                open,
            })
            .collect();
        folds.sort_by_key(|fold| fold.rows);
        Layout {
            rows,
            lanes: busy_until.len(),
            edges,
            folds,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    fn id(number: usize) -> PointId {
        format!("{number:040x}").parse().unwrap()
    }

    /// A history built oldest first: each point is numbered in the order it was made, which is
    /// also its time.
    struct Built {
        points: Vec<Point>,
        lines: Vec<Line>,
        opened: Opened,
        folded_away: HashSet<PointId>,
        current: Option<usize>,
    }

    impl Built {
        /// The project start and `more` points after it, in version 1.
        fn new(more: usize) -> Self {
            let mut built = Self {
                points: Vec::new(),
                lines: Vec::new(),
                opened: Opened::default(),
                folded_away: HashSet::new(),
                current: None,
            };
            built.push(None, 0);
            built.lines.push(line(0, Some(1)));
            built.grow(0, more);
            built
        }

        fn push(&mut self, parent: Option<usize>, line: usize) -> usize {
            let number = self.points.len();
            self.points.push(Point {
                id: id(number),
                parents: parent.into_iter().map(id).collect(),
                time: UNIX_EPOCH + Duration::from_secs(number as u64),
                label: String::new(),
                line: id(line),
            });
            if let Some(line) = self.lines.iter_mut().find(|each| each.start == id(line)) {
                line.tip = id(number);
            }
            number
        }

        /// `count` points more at the end of the line that starts at `start`.
        fn grow(&mut self, start: usize, count: usize) {
            for _ in 0..count {
                let parent = (self.points.iter().rev())
                    .find(|point| point.line == id(start))
                    .map(|point| point.id);
                let parent = self
                    .points
                    .iter()
                    .position(|point| Some(point.id) == parent);
                self.push(parent, start);
            }
        }

        /// A line of `count` points that splits off `from`. Returns its start.
        fn split(&mut self, from: usize, count: usize, number: Option<u32>) -> usize {
            let start = self.points.len();
            self.lines.push(line(start, number));
            self.push(Some(from), start);
            self.grow(start, count - 1);
            start
        }

        fn layout(&self) -> Layout {
            let page = Page {
                points: self.points.iter().rev().cloned().collect(),
                lines: self.lines.clone(),
                current: id(self.current.unwrap_or(self.points.len() - 1)),
                folded_away: self.folded_away.clone(),
            };
            Layout::new(&page, &self.opened)
        }
    }

    fn line(start: usize, number: Option<u32>) -> Line {
        Line {
            start: id(start),
            tip: id(start),
            number,
            name: None,
            named_by_hand: false,
            named_at: None,
            set_aside: false,
        }
    }

    fn rows(layout: &Layout) -> Vec<PointId> {
        layout.rows.iter().map(|row| row.point).collect()
    }

    fn row(layout: &Layout, point: usize) -> &Row {
        layout
            .rows
            .iter()
            .find(|row| row.point == id(point))
            .unwrap()
    }

    fn folds(layout: &Layout) -> Vec<(PointId, usize)> {
        let folds = layout.folds.iter().filter(|fold| !fold.open);
        folds.map(|fold| (fold.first, fold.count)).collect()
    }

    #[test]
    fn a_version_is_its_latest_point_and_its_start_with_a_fold_between() {
        let built = Built::new(9);
        let layout = built.layout();
        assert_eq!(rows(&layout), [id(9), id(0)]);
        assert_eq!(folds(&layout), [(id(8), 8)]);
        assert_eq!(layout.edges[0].stroke, Stroke::Dotted);
        assert_eq!(row(&layout, 9).name, Name::Line("Version 1".into()));
        assert_eq!(row(&layout, 0).name, Name::ProjectStart);
    }

    #[test]
    fn the_current_point_is_a_row_and_a_fold_of_one_point_is_a_row() {
        let mut built = Built::new(9);
        built.current = Some(7);
        let layout = built.layout();
        assert_eq!(rows(&layout), [id(9), id(8), id(7), id(0)]);
        assert_eq!(folds(&layout), [(id(6), 6)]);
        assert!(row(&layout, 7).current);
    }

    #[test]
    fn an_opened_fold_shows_its_points_and_its_line_is_solid() {
        let mut built = Built::new(9);
        built.opened.folds.insert(id(8));
        let layout = built.layout();
        assert_eq!(layout.rows.len(), 10);
        assert!(layout.folds[0].open);
        assert_eq!((layout.folds[0].rows), (0, 9));
        assert!(layout.edges.iter().all(|edge| edge.stroke == Stroke::Solid));
    }

    #[test]
    fn a_fold_stops_where_a_drawn_version_splits_off() {
        let mut built = Built::new(20);
        let start = built.split(10, 2, Some(2));
        built.current = Some(20);
        let layout = built.layout();
        assert_eq!(
            rows(&layout),
            [id(start + 1), id(start), id(20), id(10), id(0)]
        );
        assert_eq!(folds(&layout), [(id(19), 9), (id(9), 9)]);
        let branch = layout.edges.iter().find(|edge| edge.from.0 == 1).unwrap();
        assert_eq!((branch.to.0, branch.stroke), (3, Stroke::Solid));
    }

    #[test]
    fn undone_work_and_folded_away_versions_do_not_split_folds() {
        let mut built = Built::new(20);
        let undone = built.split(10, 2, None);
        let folded = built.split(5, 3, Some(2));
        built.folded_away.insert(id(folded));
        built.current = Some(20);
        let layout = built.layout();
        assert_eq!(rows(&layout), [id(20), id(0)]);
        assert_eq!(folds(&layout), [(id(19), 19)]);

        // Opened, the fold shows where they leave.
        built.opened.folds.insert(id(19));
        let layout = built.layout();
        assert_eq!(row(&layout, 10).undone, [id(undone)]);
        assert_eq!(row(&layout, 10).undone_points, 2);
        assert_eq!(row(&layout, 5).hidden_versions, [id(folded)]);
    }

    #[test]
    fn a_folded_away_version_shown_by_hand_is_drawn_and_splits_the_fold() {
        let mut built = Built::new(20);
        let folded = built.split(5, 3, Some(2));
        built.folded_away.insert(id(folded));
        built.opened.lines.insert(id(folded));
        built.current = Some(20);
        let layout = built.layout();
        assert!(rows(&layout).contains(&id(folded + 2)));
        assert_eq!(row(&layout, 5).opened, [id(folded)]);
        assert!(row(&layout, 5).hidden_versions.is_empty());
    }

    #[test]
    fn a_merge_is_a_row_with_a_dashed_stroke_to_the_version_it_merged() {
        let mut built = Built::new(4);
        let other = built.split(2, 2, Some(2));
        let tip = built.push(Some(4), 0);
        built.points[tip].parents.push(id(other + 1));
        let layout = built.layout();
        let merge = layout
            .rows
            .iter()
            .position(|row| row.point == id(tip))
            .unwrap();
        let dashed = layout
            .edges
            .iter()
            .find(|edge| edge.stroke == Stroke::Dashed)
            .unwrap();
        assert_eq!(dashed.from.0, merge);
        assert_eq!(layout.rows[dashed.to.0].point, id(other + 1));
    }

    #[test]
    fn a_lane_is_free_again_below_the_split_of_the_line_that_held_it() {
        let mut built = Built::new(10);
        let low = built.split(3, 2, Some(2));
        built.grow(0, 8);
        let high = built.split(15, 2, Some(3));
        built.grow(0, 3);
        let layout = built.layout();
        assert_eq!(layout.lanes, 2);
        assert_eq!(row(&layout, high).lane, 1);
        assert_eq!(row(&layout, low).lane, 1);
    }
}
