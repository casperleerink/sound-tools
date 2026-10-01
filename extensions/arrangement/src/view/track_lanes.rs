//! The automation lanes under a track in the timeline: one per automated number, with its name
//! and its line in the track colour, on the travel of the knob of the number, as it plays. They
//! are drawn, erased and cleared as the expression lanes of the note editor are
//! ([`super::lanes`]), in project ticks. Pure math, no GPUI and no project.

use std::iter::once;
use std::ops::Range;

use sound_core::{Ticks, ValueRange};
use sound_notes::{Point, thinned_within, value_at};

use super::lanes::LaneEdit;
use super::layout::{LANE_HEIGHT, Viewport};
use crate::{AutomationLane, AutomationValue};

/// The air above the top of the travel and under its bottom, in a lane.
const LANE_AIR: f32 = 8.0;
/// How far a drawn point may be from the straight line through the points kept, on the travel,
/// and still be left out: far under a pixel of a lane.
const THIN_STEP: f64 = 1e-3;
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

/// The height in a lane of a place on the travel: the bottom of the travel at the bottom.
pub fn y_of(position: f32) -> f32 {
    let bottom = LANE_HEIGHT - LANE_AIR;
    bottom - (bottom - LANE_AIR) * position.clamp(0.0, 1.0)
}

/// The place on the travel at height `y` in a lane. Above the lane it is the top, below it the
/// bottom.
pub fn position_at(y: f32) -> f32 {
    let bottom = LANE_HEIGHT - LANE_AIR;
    ((bottom - y) / (bottom - LANE_AIR)).clamp(0.0, 1.0)
}

/// The points of a lane as places on the travel of its number, where its line is straight.
pub fn on_travel(lane: &AutomationLane, range: ValueRange) -> Vec<Point<f32>> {
    let points = lane.points.iter().map(|point| Point {
        tick: point.tick,
        value: range.position(point.value.0),
    });
    points.collect()
}

/// The line of a lane in the lane across what `ticks` shows, as the places where it turns:
/// from the left edge to the right edge, level before the first point and after the last.
pub fn line(viewport: &Viewport, points: &[Point<f32>], ticks: Range<Ticks>) -> Vec<(f32, f32)> {
    let (Some(start), Some(end)) = (value_at(points, ticks.start), value_at(points, ticks.end))
    else {
        return Vec::new();
    };
    let placed = |tick: Ticks, position: f32| (viewport.x_of(tick), y_of(position));
    // A lane may have many points and the timeline shows a few bars of it.
    let from = points.partition_point(|point| point.tick <= ticks.start);
    let to = points.partition_point(|point| point.tick < ticks.end);
    let shown = points.get(from..to.max(from)).unwrap_or_default();
    once(placed(ticks.start, start))
        .chain(shown.iter().map(|point| placed(point.tick, point.value)))
        .chain(once(placed(ticks.end, end)))
        .collect()
}

/// `origin`, a lane as it was at mouse down, changed by `edit`, in project ticks. A drawn
/// height becomes the value of the knob there, in the range of the number, so a value is
/// never outside it. A number the project does not know, `range` `None`, is not drawn in.
/// `None` when no point is left: the lane is gone, and the number plays its record again.
pub fn edited(
    origin: &AutomationLane,
    range: Option<ValueRange>,
    edit: &LaneEdit,
) -> Option<AutomationLane> {
    let points = match edit {
        LaneEdit::Draw(drawn) => {
            let (Some((&first, _)), Some((&last, _)), Some(range)) =
                (drawn.first_key_value(), drawn.last_key_value(), range)
            else {
                return Some(origin.clone());
            };
            let line: Vec<Point<f32>> = drawn
                .iter()
                .map(|(tick, y)| Point {
                    tick: *tick,
                    value: position_at(*y),
                })
                .collect();
            let line = thinned_within(&line, THIN_STEP, f64::from);
            let line = line.into_iter().map(|point| Point {
                tick: point.tick,
                value: AutomationValue(range.value(point.value)),
            });
            let before = origin.points.iter().filter(|point| point.tick < first);
            let after = origin.points.iter().filter(|point| point.tick > last);
            before.copied().chain(line).chain(after.copied()).collect()
        }
        LaneEdit::Erase(ticks) => {
            let kept = origin
                .points
                .iter()
                .filter(|point| !ticks.contains(&point.tick));
            kept.copied().collect()
        }
        LaneEdit::Clear => Vec::new(),
    };
    let lane = AutomationLane {
        points,
        ..origin.clone()
    };
    (!lane.points.is_empty()).then_some(lane)
}

