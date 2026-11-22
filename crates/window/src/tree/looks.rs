//! Mistakes in how a view's boxes sit in their frames, which `--tree` lists in `warnings` for
//! whoever writes the view: a background flush with one edge of the box that frames it but a
//! little in from the two edges beside that one, and a background over a border around it.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

use serde_json::Value;
use sound_core::InstanceId;

use crate::app::State;

use super::build::{holds_plane, is_overlay};
use super::{BoxNode, Node, Opens, Scope, Scroll};

/// Room that reads as a mistake beside an edge it touches: more is a layout of its own.
const ROOM: std::ops::RangeInclusive<f32> = 1.0..=12.0;
/// Smaller backgrounds are dots, lines and handles.
const SMALLEST: f32 = 16.0;

/// What the views shown paint and the laid-out elements do not say, by where each box is.
pub(crate) static SHOWN: Mutex<Option<Paints>> = Mutex::new(None);

#[derive(Default)]
pub(crate) struct Paints(Vec<(Place, Paint)>);

/// Where a box is among the laid-out elements: from the element keyed `key`, up to parents
/// and down to children by index, as Rune names an element without a key.
struct Place {
    key: String,
    steps: Vec<Step>,
}

#[derive(Clone, Copy)]
enum Step {
    Up,
    Down(usize),
}

impl Place {
    fn key(key: String) -> Self {
        Self {
            key,
            steps: Vec::new(),
        }
    }

    fn down(&self, index: usize) -> Self {
        let mut steps = self.steps.clone();
        steps.push(Step::Down(index));
        Self {
            key: self.key.clone(),
            steps,
        }
    }
}

pub(crate) struct Paint {
    /// `#rrggbbaa`, as the elements give their fill.
    hover: Option<String>,
    /// Its colour, and its width on each side: top, right, bottom, left.
    border: Option<(String, [f32; 4])>,
    radius: f32,
    /// What it holds moves under its edges along these axes: x, y.
    scrolls: [bool; 2],
    clips: bool,
    /// Where it sits comes from the song, not the layout: `place` or `at`.
    placed: bool,
    plane: bool,
}

/// The boxes of the views shown, with what they paint.
pub(crate) fn paints(state: &State) -> Paints {
    let mut walk = Walk {
        state,
        paints: Paints::default(),
        walked: HashSet::new(),
    };
    for id in state.main().into_iter().chain(&state.shown) {
        if walk.walked.insert(id.clone())
            && let Some(root) = state.root(id)
        {
            walk.node(id, &state.prefix_of(id), &root, None, false);
        }
    }
    walk.paints
}

struct Walk<'a> {
    state: &'a State,
    paints: Paints,
    walked: HashSet<InstanceId>,
}

