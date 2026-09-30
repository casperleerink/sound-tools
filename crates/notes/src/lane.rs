//! The expression lanes of a clip: how the bend wheel, the modulation wheel and the key
//! pressure move through it.
//!
//! A lane is a list of points. Between two points the value moves in a straight line, before
//! the first point it holds the first value, and after the last it holds the last. Everyone
//! who plays or draws a lane reads it through [`value_at`], so they all agree.

use std::fmt::Debug;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use sound_core::Ticks;

use crate::{Amount, Bend};

/// Where a lane stands at `tick`, which counts from the start of the clip like a note start.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point<V> {
    pub tick: Ticks,
    pub value: V,
}

/// What a lane holds: a [`Bend`] or an [`Amount`].
pub trait LaneValue: Copy + Eq + Debug {
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

impl LaneValue for Bend {
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

impl LaneValue for Amount {
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
        let from = f64::from(self.value.number());
        let rise = f64::from(next.value.number()) - from;
        let done = (tick.0 - self.tick.0) as f64 / (next.tick.0 - self.tick.0) as f64;
        V::nearest((from + rise * done).round() as i64)
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

/// The lane with every point dropped that the straight line through the points it keeps
/// passes within [`LaneValue::STEP`] of. The first and the last point stay. So a recorded
/// wheel is a small file, and it plays as it was played.
///
/// One pass: from the last kept point, the lines that pass every point since within a step
/// have slopes between `lowest` and `highest`. A point whose own slope is outside them is the
/// end of a line, and the point before it is kept. A minute of pressure is thinned as fast as
/// it is read.
pub fn thinned<V: LaneValue>(points: &[Point<V>]) -> Vec<Point<V>> {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Vec::new();
    };
    let mut kept = vec![*first];
    let mut anchor = *first;
    let (mut lowest, mut highest) = (f64::NEG_INFINITY, f64::INFINITY);
    let slope = |from: &Point<V>, to: &Point<V>, offset: i32| {
        let rise = f64::from(to.value.number() + offset - from.value.number());
        rise / (to.tick.0 as f64 - from.tick.0 as f64)
    };
    for pair in points.windows(2) {
        let [before, point] = pair else {
            continue;
        };
        if !(lowest..=highest).contains(&slope(&anchor, point, 0)) {
            anchor = *before;
            kept.push(anchor);
            (lowest, highest) = (f64::NEG_INFINITY, f64::INFINITY);
        }
        lowest = lowest.max(slope(&anchor, point, -V::STEP));
        highest = highest.min(slope(&anchor, point, V::STEP));
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
    let mut before: Option<Ticks> = None;
    for (index, point) in points.iter().enumerate() {
        if point.tick >= length {
            return Err(format!(
                "{field}[{index}].tick must be less than the clip length {}, not {}. A point counts from the start of its clip, not from the start of the project",
                length.0, point.tick.0
            ));
        }
        if let Some(before) = before
            && point.tick <= before
        {
            return Err(format!(
                "{field}[{index}].tick must be after the tick of the point before it, {}, not {}. The points of a lane are in tick order, one per tick",
                before.0, point.tick.0
            ));
        }
        before = Some(point.tick);
    }
    Ok(())
}
