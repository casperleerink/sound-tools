//! The automation lanes under a track in the timeline: one per automated number, with its name
//! and its line in the track colour, on the travel of the knob of the number, as it plays. Each
//! point shows as a dot: a press on one selects it and a drag moves it, a press anywhere else in
//! the lane adds one there. An alt-drag erases points as in the expression lanes of the note
//! editor, with a [`super::lanes::Stroke`]. Pure math, no GPUI and no project.

use std::iter::once;
use std::ops::{Range, RangeInclusive};

use sound_core::{Ticks, ValueRange};
use sound_notes::{Point, value_at};

use super::lanes::LaneBox;
use super::layout::{LANE_HEIGHT, Viewport};
use crate::automation::positions;
use crate::{AutomationLane, AutomationValue};

/// The air above the top of the travel and under its bottom, in a lane.
const LANE_AIR: f32 = 8.0;
/// Where a lane draws the travel of its knob: the bottom of the travel at the bottom.
pub const LANE_BOX: LaneBox = LaneBox {
    top: LANE_AIR,
    bottom: LANE_HEIGHT - LANE_AIR,
};
/// How far from the middle of the dot of a point a press still takes it, in pixels.
pub const POINT_REACH: f32 = 6.0;
/// The ends of fields that say the unit of a number, which its knob shows instead.
const UNITS: [&str; 7] = [
    "_hz",
    "_db",
    "_ms",
    "_seconds",
    "_cents",
    "_octaves",
    "_semitones",
];

/// The line of a lane of a number of `range` across what `ticks` shows, as the places where it
/// turns, from the left edge to the right edge: level before the first point and after the
/// last. Only the points that show and one on each side are put on the travel, as a lane may
/// have many and the timeline shows a few bars of it.
pub fn line(
    viewport: &Viewport,
    lane: &AutomationLane,
    range: ValueRange,
    ticks: Range<Ticks>,
) -> Vec<(f32, f32)> {
    let points = &lane.points;
    let from = points.partition_point(|point| point.tick <= ticks.start);
    let to = points.partition_point(|point| point.tick < ticks.end);
    let near = points.get(from.saturating_sub(1)..(to + 1).min(points.len()));
    let near = positions(near.unwrap_or_default(), range);
    let (Some(start), Some(end)) = (value_at(&near, ticks.start), value_at(&near, ticks.end))
    else {
        return Vec::new();
    };
    let placed = |tick: Ticks, position: f32| (viewport.x_of(tick), LANE_BOX.y_of(position));
    let shown = near.iter().filter(|point| ticks.contains(&point.tick));
    let shown = shown.filter(|point| point.tick > ticks.start);
    once(placed(ticks.start, start))
        .chain(shown.map(|point| placed(point.tick, point.value)))
        .chain(once(placed(ticks.end, end)))
        .collect()
}

/// `origin` without the points in `ticks`, as an alt-drag erases them. `None` when no point is
/// left: the lane is gone, and the number plays its record again.
pub fn erased(origin: &AutomationLane, ticks: &RangeInclusive<Ticks>) -> Option<AutomationLane> {
    let kept = origin
        .points
        .iter()
        .filter(|point| !ticks.contains(&point.tick));
    let lane = AutomationLane {
        points: kept.copied().collect(),
        ..origin.clone()
    };
    (!lane.points.is_empty()).then_some(lane)
}