impl Walk<'_> {
    /// `node` and what it holds, at `place` when its parent knows it. A plane gathers what it
    /// holds in an order of its own, so inside one only a key says where a box is.
    fn node(
        &mut self,
        id: &InstanceId,
        prefix: &str,
        node: &Node,
        place: Option<Place>,
        in_plane: bool,
    ) {
        match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
                let place = match &b.key {
                    Some(key) => Some(Place::key(format!("{prefix}{key}"))),
                    None if in_plane => None,
                    None => place.or_else(|| self.anchor(id, prefix, node)),
                };
                self.boxed(id, prefix, b, place, in_plane || b.plane.is_some());
            }
            Node::Memo { key } => {
                let Some((_, inner)) = self.state.memo_of(id, key) else {
                    return;
                };
                // Rune keys a memo's root with the memo's key, unless it holds a plane.
                let place = match holds_plane(&Scope::new(self.state, id, prefix), &inner) {
                    true => place,
                    false => Some(Place::key(format!(
                        "{prefix}{}",
                        inner.key().unwrap_or(key)
                    ))),
                };
                self.node(id, prefix, &inner, place, in_plane);
            }
            Node::Child { path } => {
                let Some(child) = self.child(id, path) else {
                    return;
                };
                if let Some(root) = self.state.root(&child) {
                    self.walked.insert(child.clone());
                    self.node(
                        &child,
                        &self.state.prefix_of(&child),
                        &root,
                        place,
                        in_plane,
                    );
                }
            }
            Node::Text(_) | Node::Shapes(_) | Node::Field(_) | Node::Icon(_) => {}
        }
    }

    fn boxed(
        &mut self,
        id: &InstanceId,
        prefix: &str,
        b: &BoxNode,
        place: Option<Place>,
        in_plane: bool,
    ) {
        let theme = &self.state.theme;
        let border = (b.border.as_deref()).map(|colour| {
            let sides = b.border_sides.unwrap_or([1.0; 4]);
            (theme.color(colour).hex(), sides)
        });
        let paint = Paint {
            hover: (b.hover.as_deref()).map(|colour| theme.color(colour).hex()),
            border,
            radius: (b.radius.as_deref())
                .and_then(|name| theme.radius(name))
                .unwrap_or(0.0),
            scrolls: match b.scroll {
                Some(Scroll::X) => [true, false],
                Some(Scroll::Y) => [false, true],
                Some(Scroll::Both) => [true; 2],
                None => [false; 2],
            },
            clips: b.clip,
            placed: b.place.is_some() || b.at.is_some(),
            plane: b.plane.is_some(),
        };
        let opener = b.key.as_ref().map(|key| format!("{prefix}{key}"));
        let mut index = 0;
        for child in &b.children {
            let inside = match child {
                Node::Row(o) | Node::Col(o) | Node::Plain(o) if is_overlay(child) => {
                    let Some(opener) = &opener else {
                        continue;
                    };
                    // What a search hides is not built, so the indexes inside are not known.
                    if o.picks() && self.state.search_of(opener).is_some() {
                        continue;
                    }
                    match o.overlay.map(|overlay| overlay.open) {
                        Some(Opens::Hover) => Some(Place::key(format!("{opener}-tip")).down(0)),
                        _ => None,
                    }
                }
                _ => {
                    index += 1;
                    (place.as_ref())
                        .filter(|_| !in_plane)
                        .map(|place| place.down(index - 1))
                }
            };
            self.node(id, prefix, child, inside, in_plane);
        }
        if let Some(place) = place {
            self.paints.0.push((place, paint));
        }
    }

    /// Where a box no parent placed is: up from the first key inside it.
    fn anchor(&self, id: &InstanceId, prefix: &str, node: &Node) -> Option<Place> {
        let (key, depth) = self.first_key(id, prefix, node)?;
        Some(Place {
            key,
            steps: vec![Step::Up; depth],
        })
    }

    /// The first key in `node` and how deep it is.
    fn first_key(&self, id: &InstanceId, prefix: &str, node: &Node) -> Option<(String, usize)> {
        match node {
            Node::Row(b) | Node::Col(b) | Node::Plain(b) => {
                if let Some(key) = &b.key {
                    return Some((format!("{prefix}{key}"), 0));
                }
                if b.plane.is_some() {
                    return None;
                }
                let mut built = b.children.iter().filter(|child| !is_overlay(child));
                let (key, depth) = built.find_map(|child| self.first_key(id, prefix, child))?;
                Some((key, depth + 1))
            }
            Node::Memo { key } => {
                let (_, inner) = self.state.memo_of(id, key)?;
                match holds_plane(&Scope::new(self.state, id, prefix), &inner) {
                    true => self.first_key(id, prefix, &inner),
                    false => Some((format!("{prefix}{}", inner.key().unwrap_or(key)), 0)),
                }
            }
            Node::Child { path } => {
                let child = self.child(id, path)?;
                let root = self.state.root(&child)?;
                self.first_key(&child, &self.state.prefix_of(&child), &root)
            }
            Node::Text(_) | Node::Shapes(_) | Node::Field(_) | Node::Icon(_) => None,
        }
    }

    fn child(&self, id: &InstanceId, path: &str) -> Option<InstanceId> {
        let child = id.child(path).ok()?;
        (!self.state.code_failed(&child)).then_some(child)
    }
}

/// An element as Rune laid it out, in the order of the tree.
pub(crate) struct Seen {
    pub(crate) path: String,
    pub(crate) key: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) rect: [f32; 4],
    /// `#rrggbbaa`.
    pub(crate) fill: String,
    /// Still moving or leaving: where it is now says nothing of the layout.
    pub(crate) moving: bool,
}

