//! Mistakes in how a view's boxes sit in their frames, which `--tree` lists in `warnings` for
//! whoever writes the view: a background flush with one edge of the box that frames it, but a
//! little in from the two edges beside that one.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use serde_json::Value;

use crate::app::State;

use super::Node;

/// Room that reads as a mistake beside an edge it touches: more is a layout of its own.
const ROOM: std::ops::RangeInclusive<f32> = 1.0..=12.0;
/// Smaller backgrounds are dots, lines and handles.
const SMALLEST: f32 = 16.0;

/// What the views shown paint and the laid-out elements do not say, by the key of each box.
pub(crate) static SHOWN: Mutex<Option<Paints>> = Mutex::new(None);

#[derive(Default)]
pub(crate) struct Paints(HashMap<String, Paint>);

struct Paint {
    /// `#rrggbbaa`, as the elements give their fill.
    hover: Option<String>,
    /// A border all round, which frames what it holds as a fill does.
    border: bool,
    /// Where it sits comes from the song, not the layout: `place` or `at`.
    placed: bool,
    plane: bool,
}

/// The keyed boxes of the views shown, with what they paint.
pub(crate) fn paints(state: &State) -> Paints {
    fn walk(
        state: &State,
        id: &sound_core::InstanceId,
        prefix: &str,
        node: &Node,
        into: &mut Paints,
    ) {
        match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
                if let Some(key) = &b.key {
                    let paint = Paint {
                        hover: b
                            .hover
                            .as_deref()
                            .map(|colour| state.theme.color(colour).hex()),
                        border: b.border.is_some() && b.border_sides.is_none(),
                        placed: b.place.is_some() || b.at.is_some(),
                        plane: b.plane.is_some(),
                    };
                    into.0.insert(format!("{prefix}{key}"), paint);
                }
                for child in &b.children {
                    walk(state, id, prefix, child, into);
                }
            }
            Node::Memo { key } => {
                if let Some((_, inner)) = state.memo_of(id, key) {
                    walk(state, id, prefix, &inner, into);
                }
            }
            Node::Text(_)
            | Node::Shapes(_)
            | Node::Field(_)
            | Node::Icon(_)
            | Node::Child { .. } => {}
        }
    }
    let mut paints = Paints::default();
    for id in &state.shown {
        if let Some(root) = state.root(id) {
            walk(state, id, &state.prefix_of(id), &root, &mut paints);
        }
    }
    paints
}

/// An element as Rune laid it out, in the order of the tree.
pub(crate) struct Seen {
    pub(crate) path: String,
    pub(crate) key: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) rect: [f32; 4],
    /// `#rrggbbaa`.
    pub(crate) fill: String,
}

impl Seen {
    /// An element of the tree `--tree` writes, unless it is still moving or leaving.
    fn of_tree(element: &Value) -> Option<Self> {
        let flag = |name: &str| element.get(name).and_then(Value::as_bool) == Some(true);
        if flag("moving") || flag("exiting") {
            return None;
        }
        let text = |name: &str| {
            element
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let rect: Vec<f32> = serde_json::from_value(element.get("rect")?.clone()).ok()?;
        Some(Self {
            path: text("path")?,
            key: text("key"),
            label: text("label"),
            rect: rect.try_into().ok()?,
            fill: text("fill")?,
        })
    }

    fn name(&self) -> String {
        match (&self.key, &self.label) {
            (Some(key), _) => format!("`{key}`"),
            (None, Some(label)) => format!("\"{label}\""),
            (None, None) => format!("The box at `{}`", self.path),
        }
    }
}

/// Adds the mistakes of the views shown to `warnings` of the tree `--tree` wrote.
pub(crate) fn warn_in(tree: &mut Value) {
    let Some(paints) = SHOWN.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return;
    };
    let seen: Vec<Seen> = (tree.get("elements").and_then(Value::as_array))
        .map(|elements| elements.iter().filter_map(Seen::of_tree).collect())
        .unwrap_or_default();
    let found = mistakes(&seen, &paints);
    if let Some(Value::Array(warnings)) = tree.get_mut("warnings") {
        warnings.extend(found.into_iter().map(Value::String));
    }
}