/// The point of `lane` of a number of `range` whose dot is under `(x, y)` in the lane, the
/// nearest when dots overlap.
pub fn point_at(
    viewport: &Viewport,
    lane: &AutomationLane,
    range: ValueRange,
    (x, y): (f32, f32),
) -> Option<usize> {
    let (from, to) = (
        viewport.tick_at(x - POINT_REACH),
        viewport.tick_at(x + POINT_REACH),
    );
    let first = lane.points.partition_point(|point| point.tick < from);
    let near = lane.points[first..]
        .iter()
        .take_while(|point| point.tick <= to);
    let distances = near.enumerate().map(|(offset, point)| {
        let (px, py) = place(viewport, range, point);
        (first + offset, (px - x).hypot(py - y))
    });
    let reached = distances.filter(|(_, distance)| *distance <= POINT_REACH);
    reached
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// Where the dot of a point is in its lane.
pub fn place(viewport: &Viewport, range: ValueRange, point: &Point<AutomationValue>) -> (f32, f32) {
    let y = LANE_BOX.y_of(range.position(point.value.0));
    (viewport.x_of(point.tick), y)
}

/// `origin` with a point at `tick`, where a press on no point adds one, and where it is. One
/// that was at that tick takes the value.
pub fn with_point(
    origin: &AutomationLane,
    tick: Ticks,
    value: AutomationValue,
) -> (AutomationLane, usize) {
    let mut lane = origin.clone();
    let at = lane.points.partition_point(|point| point.tick < tick);
    let added = Point { tick, value };
    match lane.points.get(at) {
        Some(point) if point.tick == tick => lane.points[at] = added,
        _ => lane.points.insert(at, added),
    }
    (lane, at)
}

/// `origin` with its point at `index` moved to `tick` and `value`. It stays between its
/// neighbours: a point does not pass another, so the order of the line is the order of the
/// drag.
pub fn moved_point(
    origin: &AutomationLane,
    index: usize,
    tick: Ticks,
    value: AutomationValue,
) -> AutomationLane {
    let mut lane = origin.clone();
    let earliest = index
        .checked_sub(1)
        .map_or(Ticks(0), |before| Ticks(lane.points[before].tick.0 + 1));
    let latest = lane.points.get(index + 1).map_or(Ticks(u64::MAX), |after| {
        Ticks(after.tick.0.saturating_sub(1))
    });
    if let Some(point) = lane.points.get_mut(index) {
        *point = Point {
            tick: tick.clamp(earliest, latest.max(earliest)),
            value,
        };
    }
    lane
}

/// `origin` without its point at `index`. `None` when it was the last: the lane goes.
pub fn without_point(origin: &AutomationLane, index: usize) -> Option<AutomationLane> {
    let tick = origin.points.get(index)?.tick;
    erased(origin, &(tick..=tick))
}

/// What a lane is called in its header: the volume and the pan of the track by those words,
/// and a number of a device by the name of the device and its field, the unit left out, as
/// `Filter · Cutoff` for `cutoff_hz`. The unit is what the knob shows.
pub fn lane_name(device: Option<&str>, field: &str) -> String {
    match (device, field) {
        (None, "gain_db") => "Volume".to_string(),
        (None, field) => number_name(field),
        (Some(device), field) => format!("{device} · {}", number_name(field)),
    }
}

/// A field in plain words: `lfo_rate_hz` is LFO rate, and the number of an object in a list
/// counts from one, so `bands[0].gain_db` is Band 1 gain.
pub fn number_name(field: &str) -> String {
    let unit = UNITS.iter().find(|unit| field.ends_with(*unit));
    let field = unit.map_or(field, |unit| &field[..field.len() - unit.len()]);
    let mut words = Vec::new();
    for part in field.split('.') {
        let (object, index) = match part.split_once('[') {
            Some((object, index)) => (object, index.trim_end_matches(']').parse::<usize>().ok()),
            None => (part, None),
        };
        let object = match index {
            // One of a list of them.
            Some(_) => object.strip_suffix('s').unwrap_or(object),
            None => object,
        };
        words.extend(
            object
                .split('_')
                .filter(|word| !word.is_empty())
                .map(|word| match word {
                    "lfo" => "LFO".to_string(),
                    "q" => "Q".to_string(),
                    word => word.to_string(),
                }),
        );
        words.extend(index.map(|index| (index + 1).to_string()));
    }
    let name = words.join(" ");
    let mut letters = name.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => name,
    }
}

/// The value of the item of a number in the select that adds a lane: `filter/cutoff_hz`, and
/// `/gain_db` for the volume of the track. A device name has no `/`.
pub fn menu_value(device: Option<&str>, field: &str) -> String {
    format!("{}/{field}", device.unwrap_or_default())
}