impl Seen {
    /// An element of the tree `--tree` writes.
    fn of_tree(element: &Value) -> Option<Self> {
        let flag = |name: &str| element.get(name).and_then(Value::as_bool) == Some(true);
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
            moving: flag("moving") || flag("exiting"),
        })
    }

    fn name(&self) -> String {
        match (&self.key, &self.label) {
            (Some(key), _) => format!("`{key}`"),
            (None, Some(label)) => format!("\"{label}\""),
            (None, None) => format!("The box at `{}`", self.path),
        }
    }

    fn named(&self) -> String {
        match (&self.key, &self.label) {
            (None, None) => format!("the box at `{}`", self.path),
            _ => self.name(),
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

/// The elements as laid out, with the paint of each box of the views.
struct Laid<'a> {
    seen: &'a [Seen],
    parents: Vec<Option<usize>>,
    painted: Vec<Option<&'a Paint>>,
}

impl<'a> Laid<'a> {
    fn new(seen: &'a [Seen], paints: &'a Paints) -> Self {
        let parents = parents(seen);
        // Only through elements that sit still: one that leaves may have lost its parent.
        let settled = || {
            seen.iter()
                .enumerate()
                .filter(|(_, element)| !element.moving)
        };
        let mut by_key = HashMap::new();
        let mut by_path = HashMap::new();
        for (i, element) in settled() {
            if let Some(key) = &element.key {
                by_key.entry(key.as_str()).or_insert(i);
            }
            by_path.entry(element.path.as_str()).or_insert(i);
        }
        let up = |at: usize| {
            let parent = parents[at]?;
            let rest = seen[at].path.strip_prefix(&seen[parent].path)?;
            let segment = rest.strip_prefix('/')?;
            let one = match &seen[at].key {
                Some(key) => segment == key,
                None => segment.starts_with('#') && !segment.contains('/'),
            };
            (one && !seen[parent].moving).then_some(parent)
        };
        let mut painted = vec![None; seen.len()];
        for (place, paint) in &paints.0 {
            let Some(&from) = by_key.get(place.key.as_str()) else {
                continue;
            };
            let found = place.steps.iter().try_fold(from, |at, step| match step {
                Step::Up => up(at),
                Step::Down(i) => by_path
                    .get(format!("{}/#{i}", seen[at].path).as_str())
                    .copied(),
            });
            if let Some(at) = found {
                painted[at].get_or_insert(paint);
            }
        }
        Self {
            seen,
            parents,
            painted,
        }
    }

    fn up(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.parents[i], |&at| self.parents[at])
    }

    /// Whether `i` is a box of a view or inside one, not the app's own.
    fn in_view(&self, i: usize) -> bool {
        std::iter::once(i)
            .chain(self.up(i))
            .any(|at| self.painted[at].is_some() || self.seen[at].key.as_deref() == Some("main"))
    }

    /// Its background as it shows now, else as hover shows it, and whether that is hover's.
    fn background(&self, i: usize) -> Option<(&str, bool)> {
        let hover = self.painted[i].and_then(|paint| paint.hover.as_deref());
        match hover {
            _ if shows(&self.seen[i].fill) => Some((self.seen[i].fill.as_str(), false)),
            Some(hover) if shows(hover) => Some((hover, true)),
            _ => None,
        }
    }
}

pub(crate) fn mistakes(seen: &[Seen], paints: &Paints) -> Vec<String> {
    let laid = Laid::new(seen, paints);
    let mut found = flush(&laid);
    found.extend(covered(&laid));
    found
}

