//! The shapes of a space, placed in its units. A frame works out only those that move and
//! those of the rest that show.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{Hsla, SharedString};
use sound_ui::LiveNotes;
use sound_ui::components::knob::{
    decibels_readout, hertz_readout, milliseconds_readout, pan_readout, percent_readout, short,
};

use crate::live::{DragNumbers, Sources, SpaceView};
use crate::tools::Unit;
use crate::tree::{
    self, Align, Cursor, Drag, List, NodeTable, Paint, PathPoint, Props, Reach, Readout, ShapeKind,
    Weight, Words,
};

use super::input::Hears;

/// What a shape of a space does with the pointer.
#[derive(Clone)]
pub(super) struct Pointer {
    pub(super) node: u64,
    pub(super) hears: Hears,
    pub(super) drag: Option<Rc<Drag>>,
    pub(super) cursor: Option<Cursor>,
    pub(super) anchor: (f64, f64),
}

/// A shape with its numbers worked out, in the units of its space.
#[derive(Clone)]
pub(super) struct Placed {
    /// Its place among the shapes of the space, which its pointer is found by.
    pub(super) index: usize,
    pub(super) geometry: Geometry,
    pub(super) color: Hsla,
    /// The box of the groups around it, outside which it does not show.
    pub(super) clip: Option<Area>,
}

/// A box in the units of a space: left, top, right and bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Area(pub(super) [f64; 4]);

impl Area {
    /// The part of it inside `other` as well, when there is one.
    fn within(self, other: Option<Area>) -> Area {
        let Some(Area([left, top, right, bottom])) = other else {
            return self;
        };
        let [l, t, r, b] = self.0;
        Area([l.max(left), t.max(top), r.min(right), b.min(bottom)])
    }

    fn meets(self, extent: &Extent) -> bool {
        let [left, top, right, bottom] = self.0;
        let (across, down) = (extent.across, extent.down);
        across.0.min(across.1) <= right
            && across.0.max(across.1) >= left
            && down.0.min(down.1) <= bottom
            && down.0.max(down.1) >= top
    }
}

#[derive(Clone)]
pub(super) enum Geometry {
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f32,
        border: Option<(Hsla, f32)>,
    },
    Circle {
        x: f64,
        y: f64,
        radius: f32,
    },
    Line {
        from: (f64, f64),
        to: (f64, f64),
        width: f32,
    },
    Arc {
        x: f64,
        y: f64,
        radius: f32,
        turns: (f32, f32),
        width: f32,
    },
    Text {
        x: f64,
        y: f64,
        text: SharedString,
        /// Its parts by their length, with their own colour and weight; none for one part.
        parts: Rc<[TextPart]>,
        size: f32,
        align: Align,
    },
    Wave {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        wave: Rc<Wave>,
    },
    Notes {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        notes: Rc<Notes>,
    },
    Path {
        bends: Rc<[Bend<(f64, f64)>]>,
        /// The width of its outline, else it is filled.
        outline: Option<f32>,
    },
}

/// A piece of a path: straight to a point, or curved to it pulled by one or two control
/// points, which come first.
#[derive(Clone, Copy)]
pub(super) enum Bend<P> {
    To(P),
    Quadratic(P, P),
    Cubic(P, P, P),
}

impl<P: Copy> Bend<P> {
    pub(super) fn map<Q>(self, at: impl Fn(P) -> Q) -> Bend<Q> {
        match self {
            Self::To(to) => Bend::To(at(to)),
            Self::Quadratic(control, to) => Bend::Quadratic(at(control), at(to)),
            Self::Cubic(first, second, to) => Bend::Cubic(at(first), at(second), at(to)),
        }
    }

    pub(super) fn end(self) -> P {
        match self {
            Self::To(to) | Self::Quadratic(_, to) | Self::Cubic(_, _, to) => to,
        }
    }

    /// Its points with its control points: a curve stays inside their box.
    pub(super) fn points(self) -> impl Iterator<Item = P> {
        match self {
            Self::To(to) => [Some(to), None, None],
            Self::Quadratic(control, to) => [Some(control), Some(to), None],
            Self::Cubic(first, second, to) => [Some(first), Some(second), Some(to)],
        }
        .into_iter()
        .flatten()
    }
}