/// A line for each background that touches one edge of its frame and stands a little in from
/// both edges beside it.
pub(crate) fn mistakes(seen: &[Seen], paints: &Paints) -> Vec<String> {
    let parents = parents(seen);
    let paint = |i: usize| seen[i].key.as_ref().and_then(|key| paints.0.get(key));
    let framed = |i: usize| shows(&seen[i].fill) || paint(i).is_some_and(|paint| paint.border);
    let moved = |i: usize| paint(i).is_some_and(|paint| paint.placed || paint.plane);
    let in_view = |i: usize| {
        std::iter::successors(Some(i), |&at| parents[at]).any(|at| {
            (seen[at].key.as_ref()).is_some_and(|key| key == "main" || paints.0.contains_key(key))
        })
    };
    let mut found = Vec::new();
    for (i, element) in seen.iter().enumerate() {
        let hover = paint(i).and_then(|paint| paint.hover.as_deref());
        let (background, hovered) = match hover {
            _ if shows(&element.fill) => (element.fill.as_str(), false),
            Some(hover) if shows(hover) => (hover, true),
            _ => continue,
        };
        let [x, y, w, h] = element.rect;
        if w < SMALLEST || h < SMALLEST || paint(i).is_some_and(|paint| paint.placed) {
            continue;
        }
        // Inside a plane or a placed box, where it sits is the song's, not the layout's.
        let up = std::iter::successors(parents[i], |&at| parents[at]);
        let Some(frame) = up
            .take_while(|&at| !moved(at) || framed(at))
            .find(|&at| framed(at))
        else {
            continue;
        };
        let plane = paint(frame).is_some_and(|paint| paint.plane);
        if plane || !in_view(frame) || seen[frame].fill == background {
            continue;
        }
        let [fx, fy, fw, fh] = seen[frame].rect;
        let gaps = [y - fy, fx + fw - x - w, fy + fh - y - h, x - fx];
        let flush = (0..4).find(|&side| {
            let beside = [gaps[(side + 3) % 4], gaps[(side + 1) % 4]];
            gaps[side].abs() < 0.5 && beside.iter().all(|gap| ROOM.contains(gap))
        });
        let Some(side) = flush else {
            continue;
        };
        let (before, after) = (gaps[(side + 3) % 4], gaps[(side + 1) % 4]);
        let (first, second) = (EDGES[(side + 3) % 4], EDGES[(side + 1) % 4]);
        let room = match (before - after).abs() < 0.5 {
            true => format!("{} points", points(before)),
            false => format!("{} and {} points", points(before), points(after)),
        };
        found.push(format!(
            "{}: its {}background touches the {} edge of the box with a fill or border around it, but stands {room} in from the {first} and {second} edges. Give it the same room on every side, or let it reach the {first} and {second} edges too.",
            element.name(),
            if hovered { "hover " } else { "" },
            EDGES[side],
        ));
    }
    found
}

const EDGES: [&str; 4] = ["top", "right", "bottom", "left"];

/// Whether a fill `#rrggbbaa` shows at all.
fn shows(fill: &str) -> bool {
    !fill.is_empty() && !fill.ends_with("00")
}

/// `4`, `4.5`.
fn points(gap: f32) -> f32 {
    (gap * 10.0).round() / 10.0
}

/// The parent of each element, from the paths of a tree in order. A key may hold a `/`, so a
/// parent is the nearest open element whose path starts the child's.
fn parents(seen: &[Seen]) -> Vec<Option<usize>> {
    let mut open: Vec<usize> = Vec::new();
    let mut parents = Vec::with_capacity(seen.len());
    for (i, element) in seen.iter().enumerate() {
        while let Some(&last) = open.last() {
            let rest = element.path.strip_prefix(seen[last].path.as_str());
            if rest.is_some_and(|rest| rest.starts_with('/')) {
                break;
            }
            open.pop();
        }
        parents.push(open.last().copied());
        open.push(i);
    }
    parents
}
