//! The notes, the pedal and the lanes of clips at their place on the timeline, as a
//! [`NotePlayer`](crate::NotePlayer) plays them.

use std::iter::once;
use std::ops::Range;

use sound_core::Ticks;

use crate::{
    Amount, Bend, Clip, Expression, ExpressionValue, Pedal, Pitch, PlacedNote, PlacedPedal, Point,
};

/// Every note of the clips at their place on the timeline, sorted by start and then pitch, so a
/// block finds its notes with one binary search. The sustain pedal and the lanes are kept the
/// same way.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct NoteTimeline {
    notes: Vec<PlacedNote>,
    pedal: Vec<PlacedPedal>,
    bend: PlayedLane<Bend>,
    mod_wheel: PlayedLane<Amount>,
    pressure: PlayedLane<Amount>,
}

impl NoteTimeline {
    /// Clips may overlap: the notes and the pedal moves of all of them play. A lane has one
    /// owner at a time, see [`PlayedLane::new`].
    pub fn new<'a>(clips: impl IntoIterator<Item = &'a Clip>) -> Self {
        let clips: Vec<&Clip> = clips.into_iter().collect();
        let bend = PlayedLane::new(&clips, |clip| &clip.bend);
        let mod_wheel = PlayedLane::new(&clips, |clip| &clip.mod_wheel);
        let pressure = PlayedLane::new(&clips, |clip| &clip.pressure);
        let mut notes = Vec::new();
        let mut pedal = Vec::new();
        let mut own = Vec::new();
        for clip in clips {
            notes.extend(clip.placed_notes());
            own.clear();
            own.extend(clip.placed_pedal());
            one_per_tick(&mut own);
            // The pedal of a clip ends with the clip, as a note that is longer than the rest
            // of its clip ends there. Without this a clip that ends under the pedal, or one
            // that was trimmed before its lift, would sustain for the rest of the piece.
            let ends_down = own.last().is_some_and(|change| change.value.is_down());
            pedal.append(&mut own);
            if ends_down {
                pedal.push(PlacedPedal {
                    start: clip.end(),
                    value: Pedal::UP,
                });
            }
        }
        sort(&mut notes);
        one_per_tick(&mut pedal);
        Self {
            notes,
            pedal,
            bend,
            mod_wheel,
            pressure,
        }
    }

    pub fn pedal(&self) -> &[PlacedPedal] {
        &self.pedal
    }

    pub(crate) fn starting_in(&self, range: &Range<Ticks>) -> &[PlacedNote] {
        starting_in(&self.notes, range)
    }

    pub(crate) fn pedal_starting_in(&self, range: &Range<Ticks>) -> &[PlacedPedal] {
        let first = self
            .pedal
            .partition_point(|change| change.start < range.start);
        let last = self
            .pedal
            .partition_point(|change| change.start < range.end);
        self.pedal.get(first..last).unwrap_or_default()
    }

    /// Where the pedal stands just before `tick`: the last move before it, or up. Clips may
    /// overlap, so of several moves at one tick the last in sorted order wins, as for notes.
    pub(crate) fn pedal_before(&self, tick: Ticks) -> Pedal {
        let before = self.pedal.partition_point(|change| change.start < tick);
        self.pedal
            .get(..before)
            .and_then(<[PlacedPedal]>::last)
            .map_or(Pedal::UP, |change| change.value)
    }

    /// Where the lanes stand at `tick`.
    pub(crate) fn expression_at(&self, tick: Ticks) -> Expression {
        Expression {
            bend: self.bend.at(tick),
            mod_wheel: self.mod_wheel.at(tick),
            pressure: self.pressure.at(tick),
        }
    }

    pub(crate) fn end_of(&self, start: Ticks, pitch: Pitch) -> Option<Ticks> {
        end_of(&self.notes, start, pitch)
    }
}

/// The whole note is the key, so the same notes always give the same order.
pub(crate) fn sort(notes: &mut [PlacedNote]) {
    notes.sort_unstable_by_key(|note| (note.start, note.pitch, note.end, note.velocity));
}

/// The notes of `notes`, sorted with [`sort`], that start in `range`.
pub(crate) fn starting_in<'a>(notes: &'a [PlacedNote], range: &Range<Ticks>) -> &'a [PlacedNote] {
    let first = notes.partition_point(|note| note.start < range.start);
    let last = notes.partition_point(|note| note.start < range.end);
    notes.get(first..last).unwrap_or_default()
}