impl Geometry {
    /// Where it starts: its top left, its centre, or where a line starts.
    fn origin(&self) -> (f64, f64) {
        match *self {
            Self::Rect { x, y, .. }
            | Self::Circle { x, y, .. }
            | Self::Arc { x, y, .. }
            | Self::Text { x, y, .. }
            | Self::Wave { x, y, .. }
            | Self::Notes { x, y, .. } => (x, y),
            Self::Line { from, .. } => from,
            Self::Path { ref bends, .. } => {
                bends.first().map(|bend| bend.end()).unwrap_or_default()
            }
        }
    }
}

/// The values a wave draws, oldest first, between `low` at its bottom and `high` at its top.
pub(super) struct Wave {
    pub(super) values: Vec<f32>,
    pub(super) low: f32,
    pub(super) high: f32,
    pub(super) band: bool,
    pub(super) line: f32,
    /// What the values are multiplied by along it, as `(fraction across, factor)` in order.
    level: Vec<(f32, f32)>,
}

impl Wave {
    /// What the values are multiplied by `fraction` across it: straight between the points of
    /// its level, and as the nearest one past its ends.
    pub(super) fn level_at(&self, fraction: f32) -> f32 {
        let (Some(first), Some(last)) = (self.level.first(), self.level.last()) else {
            return 1.0;
        };
        if fraction <= first.0 {
            return first.1;
        }
        for pair in self.level.windows(2) {
            let [(from, start), (to, end)] = [pair[0], pair[1]];
            if fraction <= to {
                let along = if to > from {
                    (fraction - from) / (to - from)
                } else {
                    1.0
                };
                return start + (end - start) * along;
            }
        }
        last.1
    }
}

/// The notes of a take as `Notes` draws them, as a clip draws its own: a line each, between the
/// lowest pitch and the highest, 12 apart at least.
pub(super) struct Notes {
    /// Where each starts and ends, as fractions of the way across, and its pitch above the lowest.
    pub(super) lines: Vec<(f32, f32, f32)>,
    /// How many pitches its height holds.
    pub(super) shown: f32,
    /// How far the lowest pitch's line is above its foot, in pitches.
    pub(super) under: f32,
}

impl Notes {
    const NONE: Self = Self {
        lines: Vec::new(),
        shown: 12.0,
        under: 0.0,
    };

    /// The notes of `take` across a box from where it starts to the playhead at `tick`.
    fn of(take: &LiveNotes, tick: f64) -> Self {
        let start = take.start.0 as f64;
        let span = (tick - start).max(1.0);
        let across = |at: f64| ((at - start) / span).clamp(0.0, 1.0) as f32;
        let pitches = take.notes.iter().map(|note| note.pitch);
        let (lowest, highest) = (
            pitches.clone().min().unwrap_or(0),
            pitches.max().unwrap_or(0),
        );
        let range = f32::from(highest - lowest) + 1.0;
        let shown = range.max(12.0);
        let lines = (take.notes.iter())
            .map(|note| {
                let end = note.end.map_or(tick, |end| end.0 as f64);
                (
                    across(note.start.0 as f64),
                    across(end),
                    f32::from(note.pitch - lowest),
                )
            })
            .collect();
        Self {
            lines,
            shown,
            under: (shown - range) / 2.0,
        }
    }
}

/// A part of a label's line, by its length in bytes.
#[derive(Clone, Debug)]
pub(super) struct TextPart {
    pub(super) len: usize,
    pub(super) color: Option<Hsla>,
    pub(super) weight: Option<Weight>,
    pub(super) border: Option<Hsla>,
}

/// What of a space shows, in its units, as it was last laid out; with points per unit, to
/// reach as far as a shape sized in points does.
pub(super) struct Shown {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    per_point: (f64, f64),
}

