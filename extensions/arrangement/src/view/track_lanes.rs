//! The automation lanes under a track in the timeline: one per automated number, with its name
//! and its line in the track colour, on the travel of the knob of the number, as it plays. They
//! are drawn, erased and cleared as the expression lanes of the note editor are, with a
//! [`super::lanes::Stroke`] into a [`LaneBox`], in project ticks. Pure math, no GPUI and no
//! project.

use std::iter::once;
use std::ops::Range;

use sound_core::{Ticks, ValueRange};
use sound_notes::{thinned_within, value_at};

use super::lanes::{LaneBox, LaneEdit};
use super::layout::{LANE_HEIGHT, Viewport};
use crate::automation::{ON_THE_LINE, positions};
use crate::{AutomationLane, AutomationValue};

/// The air above the top of the travel and under its bottom, in a lane.
const LANE_AIR: f32 = 8.0;
/// Where a lane draws the travel of its knob: the bottom of the travel at the bottom.
pub const LANE_BOX: LaneBox = LaneBox {
    top: LANE_AIR,
    bottom: LANE_HEIGHT - LANE_AIR,
};
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

/// `origin`, a lane as it was at mouse down, changed by `edit`, in project ticks. A drawn
/// height becomes the value of the knob there, in the range of the number, so a value is
/// never outside it, and the line drawn is thinned on the travel. A number the project does
/// not know, `range` `None`, is not drawn in. `None` when no point is left: the lane is gone,
/// and the number plays its record again.
pub fn edited(
    origin: &AutomationLane,
    range: Option<ValueRange>,
    edit: &LaneEdit,
) -> Option<AutomationLane> {
    let points = match (edit, range) {
        (LaneEdit::Draw(_), None) => origin.points.clone(),
        (_, range) => {
            let value_at = |y: f32| {
                let value = range.map(|range| range.value(LANE_BOX.share_at(y)));
                AutomationValue(value.unwrap_or_default())
            };
            let place = |value: AutomationValue| {
                let place = range.map(|range| range.position(value.0));
                f64::from(place.unwrap_or_default())
            };
            let thin = |line: &[_]| thinned_within(line, f64::from(ON_THE_LINE), place);
            edit.applied(&origin.points, value_at, thin)
        }
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
    use std::collections::BTreeMap;

    use sound_notes::Point;

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

    /// A drawn line goes in as values of the knob over the points it passes, the level part of
    /// it as its two ends, and the points around it stay. The top of the lane is the top of the
    /// range, and no height gives a value outside it.
    #[test]
    fn a_draw_puts_values_of_the_knob_over_the_points_it_passes() {
        let origin = lane(&[(0, 200.), (1000, 400.), (2000, 800.), (5000, 1000.)]);
        let middle = LANE_BOX.y_of(0.5);
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