/// The device and the field of an item of that select.
pub fn from_menu_value(value: &str) -> Option<(Option<&str>, &str)> {
    let (device, field) = value.split_once('/')?;
    Some(((!device.is_empty()).then_some(device), field))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(points: &[(u64, f32)]) -> AutomationLane {
        AutomationLane {
            device: Some("filter".to_string()),
            parameter: "cutoff_hz".to_string(),
            points: points
                .iter()
                .map(|&(tick, value)| Point {
                    tick: Ticks(tick),
                    value: AutomationValue(value),
                })
                .collect(),
        }
    }

    fn values(lane: &AutomationLane) -> Vec<(u64, f32)> {
        let points = lane.points.iter();
        points.map(|point| (point.tick.0, point.value.0)).collect()
    }

    const CUTOFF: ValueRange = ValueRange::logarithmic(20., 20000.);

    #[test]
    fn a_height_is_a_place_on_the_travel_and_back() {
        assert_eq!(LANE_BOX.y_of(0.0), LANE_HEIGHT - LANE_AIR);
        assert_eq!(LANE_BOX.y_of(1.0), LANE_AIR);
        assert_eq!(LANE_BOX.share_at(-100.0), 1.0);
        assert_eq!(LANE_BOX.share_at(500.0), 0.0);
        for position in [0.0, 0.25, 0.5, 1.0] {
            assert!((LANE_BOX.share_at(LANE_BOX.y_of(position)) - position).abs() < 1e-6);
        }
    }

    /// A press finds the dot under it, the nearest of two, and nothing past its reach.
    #[test]
    fn a_press_finds_the_dot_under_it() {
        let viewport = Viewport::default();
        let sweep = lane(&[(0, 20.), (960, 20000.), (1000, 20.)]);
        let dot = |index: usize| place(&viewport, CUTOFF, &sweep.points[index]);
        let (x, y) = dot(1);
        assert_eq!(
            point_at(&viewport, &sweep, CUTOFF, (x + 2., y - 2.)),
            Some(1)
        );
        let (x, y) = dot(2);
        assert_eq!(point_at(&viewport, &sweep, CUTOFF, (x, y + 3.)), Some(2));
        let far = (x, y - POINT_REACH - 1.);
        assert_eq!(point_at(&viewport, &sweep, CUTOFF, far), None);
    }

    /// A point is added in its place, or takes the value of one at its tick, and a moved point
    /// stays between its neighbours.
    #[test]
    fn points_are_added_and_moved_in_order() {
        let origin = lane(&[(0, 200.), (1000, 400.), (2000, 800.)]);
        let (added, at) = with_point(&origin, Ticks(1500), AutomationValue(600.));
        assert_eq!(at, 2);
        assert_eq!(
            values(&added),
            [(0, 200.), (1000, 400.), (1500, 600.), (2000, 800.)]
        );
        let (replaced, at) = with_point(&origin, Ticks(1000), AutomationValue(300.));
        assert_eq!((at, values(&replaced)[1]), (1, (1000, 300.)));

        let moved = moved_point(&origin, 1, Ticks(1200), AutomationValue(500.));
        assert_eq!(values(&moved), [(0, 200.), (1200, 500.), (2000, 800.)]);
        let past = moved_point(&origin, 1, Ticks(5000), AutomationValue(500.));
        assert_eq!(values(&past)[1], (1999, 500.));
        let before = moved_point(&origin, 1, Ticks(0), AutomationValue(500.));
        assert_eq!(values(&before)[1], (1, 500.));
    }

    /// An erase takes the points it covers, and a lane with none left is gone, as a delete of
    /// its last point is.
    #[test]
    fn an_erase_takes_points_and_the_last_takes_the_lane() {
        let origin = lane(&[(0, 200.), (1000, 400.), (2000, 800.)]);
        let erased_lane = erased(&origin, &(Ticks(500)..=Ticks(1000)));
        assert_eq!(values(&erased_lane.unwrap()), [(0, 200.), (2000, 800.)]);
        assert_eq!(erased(&origin, &(Ticks(0)..=Ticks(2000))), None);
        let one = without_point(&origin, 1).unwrap();
        assert_eq!(values(&one), [(0, 200.), (2000, 800.)]);
        assert_eq!(without_point(&lane(&[(0, 200.)]), 0), None);
    }

    /// The line spans what shows, level outside the points, and turns at each point between.
    #[test]
    fn the_line_spans_what_shows() {
        let viewport = Viewport::default();
        let sweep = lane(&[(1000, 20.), (2000, 20000.), (9000, 20.)]);
        let line = line(&viewport, &sweep, CUTOFF, Ticks(0)..Ticks(4000));
        let x = |tick: u64| viewport.x_of(Ticks(tick));
        let y = |position: f32| LANE_BOX.y_of(position);
        // The point at 9000 is past what shows: it only places the right edge.
        let right = y(1.0 - 2000. / 7000.);
        assert_eq!(line.len(), 4);
        assert_eq!(
            &line[..3],
            [(x(0), y(0.0)), (x(1000), y(0.0)), (x(2000), y(1.0))]
        );
        assert_eq!(line[3].0, x(4000));
        assert!((line[3].1 - right).abs() < 1e-3, "{line:?}");
        // Between two points, the edges are on the line between them.
        let inside = super::line(&viewport, &sweep, CUTOFF, Ticks(1250)..Ticks(1750));
        assert_eq!(inside.len(), 2);
        assert!((inside[0].1 - y(0.25)).abs() < 1e-3, "{inside:?}");
        assert!((inside[1].1 - y(0.75)).abs() < 1e-3, "{inside:?}");
        assert!(super::line(&viewport, &lane(&[]), CUTOFF, Ticks(0)..Ticks(10)).is_empty());
    }

    #[test]
    fn a_lane_is_named_in_plain_words() {
        assert_eq!(lane_name(None, "gain_db"), "Volume");
        assert_eq!(lane_name(None, "pan"), "Pan");
        assert_eq!(lane_name(Some("Filter"), "cutoff_hz"), "Filter · Cutoff");
        assert_eq!(
            lane_name(Some("Filter"), "lfo_rate_hz"),
            "Filter · LFO rate"
        );
        assert_eq!(lane_name(Some("Synth"), "decay_seconds"), "Synth · Decay");
        assert_eq!(
            lane_name(Some("EQ"), "bands[0].gain_db"),
            "EQ · Band 1 gain"
        );
        assert_eq!(lane_name(Some("EQ"), "bands[3].q"), "EQ · Band 4 Q");
        assert_eq!(
            lane_name(Some("Wavetable"), "filter.cutoff_hz"),
            "Wavetable · Filter cutoff"
        );
    }

    #[test]
    fn a_menu_value_names_its_device_and_number() {
        for (device, field) in [(None, "gain_db"), (Some("filter"), "cutoff_hz")] {
            let value = menu_value(device, field);
            assert_eq!(from_menu_value(&value), Some((device, field)));
        }
        assert_eq!(menu_value(None, "pan"), "/pan");
        assert_eq!(from_menu_value("pan"), None);
    }
}