impl Shown {
    /// `None` before the space was laid out: then everything is placed.
    pub(super) fn of(view: &SpaceView) -> Option<Self> {
        let (across, down) = view.scale();
        (view.width > 0.0 && across > 0.0 && down > 0.0).then(|| Self {
            left: view.left,
            right: view.left + view.width / across,
            top: view.top,
            bottom: view.top + view.height / down,
            per_point: (1.0 / across, 1.0 / down),
        })
    }

    /// Whether what spans `left..right` and `top..bottom`, and `reach` points further each way,
    /// may show.
    fn reaches(
        &self,
        (left, right): (f64, f64),
        (top, bottom): (f64, f64),
        reach: (f32, f32),
    ) -> bool {
        let (x, y) = (
            f64::from(reach.0) * self.per_point.0,
            f64::from(reach.1) * self.per_point.1,
        );
        left.min(right) - x <= self.right
            && left.max(right) + x >= self.left
            && top.min(bottom) - y <= self.bottom
            && top.max(bottom) + y >= self.top
    }

    fn shows(&self, geometry: &Geometry) -> bool {
        let Extent {
            across,
            down,
            reach,
        } = Extent::of(geometry);
        self.reaches(across, down, reach)
    }
}

/// Where a shape is in the units of its space, and how many points it reaches further each
/// way, as a shape sized in points does.
#[derive(Clone, Copy)]
struct Extent {
    across: (f64, f64),
    down: (f64, f64),
    reach: (f32, f32),
}

impl Extent {
    fn of(geometry: &Geometry) -> Self {
        let extent = |across, down, reach| Self {
            across,
            down,
            reach,
        };
        match *geometry {
            Geometry::Rect {
                x,
                y,
                width,
                height,
                ..
            }
            | Geometry::Wave {
                x,
                y,
                width,
                height,
                ..
            }
            | Geometry::Notes {
                x,
                y,
                width,
                height,
                ..
            } => extent((x, x + width), (y, y + height), (1.0, 1.0)),
            Geometry::Circle { x, y, radius } | Geometry::Arc { x, y, radius, .. } => {
                extent((x, x), (y, y), (radius + 2.0, radius + 2.0))
            }
            Geometry::Line { from, to, width } => {
                extent((from.0, to.0), (from.1, to.1), (width, width))
            }
            Geometry::Path { ref bends, outline } => {
                let fold = |pick: fn((f64, f64)) -> f64| {
                    (bends.iter().flat_map(|bend| bend.points()).map(pick))
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                            (low.min(value), high.max(value))
                        })
                };
                let reach = outline.unwrap_or(1.0);
                extent(fold(|point| point.0), fold(|point| point.1), (reach, reach))
            }
            // A line of text is as wide as its words: a long label far off to the left may
            // still reach in.
            Geometry::Text { x, y, size, .. } => extent((x, x), (y, y), (size * 40.0, size * 2.0)),
        }
    }
}

/// The shapes of a space in the order they paint, kept from one batch of ops to the next. A
/// frame works out only the shapes that move and those of the rest that show, so a long piece
/// costs what shows.
pub(crate) struct SpaceIndex {
    pub(super) version: u64,
    shapes: Vec<Indexed>,
    /// By the place of each shape in `shapes`: whether it and the groups that move it are
    /// [`tree::ShapeProps::steady`].
    steady: Vec<bool>,
    /// What the steady shapes gave in the frame before, while what they read stays the same.
    kept: Kept,
}

/// What the steady shapes of a space gave, and what they read when they gave it.
#[derive(Default)]
struct Kept {
    reads: Option<Reads>,
    /// By the place of each shape in `shapes`: what it gave, unless it is not steady.
    shaped: Vec<Option<Option<Showing>>>,
    /// How many shapes showed, and how many of those were placed, in the last frame.
    shown: (usize, usize),
}

/// All a steady shape reads: how the spaces show, the drag, and what the pointer is over.
struct Reads {
    spaces: HashMap<String, SpaceView>,
    drag: Option<(String, DragNumbers)>,
    hovered: (Option<u64>, Option<String>),
}

