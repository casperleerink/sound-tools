//! Lanes: how a value moves over time. The expression lanes of a clip move the bend wheel, the
//! modulation wheel and the key pressure through it; an automation lane of a track moves a
//! number of a device, as a place on the travel of its knob.
//!
//! A lane is a list of points. Between two points the value moves in a straight line, before
//! the first point it holds the first value, and after the last it holds the last. Everyone
//! who plays or draws a lane reads it through [`value_at`], so they all agree.

use std::fmt::Debug;
use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{Clock, Frames, Ticks, Transport};

use crate::{Amount, Bend};

/// Where a lane stands at `tick`, which counts from the start of the clip like a note start.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Point<V> {
    pub tick: Ticks,
    pub value: V,
}

/// What a lane holds, and the straight line between two of its values.
pub trait LaneValue: Copy + PartialEq + Debug {
    /// The value `done` of the way from `from` to `to`, with `done` from 0 to 1.
    fn between(from: Self, to: Self, done: f64) -> Self;
}

/// A plain number, such as a place on the travel of a knob.
impl LaneValue for f32 {
    fn between(from: Self, to: Self, done: f64) -> Self {
        let from = f64::from(from);
        (from + (f64::from(to) - from) * done) as f32
    }
}

/// What an expression lane holds: a [`Bend`] or an [`Amount`], in the steps MIDI sends.
pub trait ExpressionValue: LaneValue + Eq {
    /// Where the lane stands when no clip moves it, as after an `AllOff`.
    const REST: Self;
    /// The lowest and the highest value, as numbers.
    const LOWEST: i32;
    const HIGHEST: i32;
    /// How far a recorded lane may pass from a point it drops when it is thinned: one step of
    /// what a keyboard sends.
    const STEP: i32;

    fn number(self) -> i32;

    /// The value nearest to any number.
    fn nearest(number: i64) -> Self;
}

/// The step nearest to the straight line between two steps.
fn nearest_between<V: ExpressionValue>(from: V, to: V, done: f64) -> V {
    let from = f64::from(from.number());
    let rise = f64::from(to.number()) - from;
    V::nearest((from + rise * done).round() as i64)
}

impl LaneValue for Bend {
    fn between(from: Self, to: Self, done: f64) -> Self {
        nearest_between(from, to, done)
    }
}

impl LaneValue for Amount {
    fn between(from: Self, to: Self, done: f64) -> Self {
        nearest_between(from, to, done)
    }
}

impl ExpressionValue for Bend {
    const REST: Self = Self::MIDDLE;
    const LOWEST: i32 = -8192;
    const HIGHEST: i32 = 8191;
    /// One step of the coarse half of MIDI's bend, 128 of 16384: most keyboards send the bend in
    /// those steps, and it is 3 cents at a bend of two semitones. A step of 1 would keep nearly
    /// every point of a recorded bend.
    const STEP: i32 = 128;

    fn number(self) -> i32 {
        i32::from(self.value())
    }

    fn nearest(number: i64) -> Self {
        Bend::nearest(number)
    }
}

impl ExpressionValue for Amount {
    const REST: Self = Self::NONE;
    const LOWEST: i32 = 0;
    const HIGHEST: i32 = 127;
    const STEP: i32 = 1;

    fn number(self) -> i32 {
        i32::from(self.value())
    }

    fn nearest(number: i64) -> Self {
        Amount::nearest(number)
    }
}

impl<V: LaneValue> Point<V> {
    /// The value at `tick` on the straight line from this point to `next`: this value before
    /// this point, and the value of `next` from `next` on.
    pub fn towards(self, next: Self, tick: Ticks) -> V {
        if tick <= self.tick {
            return self.value;
        }
        if tick >= next.tick {
            return next.value;
        }
        let done = (tick.0 - self.tick.0) as f64 / (next.tick.0 - self.tick.0) as f64;
        V::between(self.value, next.value, done)
    }
}

/// Where a lane stands at `tick`. `None` for a lane with no points, which moves nothing.
pub fn value_at<V: LaneValue>(points: &[Point<V>], tick: Ticks) -> Option<V> {
    let after = points.partition_point(|point| point.tick <= tick);
    let before = after.checked_sub(1).and_then(|index| points.get(index));
    match (before, points.get(after)) {
        (Some(before), Some(next)) => Some(before.towards(*next, tick)),
        (Some(only), None) | (None, Some(only)) => Some(only.value),
        (None, None) => None,
    }
}

/// The most pieces a player of lanes cuts a block into, see [`pieces`]. Bends past these in one
/// block, which only lanes with points a few frames apart have, ramp straight to the end of the
/// block.
pub const MAX_PIECES: usize = 4;

/// The pieces a player of `lines` plays a block in: each starts at an offset in the block and
/// takes the value of every line at the project frame where it ends, so the ramp of each piece
/// ends on the line. One piece without a bend; else a cut at each frame inside the block where
/// some line bends, the earliest first. Gives the pieces and how many there are.
pub fn pieces<'a>(
    lines: impl IntoIterator<Item = &'a [Point<f32>]>,
    transport: &Transport<'_>,
) -> ([(usize, Frames); MAX_PIECES], usize) {
    let mut bends = [(0, Frames(0)); MAX_PIECES - 1];
    let mut count = 0;
    let range = &transport.tick_range;
    for points in lines {
        let after = points.partition_point(|point| point.tick < range.start);
        let points = points.get(after..).unwrap_or_default().iter();
        for point in points.take_while(|point| point.tick < range.end) {
            let Some(offset) = transport.offset_of(point.tick).filter(|offset| *offset > 0) else {
                continue;
            };
            let found = bends.get(..count).unwrap_or_default();
            let at = found.partition_point(|(other, _)| *other < offset);
            if found.get(at).is_some_and(|(other, _)| *other == offset) || at == bends.len() {
                continue;
            }
            count = (count + 1).min(bends.len());
            if let Some(later) = bends.get_mut(at..count) {
                later.rotate_right(1);
                if let Some(bend) = later.first_mut() {
                    *bend = (offset, transport.clock.frame_of(point.tick));
                }
            }
        }
    }
    let mut pieces = [(0, transport.frame_range.end); MAX_PIECES];
    for (index, (offset, frame)) in bends.iter().take(count).enumerate() {
        if let Some(piece) = pieces.get_mut(index) {
            piece.1 = *frame;
        }
        if let Some(piece) = pieces.get_mut(index + 1) {
            piece.0 = *offset;
        }
    }
    (pieces, count + 1)
}