/// Where the note of `notes`, sorted with [`sort`], that started at `start` with `pitch` ends
/// now. `None` when it is gone. Of several such notes, in clips that overlap, the last end.
pub(crate) fn end_of(notes: &[PlacedNote], start: Ticks, pitch: Pitch) -> Option<Ticks> {
    let first = notes.partition_point(|note| (note.start, note.pitch) < (start, pitch));
    let same = notes.get(first..).unwrap_or_default().iter();
    same.take_while(|note| (note.start, note.pitch) == (start, pitch))
        .map(|note| note.end)
        .max()
}

/// One expression lane of the clips as it plays: stretches of straight line on the project
/// timeline, in time order and never overlapping, from the clips that own the lane. Outside
/// them the lane is at rest.
#[derive(Debug, Default, PartialEq, Eq)]
struct PlayedLane<V> {
    pieces: Vec<Piece<V>>,
}

/// A stretch of a lane: from, up to, and the line it follows there, as two points on the
/// project timeline. Where a clip holds its first or last value the two points are the same.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct Piece<V> {
    from: Ticks,
    to: Ticks,
    line: [Point<V>; 2],
}

impl<V: ExpressionValue> PlayedLane<V> {
    /// The lane `points` of `clips`. Where clips with points in it overlap, the one that
    /// started last owns the lane, and of two that start together the later one in `clips`. A
    /// clip without points owns nothing.
    fn new(clips: &[&Clip], points: fn(&Clip) -> &Vec<Point<V>>) -> Self {
        let clips: Vec<_> = clips
            .iter()
            .map(|clip| (clip.start, clip.end(), points(clip)))
            .filter(|(.., points)| !points.is_empty())
            .collect();
        let mut edges: Vec<Ticks> = clips
            .iter()
            .flat_map(|(start, end, _)| [*start, *end])
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let mut pieces = Vec::new();
        for span in edges.windows(2) {
            let &[from, to] = span else {
                continue;
            };
            let holding = clips.iter().enumerate();
            let holding = holding.filter(|(_, (start, end, _))| *start <= from && from < *end);
            let Some((_, (start, _, points))) =
                holding.max_by_key(|(index, (start, ..))| (*start, *index))
            else {
                continue;
            };
            let placed = |point: &Point<V>| Point {
                tick: *start + point.tick,
                value: point.value,
            };
            let (Some(first), Some(last)) = (points.first(), points.last()) else {
                continue;
            };
            let (first, last) = (placed(first), placed(last));
            let between = points.windows(2).filter_map(|pair| match pair {
                [before, after] => Some([placed(before), placed(after)]),
                _ => None,
            });
            let lines = once((Ticks(0), [first, first]))
                .chain(between.map(|line| (line[0].tick, line)))
                .chain(once((last.tick, [last, last])));
            let mut lines = lines.peekable();
            while let Some((line_from, line)) = lines.next() {
                let line_to = lines.peek().map_or(Ticks(u64::MAX), |(next, _)| *next);
                let (from, to) = (line_from.max(from), line_to.min(to));
                if from < to {
                    pieces.push(Piece { from, to, line });
                }
            }
        }
        Self { pieces }
    }

    /// Where the lane stands at `tick`.
    fn at(&self, tick: Ticks) -> V {
        let index = self.pieces.partition_point(|piece| piece.to <= tick);
        let piece = self.pieces.get(index).filter(|piece| piece.from <= tick);
        piece.map_or(V::REST, |piece| piece.line[0].towards(piece.line[1], tick))
    }
}

/// Sorts pedal moves by tick and keeps one per tick, the most pressed of them.
///
/// A record may hold any number of moves on one tick, and clips that overlap add more. They
/// land on the same frame with no order among them, so the most pressed wins: a clip that
/// starts with the pedal down where another clip ends keeps it down. One per tick also bounds
/// the work of a block by the ticks it covers, whatever a record holds.
fn one_per_tick(pedal: &mut Vec<PlacedPedal>) {
    pedal.sort_unstable_by_key(|change| (change.start, change.value));
    pedal.dedup_by(|later, earlier| {
        let same_tick = later.start == earlier.start;
        if same_tick {
            *earlier = *later;
        }
        same_tick
    });
}