impl Reads {
    fn of(sources: &Sources<'_>, hovered: Option<u64>) -> Self {
        Self {
            spaces: sources.spaces.clone(),
            drag: (sources.drag).map(|(id, numbers)| (id.to_string(), numbers)),
            hovered: (hovered, sources.hovered.map(str::to_string)),
        }
    }

    fn are(&self, sources: &Sources<'_>, hovered: Option<u64>) -> bool {
        let drag = (self.drag.as_ref()).map(|(id, numbers)| (id.as_str(), *numbers));
        self.spaces == *sources.spaces
            && drag == sources.drag
            && self.hovered.0 == hovered
            && self.hovered.1.as_deref() == sources.hovered
    }
}

/// What a shape that shows gives a frame: what it does with the pointer, and where it is and
/// in what colour unless it is see-through.
#[derive(Clone)]
struct Showing {
    pointer: Option<Pointer>,
    placed: Option<(Geometry, Hsla, Option<Area>)>,
}

enum Indexed {
    /// A shape that stays where it is, at `offset` from the groups around it, inside `clip`.
    Stays {
        slot: usize,
        offset: (f64, f64),
        clip: Option<Area>,
        extent: Extent,
    },
    /// A shape that moves, or one in a group that does: worked out on every frame, from
    /// `offset` and `clip` and the groups in `groups` that move.
    Moves {
        slot: usize,
        offset: (f64, f64),
        clip: Option<Area>,
        groups: Rc<[usize]>,
    },
}

/// The shapes of a space as they are placed, in the order they paint.
pub(super) struct Placing<'a> {
    pub(super) sources: Sources<'a>,
    pub(super) table: &'a NodeTable,
    /// Shapes outside it are not placed, so a long piece costs what shows.
    pub(super) shown: Option<Shown>,
    /// The node of the shape of this space the pointer is over.
    pub(super) hovered: Option<u64>,
    pub(super) placed: Vec<Placed>,
    /// What each shape does with the pointer, by its place; a group has none.
    pub(super) pointed: Vec<Option<Pointer>>,
}