/// Where a lane of places on a travel stands at the project frame `frame`, on the straight line
/// between the frames of the points around it: what a device hears. A tick is many frames, so a
/// ramp a few ticks long would reach its point up to a tick early if it ended on a whole tick.
pub fn value_at_frame(points: &[Point<f32>], frame: Frames, clock: &Clock) -> Option<f32> {
    let after = points.partition_point(|point| clock.frame_of(point.tick) <= frame);
    let before = after.checked_sub(1).and_then(|index| points.get(index));
    match (before, points.get(after)) {
        (Some(before), Some(next)) => {
            let from = clock.frame_of(before.tick).0;
            let to = clock.frame_of(next.tick).0;
            let done = (frame.0 - from) as f64 / (to - from) as f64;
            Some(f32::between(before.value, next.value, done))
        }
        (Some(only), None) | (None, Some(only)) => Some(only.value),
        (None, None) => None,
    }
}

/// The lane with every point dropped that the straight line through the points it keeps
/// passes within [`ExpressionValue::STEP`] of. The first and the last point stay. So a recorded
/// wheel is a small file, and it plays as it was played.
///
/// One pass: from the last kept point, the lines that pass every point since within a step
/// have slopes between `lowest` and `highest`. A point whose own slope is outside them is the
/// end of a line, and the point before it is kept. A minute of pressure is thinned as fast as
/// it is read.
pub fn thinned<V: ExpressionValue>(points: &[Point<V>]) -> Vec<Point<V>> {
    let number = |value: V| f64::from(value.number());
    let step = f64::from(V::STEP);
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Vec::new();
    };
    let mut kept = vec![*first];
    let mut anchor = *first;
    let (mut lowest, mut highest) = (f64::NEG_INFINITY, f64::INFINITY);
    let slope = |from: &Point<V>, to: &Point<V>, offset: f64| {
        let rise = number(to.value) + offset - number(from.value);
        rise / (to.tick.0 as f64 - from.tick.0 as f64)
    };
    for pair in points.windows(2) {
        let [before, point] = pair else {
            continue;
        };
        if !(lowest..=highest).contains(&slope(&anchor, point, 0.)) {
            anchor = *before;
            kept.push(anchor);
            (lowest, highest) = (f64::NEG_INFINITY, f64::INFINITY);
        }
        lowest = lowest.max(slope(&anchor, point, -step));
        highest = highest.min(slope(&anchor, point, step));
    }
    if points.len() > 1 {
        kept.push(*last);
    }
    kept
}

/// The part of a lane inside `range`, with ticks counted from its start. Where points are cut
/// away, a point on the edge keeps the value the lane had there, so what is left moves as it
/// did.
pub fn cut<V: LaneValue>(points: &[Point<V>], range: Range<Ticks>) -> Vec<Point<V>> {
    if range.is_empty() {
        return Vec::new();
    }
    let from_start = |tick: Ticks| tick.saturating_sub(range.start);
    let edge = |tick: Ticks| {
        value_at(points, tick).map(|value| Point {
            tick: from_start(tick),
            value,
        })
    };
    let mut kept = Vec::new();
    if points.first().is_some_and(|point| point.tick < range.start)
        && !points.iter().any(|point| point.tick == range.start)
    {
        kept.extend(edge(range.start));
    }
    let inside = points.iter().filter(|point| range.contains(&point.tick));
    kept.extend(inside.map(|point| Point {
        tick: from_start(point.tick),
        value: point.value,
    }));
    let last_tick = Ticks(range.end.0 - 1);
    if points.last().is_some_and(|point| point.tick > last_tick)
        && kept.last().map(|point| point.tick) != Some(from_start(last_tick))
    {
        kept.extend(edge(last_tick));
    }
    kept
}

/// Whether a lane of a clip `length` long follows the rules of a clip: every point inside it,
/// in tick order, one per tick. `field` is the name of the lane in the record.
pub(crate) fn check<V>(field: &str, points: &[Point<V>], length: Ticks) -> Result<(), String> {
    let mut points_inside = points.iter().enumerate();
    if let Some((index, point)) = points_inside.find(|(_, point)| point.tick >= length) {
        return Err(format!(
            "{field}[{index}].tick must be less than the clip length {}, not {}. A point counts from the start of its clip, not from the start of the project",
            length.0, point.tick.0
        ));
    }
    check_order(field, points)
}

/// Whether the points of a lane are in tick order, one per tick. `field` is the name of the
/// lane in the record.
pub fn check_order<V>(field: &str, points: &[Point<V>]) -> Result<(), String> {
    for (index, pair) in (1..).zip(points.windows(2)) {
        let [before, point] = pair else {
            continue;
        };
        if point.tick <= before.tick {
            return Err(format!(
                "{field}[{index}].tick must be after the tick of the point before it, {}, not {}. The points of a lane are in tick order, one per tick",
                before.tick.0, point.tick.0
            ));
        }
    }
    Ok(())
}