/// A line for each background that touches one edge of its frame and stands a little in from
/// both edges beside it.
fn flush(laid: &Laid) -> Vec<String> {
    let (seen, painted) = (laid.seen, &laid.painted);
    let paint = |i: usize| painted[i];
    let framed = |i: usize| {
        shows(&seen[i].fill)
            || paint(i)
                .and_then(|paint| paint.border.as_ref())
                .is_some_and(|(_, sides)| sides.iter().all(|&width| width > 0.0))
    };
    let moved = |i: usize| paint(i).is_some_and(|paint| paint.placed || paint.plane);
    let mut found = Vec::new();
    for (i, element) in seen.iter().enumerate() {
        let Some((background, hovered)) = laid.background(i) else {
            continue;
        };
        let [x, y, w, h] = element.rect;
        if w < SMALLEST || h < SMALLEST || paint(i).is_some_and(|paint| paint.placed) {
            continue;
        }
        // Inside a plane or a placed box, where it sits is the song's, not the layout's.
        let Some(frame) = laid
            .up(i)
            .take_while(|&at| !moved(at) || framed(at))
            .find(|&at| framed(at))
        else {
            continue;
        };
        let plane = paint(frame).is_some_and(|paint| paint.plane);
        if element.moving || plane || !laid.in_view(frame) || seen[frame].fill == background {
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

/// A line for each side or rounded corner of a border that a background inside it lies over:
/// Rune draws a border under what its box holds.
fn covered(laid: &Laid) -> Vec<String> {
    let seen = laid.seen;
    let mut named = HashSet::new();
    let mut found = Vec::new();
    for (i, element) in seen.iter().enumerate() {
        let Some((background, hovered)) = laid.background(i) else {
            continue;
        };
        let own = laid.painted[i];
        // Placed by the song, such as a light that fills a pad while it sounds.
        if element.moving || !laid.in_view(i) || own.is_some_and(|own| own.placed) {
            continue;
        }
        let mut rect = element.rect;
        let mut scrolls = [false; 2];
        for frame in laid.up(i) {
            let Some(paint) = laid.painted[frame] else {
                break;
            };
            scrolls = [0, 1].map(|axis| scrolls[axis] || paint.scrolls[axis]);
            if let Some((colour, widths)) = &paint.border
                && colour != background
            {
                let [fx, fy, fw, fh] = seen[frame].rect;
                let [top, right, bottom, left] = *widths;
                let strips = [
                    [fx, fy, fw, top],
                    [fx + fw - right, fy, right, fh],
                    [fx, fy + fh - bottom, fw, bottom],
                    [fx, fy, left, fh],
                ];
                let under_own = |side: usize| {
                    (own.and_then(|own| own.border.as_ref()))
                        .is_some_and(|(own, sides)| own == colour && sides[side] >= widths[side])
                };
                for (side, strip) in strips.into_iter().enumerate() {
                    let [_, _, w, h] = meet(rect, strip);
                    let (along, across) = if side % 2 == 0 { (w, h) } else { (h, w) };
                    if widths[side] <= 0.0
                        || scrolls[(side + 1) % 2]
                        || across < 0.25
                        || along < 1.0
                        || under_own(side)
                        || !named.insert((frame, side))
                    {
                        continue;
                    }
                    let long = if side % 2 == 0 { fw } else { fh };
                    let how = match along >= long - 1.0 {
                        true => "all along it".to_string(),
                        false => format!("along {} of its {} points", points(along), points(long)),
                    };
                    found.push(format!(
                        "{}: its {}background lies over the {} line of {}, {how}. A box draws its line under what it holds: keep what it holds off the line, or draw the line over it.",
                        element.name(),
                        if hovered { "hover " } else { "" },
                        EDGES[side],
                        seen[frame].named(),
                    ));
                }
                let corners: Vec<&str> = poking(rect, own, seen[frame].rect, paint)
                    .filter(|&corner| named.insert((frame, 4 + corner)))
                    .map(|corner| CORNERS[corner])
                    .collect();
                if !corners.is_empty() {
                    found.push(format!(
                        "{}: its {}background pokes over the rounded {} of the line of {}. Give it the same rounding, or room inside the corners.",
                        element.name(),
                        if hovered { "hover " } else { "" },
                        match corners.as_slice() {
                            [one] => format!("{one} corner"),
                            more => format!("{} corners", more.join(", ")),
                        },
                        seen[frame].named(),
                    ));
                }
            }
            if paint.clips || paint.scrolls.contains(&true) {
                rect = meet(rect, seen[frame].rect);
            }
            // Inside a plane or a placed box, where it sits is the song's, not the layout's.
            if paint.placed || paint.plane {
                break;
            }
        }
    }
    found
}

const EDGES: [&str; 4] = ["top", "right", "bottom", "left"];
const CORNERS: [&str; 4] = ["top left", "top right", "bottom right", "bottom left"];

/// The rounded corners of the line of `frame` that a background at `rect` reaches past.
fn poking(
    rect: [f32; 4],
    own: Option<&Paint>,
    frame: [f32; 4],
    paint: &Paint,
) -> impl Iterator<Item = usize> {
    let rounding = own.map(|own| own.radius);
    let widths = paint.border.as_ref().map(|(_, widths)| *widths);
    let [fx, fy, fw, fh] = frame;
    let radius = paint.radius.min(fw / 2.0).min(fh / 2.0);
    let [x, y, w, h] = rect;
    (0..4).filter(move |&corner| {
        let (Some(rounding), Some(widths)) = (rounding, widths) else {
            return false;
        };
        let rounding = rounding.min(w / 2.0).min(h / 2.0);
        let width = widths[corner].min(widths[(corner + 3) % 4]);
        if radius < 1.0 || width <= 0.0 {
            return false;
        }
        // Flipped so the corner is the top left one.
        let flip = |at: [f32; 2], from: [f32; 2], size: [f32; 2]| match corner {
            0 => [at[0] - from[0], at[1] - from[1]],
            1 => [from[0] + size[0] - at[0], at[1] - from[1]],
            2 => [from[0] + size[0] - at[0], from[1] + size[1] - at[1]],
            _ => [at[0] - from[0], from[1] + size[1] - at[1]],
        };
        let near = |at: [f32; 2]| flip(at, [fx, fy], [fw, fh]);
        let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        let [cx, cy] = near(corners[corner]);
        // The centre of its own rounded corner, then the farthest point of that from the frame's.
        let centre = [cx + rounding, cy + rounding];
        if centre[0] >= radius || centre[1] >= radius {
            return false;
        }
        let far = (centre[0] - radius).hypot(centre[1] - radius) + rounding;
        far > radius - width + 0.5
    })
}

/// Where two rects `[x, y, w, h]` meet; no size when they do not.
fn meet(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (x, y) = (a[0].max(b[0]), a[1].max(b[1]));
    let right = (a[0] + a[2]).min(b[0] + b[2]);
    let bottom = (a[1] + a[3]).min(b[1] + b[3]);
    [x, y, (right - x).max(0.0), (bottom - y).max(0.0)]
}

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