impl Placing<'_> {
    fn paint(&self, paint: &Paint) -> Hsla {
        match paint {
            Paint::Fixed(color) => color.0,
            Paint::Mix { from, to, by } => {
                let by = self.sources.number(by).clamp(0.0, 1.0) as f32;
                let (from, to) = (from.0.to_rgb(), to.0.to_rgb());
                let mix = |from: f32, to: f32| from + (to - from) * by;
                gpui::Rgba {
                    r: mix(from.r, to.r),
                    g: mix(from.g, to.g),
                    b: mix(from.b, to.b),
                    a: mix(from.a, to.a),
                }
                .into()
            }
        }
    }

    /// The values of a wave, oldest first.
    fn list(&self, list: Option<&List>) -> Vec<f32> {
        let feeds = self.sources.feeds;
        match list {
            Some(List::Scope {
                name,
                channel,
                frames,
            }) => {
                let Some((kept, _)) = feeds.scopes.get(name) else {
                    return Vec::new();
                };
                let count = frames.map_or(kept.len(), |frames| frames as usize);
                let skip = kept.len().saturating_sub(count);
                let pick = |frame: &[f32; 2]| match channel {
                    Some(0) => frame[0],
                    Some(_) => frame[1],
                    None => (frame[0] + frame[1]) / 2.0,
                };
                kept.iter().skip(skip).map(pick).collect()
            }
            Some(List::History { of, frames, key }) => {
                feeds.history(key, *frames, self.sources.number(of))
            }
            Some(List::File { asset, from, to }) => feeds.wave(asset, *from, *to).to_vec(),
            Some(List::Take { name }) => (feeds.takes.get(name))
                .map(|wave| wave.to_vec())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn number(&self, number: &Option<tree::Num>) -> f64 {
        number
            .as_ref()
            .map_or(0.0, |number| self.sources.number(number))
    }

    /// The shapes under `children`, with where those that stay are.
    pub(super) fn index(&self, children: &[usize], version: u64) -> SpaceIndex {
        let mut shapes = Vec::with_capacity(children.len());
        self.index_into(children, ((0.0, 0.0), None), &Rc::from([]), &mut shapes);
        let steady_at = |slot: &usize| match self.table.get(*slot).map(|entry| &entry.props) {
            Some(Props::Shape(shape)) => shape.steady(),
            _ => true,
        };
        let steady = (shapes.iter())
            .map(|indexed| match indexed {
                Indexed::Stays { slot, .. } => steady_at(slot),
                Indexed::Moves { slot, groups, .. } => {
                    steady_at(slot) && groups.iter().all(steady_at)
                }
            })
            .collect();
        SpaceIndex {
            version,
            shapes,
            steady,
            kept: Kept::default(),
        }
    }

    fn index_into(
        &self,
        children: &[usize],
        (offset, clip): ((f64, f64), Option<Area>),
        groups: &Rc<[usize]>,
        into: &mut Vec<Indexed>,
    ) {
        let table = self.table;
        for slot in children {
            let Some(entry) = table.get(*slot) else {
                continue;
            };
            let Props::Shape(shape) = &entry.props else {
                continue;
            };
            if shape.kind == Some(ShapeKind::Group) {
                if groups.is_empty() && shape.group_stays() {
                    let placed = self.group(shape, (offset, clip));
                    self.index_into(&entry.children, placed, groups, into);
                } else {
                    let groups: Rc<[usize]> = groups.iter().copied().chain([*slot]).collect();
                    self.index_into(&entry.children, (offset, clip), &groups, into);
                }
            } else if groups.is_empty() && shape.stays() {
                let Some(geometry) = self.geometry(shape, offset) else {
                    continue;
                };
                into.push(Indexed::Stays {
                    slot: *slot,
                    offset,
                    clip,
                    extent: Extent::of(&geometry),
                });
            } else {
                into.push(Indexed::Moves {
                    slot: *slot,
                    offset,
                    clip,
                    groups: groups.clone(),
                });
            }
        }
    }

    /// Where a group puts its shapes, from where its own group put it: moved by its `x` and
    /// `y` and the drags it moves with, inside its box when it has one.
    fn group(
        &self,
        group: &tree::ShapeProps,
        ((left, top), clip): ((f64, f64), Option<Area>),
    ) -> ((f64, f64), Option<Area>) {
        let (moved_x, moved_y) = self.followed(&group.moves);
        let left = left + self.number(&group.x) + moved_x;
        let top = top + self.number(&group.y) + moved_y;
        let clip = match &group.clip {
            Some(tree::Clip { width, height }) => {
                let (grown_x, grown_y) = self.followed(&group.grows);
                let right = left + self.sources.number(width) + grown_x;
                let bottom = top + self.sources.number(height) + grown_y;
                Some(Area([left, top, right, bottom]).within(clip))
            }
            None => clip,
        };
        ((left, top), clip)
    }

    /// Places the shapes of `index` that move, and those of the rest that show. A steady shape
    /// gives what it gave in the frame before, while what it reads is the same.
    pub(super) fn indexed(&mut self, index: &mut SpaceIndex) {
        let kept = &mut index.kept;
        // As many as in the last frame, rather than growing from none.
        self.pointed.reserve(kept.shown.0);
        self.placed.reserve(kept.shown.1);
        let same =
            (kept.reads.as_ref()).is_some_and(|reads| reads.are(&self.sources, self.hovered));
        if !same || kept.shaped.len() != index.shapes.len() {
            kept.shaped = vec![None; index.shapes.len()];
            kept.reads = Some(Reads::of(&self.sources, self.hovered));
        }
        for ((indexed, steady), kept) in (index.shapes.iter())
            .zip(&index.steady)
            .zip(&mut kept.shaped)
        {
            let shaped = match kept {
                Some(shaped) => shaped.clone(),
                None => {
                    let shaped = self.shaped(indexed);
                    if *steady {
                        *kept = Some(shaped.clone());
                    }
                    shaped
                }
            };
            self.put(shaped);
        }
        index.kept.shown = (self.pointed.len(), self.placed.len());
    }

    /// What `indexed` gives this frame.
    fn shaped(&self, indexed: &Indexed) -> Option<Showing> {
        let table = self.table;
        let (slot, placed) = match indexed {
            Indexed::Stays {
                slot,
                offset,
                clip,
                extent,
            } => {
                let shows =
                    self.shown.as_ref().is_none_or(|shown| {
                        shown.reaches(extent.across, extent.down, extent.reach)
                    }) && clip.is_none_or(|clip| clip.meets(extent));
                if !shows {
                    return None;
                }
                (*slot, (*offset, *clip))
            }
            Indexed::Moves {
                slot,
                offset,
                clip,
                groups,
            } => {
                // A shape that moves inside the box of a group that stays shows only when
                // that box does: the handle at the end of a clip far off.
                let boxed_off = |shown: &Shown| {
                    clip.is_some_and(|Area([left, top, right, bottom])| {
                        !shown.reaches((left, right), (top, bottom), (0.0, 0.0))
                    })
                };
                if groups.is_empty() && self.shown.as_ref().is_some_and(boxed_off) {
                    return None;
                }
                let mut placed = (*offset, *clip);
                for group in groups.iter() {
                    if let Some(Props::Shape(group)) = table.get(*group).map(|entry| &entry.props) {
                        placed = self.group(group, placed);
                    }
                }
                (*slot, placed)
            }
        };
        match table.get(slot) {
            Some(entry) if let Props::Shape(shape) = &entry.props => {
                self.shape(entry.id, shape, placed)
            }
            _ => None,
        }
    }

    /// How far the drag that goes on moves a shape that follows it with `follows`.
    fn followed(&self, follows: &Option<Vec<String>>) -> (f64, f64) {
        match (self.sources.drag, follows) {
            (Some((held, numbers)), Some(ids)) if ids.iter().any(|id| id == held) => {
                (numbers.dx, numbers.dy)
            }
            _ => (0.0, 0.0),
        }
    }

    fn shape(
        &self,
        id: u64,
        shape: &tree::ShapeProps,
        ((left, top), clip): ((f64, f64), Option<Area>),
    ) -> Option<Showing> {
        let hovered = self.hovered == Some(id);
        let (moved_x, moved_y) = self.followed(&shape.moves);
        let (left, top) = (left + moved_x, top + moved_y);
        let Some(geometry) = self.geometry(shape, (left, top)) else {
            return None;
        };
        let extent = Extent::of(&geometry);
        let hidden = self
            .shown
            .as_ref()
            .is_some_and(|shown| !shown.shows(&geometry))
            || clip.is_some_and(|clip| !clip.meets(&extent));
        if hidden {
            return None;
        }
        let origin = geometry.origin();
        let anchor = shape.anchor.unwrap_or_default();
        let pointer = shape.is_pointed().then(|| Pointer {
            node: id,
            hears: Hears::of(shape.on_press, shape.on_release, shape.on_click),
            drag: self.table.drag(shape.drag).cloned(),
            cursor: shape.cursor,
            anchor: (
                anchor.x.map_or(origin.0, |x| left + f64::from(x)),
                anchor.y.map_or(origin.1, |y| top + f64::from(y)),
            ),
        });
        let opacity = (shape.opacity.as_ref())
            .map_or(1.0, |opacity| self.sources.number(opacity).clamp(0.0, 1.0))
            as f32;
        if opacity <= 0.0 {
            return Some(Showing {
                pointer,
                placed: None,
            });
        }
        let mut color = match (hovered.then_some(shape.hover).flatten(), &shape.color) {
            (Some(hover), _) => hover.0,
            (None, Some(paint)) => self.paint(paint),
            (None, None) => gpui::white(),
        };
        color.a *= opacity;
        Some(Showing {
            pointer,
            placed: Some((geometry, color, clip)),
        })
    }

    fn put(&mut self, shaped: Option<Showing>) {
        let Some(Showing { pointer, placed }) = shaped else {
            return;
        };
        let index = self.pointed.len();
        self.pointed.push(pointer);
        if let Some((geometry, color, clip)) = placed {
            self.placed.push(Placed {
                index,
                geometry,
                color,
                clip,
            });
        }
    }

    /// Where `shape` is and how big, at `left` and `top` in its space; `None` for a group.
    fn geometry(&self, shape: &tree::ShapeProps, (left, top): (f64, f64)) -> Option<Geometry> {
        let (grown_x, grown_y) = self.followed(&shape.grows);
        let (shrunk_x, shrunk_y) = self.followed(&shape.shrinks);
        let (grown_x, grown_y) = (grown_x - shrunk_x, grown_y - shrunk_y);
        let reach = |reach: &Option<Reach>| match reach {
            Some(Reach::Point([x, y])) => {
                (left + self.sources.number(x), top + self.sources.number(y))
            }
            Some(Reach::Turn(turn)) => (self.sources.number(turn), 0.0),
            None => (left, top),
        };
        let (x, y) = (left + self.number(&shape.x), top + self.number(&shape.y));
        let geometry = match shape.kind? {
            ShapeKind::Rect => {
                let border_width = self.number(&shape.border_width) as f32;
                Geometry::Rect {
                    x,
                    y,
                    width: (self.number(&shape.width) + grown_x).max(0.0),
                    height: (self.number(&shape.height) + grown_y).max(0.0),
                    radius: self.number(&shape.radius) as f32,
                    border: (shape.border)
                        .filter(|_| border_width > 0.0)
                        .map(|border| (border.0, border_width)),
                }
            }
            ShapeKind::Circle => Geometry::Circle {
                x,
                y,
                radius: self.number(&shape.radius) as f32,
            },
            ShapeKind::Line => Geometry::Line {
                from: reach(&shape.from),
                to: reach(&shape.to),
                width: self.number(&shape.width).max(0.0) as f32,
            },
            ShapeKind::Arc => Geometry::Arc {
                x,
                y,
                radius: self.number(&shape.radius) as f32,
                turns: (reach(&shape.from).0 as f32, reach(&shape.to).0 as f32),
                width: self.number(&shape.width).max(0.0) as f32,
            },
            ShapeKind::Text => Geometry::Text {
                x,
                y,
                text: match &shape.text {
                    Some(Words::Fixed(text)) => text.clone().into(),
                    Some(Words::Parts(parts)) => (parts.iter())
                        .map(|part| part.text.as_str())
                        .collect::<String>()
                        .into(),
                    Some(Words::Live(live)) => {
                        let value = self.sources.number(live);
                        text_of(value, shape.unit, self.sources.feeds.clock.as_ref()).into()
                    }
                    None => SharedString::default(),
                },
                parts: match &shape.text {
                    Some(Words::Parts(parts)) => (parts.iter())
                        .map(|part| TextPart {
                            len: part.text.len(),
                            color: part.color.as_ref().map(|color| color.0),
                            weight: part.weight,
                            border: part.border.as_ref().map(|color| color.0),
                        })
                        .collect(),
                    _ => Rc::new([]),
                },
                size: shape.size.unwrap_or(12.0),
                align: shape.align.unwrap_or(Align::Left),
            },
            ShapeKind::Wave => {
                let width = self.number(&shape.width);
                let level = (shape.level.iter().flatten())
                    .map(|[along, factor]| {
                        let along = self.sources.number(along) / width.max(f64::MIN_POSITIVE);
                        (along as f32, self.sources.number(factor) as f32)
                    })
                    .collect();
                Geometry::Wave {
                    x,
                    y,
                    width,
                    height: self.number(&shape.height),
                    wave: Rc::new(Wave {
                        values: self.list(shape.list.as_ref()),
                        low: shape.low.unwrap_or(-1.0),
                        high: shape.high.unwrap_or(1.0),
                        band: shape.band == Some(true),
                        line: shape.size.unwrap_or(1.0),
                        level,
                    }),
                }
            }
            ShapeKind::Notes => {
                let take = match &shape.list {
                    Some(List::Take { name }) => self.sources.feeds.take_notes.get(name),
                    _ => None,
                };
                Geometry::Notes {
                    x,
                    y,
                    width: self.number(&shape.width),
                    height: self.number(&shape.height),
                    notes: Rc::new(
                        take.map_or(Notes::NONE, |take| Notes::of(take, self.sources.tick)),
                    ),
                }
            }
            ShapeKind::Path => {
                let at = |x: &tree::Num, y: &tree::Num| {
                    (left + self.sources.number(x), top + self.sources.number(y))
                };
                let width = self.number(&shape.width) as f32;
                Geometry::Path {
                    bends: (shape.points.iter().flatten())
                        .map(|each| match each {
                            PathPoint::To([x, y]) => Bend::To(at(x, y)),
                            PathPoint::Quadratic([cx, cy, x, y]) => {
                                Bend::Quadratic(at(cx, cy), at(x, y))
                            }
                            PathPoint::Cubic([ax, ay, bx, by, x, y]) => {
                                Bend::Cubic(at(ax, ay), at(bx, by), at(x, y))
                            }
                        })
                        .collect(),
                    outline: (width > 0.0).then_some(width),
                }
            }
            ShapeKind::Group => return None,
        };
        Some(geometry)
    }
}

/// A number as a readout of `unit`.
fn readout(value: f32, unit: Option<Unit>) -> String {
    match unit {
        Some(Unit::Hz) => hertz_readout(value),
        Some(Unit::Ms) => milliseconds_readout(value),
        Some(Unit::Db) => decibels_readout(value),
        Some(Unit::Percent) => percent_readout(value),
        None => short(value),
    }
}

/// A live number as text shows it: a tick as a place in the piece (`5.3`), as minutes and seconds
/// (`1:07`), or as the time signature there (`7/8`).
fn text_of(value: f64, shown: Option<Readout>, clock: Option<&sound_core::Clock>) -> String {
    let unit = |unit| readout(value as f32, Some(unit));
    let tick = sound_core::Ticks(value.max(0.0) as u64);
    let signatures = clock.map(|clock| clock.tempo_map().time_signatures());
    match shown {
        None => readout(value as f32, None),
        Some(Readout::Hz) => unit(Unit::Hz),
        Some(Readout::Ms) => unit(Unit::Ms),
        Some(Readout::Db) => unit(Unit::Db),
        Some(Readout::Percent) => unit(Unit::Percent),
        Some(Readout::Pan) => pan_readout(value as f32),
        Some(Readout::Position) => signatures.map_or_else(String::new, |signatures| {
            let position = signatures.bar_beat_of(tick);
            format!("{}.{}", position.bar, position.beat)
        }),
        Some(Readout::Time) => clock.map_or_else(String::new, |clock| {
            let seconds = clock.seconds_of(tick).max(0.0) as u64;
            format!("{}:{:02}", seconds / 60, seconds % 60)
        }),
        Some(Readout::Signature) => signatures.map_or_else(String::new, |signatures| {
            signatures.bar_at(tick).signature.to_string()
        }),
        Some(Readout::Beats) => signatures.map_or_else(String::new, |signatures| {
            signatures.bar_at(tick).signature.numerator().to_string()
        }),
        Some(Readout::Note) => signatures.map_or_else(String::new, |signatures| {
            signatures.bar_at(tick).signature.denominator().to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wave_is_scaled_straight_between_the_points_of_its_level_and_as_the_nearest_past_them() {
        let wave = Wave {
            values: Vec::new(),
            low: -1.0,
            high: 1.0,
            band: true,
            line: 1.0,
            level: vec![(0.1, 0.0), (0.3, 0.5), (0.9, 0.5), (1.0, 0.0)],
        };
        let near =
            |at: f32, expected: f32| assert!((wave.level_at(at) - expected).abs() < 1e-5, "{at}");
        near(0.0, 0.0);
        near(0.2, 0.25);
        near(0.6, 0.5);
        near(0.95, 0.25);
        let plain = Wave {
            level: Vec::new(),
            ..wave
        };
        assert_eq!(plain.level_at(0.5), 1.0);
    }
}