/// The undo label of an edit of a lane.
pub fn label(edit: &LaneEdit) -> &'static str {
    match edit {
        LaneEdit::Draw(_) => "Draw automation",
        LaneEdit::Erase(_) => "Erase automation",
        LaneEdit::Clear => "Clear automation",
    }
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

/// A field in plain words: `lfo_rate_hz` is LFO rate, `filter_1.cutoff` is Filter 1 cutoff.
pub fn number_name(field: &str) -> String {
    let unit = UNITS.iter().find(|unit| field.ends_with(*unit));
    let field = unit.map_or(field, |unit| &field[..field.len() - unit.len()]);
    let words = field.split(['_', '.']).filter(|word| !word.is_empty());
    let words = words.map(|word| match word {
        "lfo" => "LFO".to_string(),
        "q" => "Q".to_string(),
        word => word.to_string(),
    });
    let name = words.collect::<Vec<_>>().join(" ");
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
    use std::collections::BTreeMap;

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
        assert_eq!(y_of(0.0), LANE_HEIGHT - LANE_AIR);
        assert_eq!(y_of(1.0), LANE_AIR);
        assert_eq!(position_at(-100.0), 1.0);
        assert_eq!(position_at(500.0), 0.0);
        for position in [0.0, 0.25, 0.5, 1.0] {
            assert!((position_at(y_of(position)) - position).abs() < 1e-6);
        }
    }

    /// A drawn line goes in as values of the knob over the points it passes, the level part of
    /// it as its two ends, and the points around it stay. The top of the lane is the top of the
    /// range, and no height gives a value outside it.
    #[test]
    fn a_draw_puts_values_of_the_knob_over_the_points_it_passes() {
        let origin = lane(&[(0, 200.), (1000, 400.), (2000, 800.), (5000, 1000.)]);
        let middle = y_of(0.5);
        let drawn: BTreeMap<Ticks, f32> = [(500, middle), (1500, middle), (2500, -50.)]
            .into_iter()
            .map(|(tick, y)| (Ticks(tick), y))
            .collect();
        let drawn_lane = edited(&origin, Some(CUTOFF), &LaneEdit::Draw(&drawn)).unwrap();
        // Halfway up a logarithmic knob from 20 Hz to 20 kHz is 632 Hz, with three digits.
        assert_eq!(
            values(&drawn_lane),
            [
                (0, 200.),
                (500, 632.),
                (1500, 632.),
                (2500, 20000.),
                (5000, 1000.)
            ]
        );
        assert_eq!(drawn_lane.device, origin.device);
        // A number the project does not know is not drawn in.
        let unknown = edited(&origin, None, &LaneEdit::Draw(&drawn));
        assert_eq!(unknown.as_ref(), Some(&origin));
    }

    /// An erase takes the points it covers, and a lane with none left is gone, as a clear is.
    #[test]
    fn an_erase_takes_points_and_the_last_takes_the_lane() {
        let origin = lane(&[(0, 200.), (1000, 400.), (2000, 800.)]);
        let erased = edited(&origin, None, &LaneEdit::Erase(Ticks(500)..=Ticks(1000)));
        assert_eq!(values(&erased.unwrap()), [(0, 200.), (2000, 800.)]);
        let all = LaneEdit::Erase(Ticks(0)..=Ticks(2000));
        assert_eq!(edited(&origin, Some(CUTOFF), &all), None);
        assert_eq!(edited(&origin, Some(CUTOFF), &LaneEdit::Clear), None);
    }

    /// The line spans what shows, level outside the points, and turns at each point between.
    #[test]
    fn the_line_spans_what_shows() {
        let viewport = Viewport::default();
        let points = on_travel(&lane(&[(1000, 20.), (2000, 20000.)]), CUTOFF);
        let line = line(&viewport, &points, Ticks(0)..Ticks(4000));
        let x = |tick: u64| viewport.x_of(Ticks(tick));
        assert_eq!(
            line,
            [
                (x(0), y_of(0.0)),
                (x(1000), y_of(0.0)),
                (x(2000), y_of(1.0)),
                (x(4000), y_of(1.0)),
            ]
        );
        // Between two points, the edges are on the line between them.
        let inside = super::line(&viewport, &points, Ticks(1250)..Ticks(1750));
        assert_eq!(inside.len(), 2);
        assert!((inside[0].1 - y_of(0.25)).abs() < 1e-3, "{inside:?}");
        assert!((inside[1].1 - y_of(0.75)).abs() < 1e-3, "{inside:?}");
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
        assert_eq!(lane_name(Some("EQ"), "q"), "EQ · Q");
        assert_eq!(
            lane_name(Some("Wavetable"), "filter_1.cutoff"),
            "Wavetable · Filter 1 cutoff"
        );
        assert_eq!(lane_name(Some("Synth"), "decay_seconds"), "Synth · Decay");
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
