//! Live values: numbers of a tree the window works out again on every frame, so what moves on
//! a card or a page moves without Bun.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use sound_core::ValueRange;

use crate::feeds::Feeds;
use crate::tools::{Control, Field, ToolInfo, knob_range};
use crate::tree::{Axis, DragPart, Live, Num, SpacePart};

/// Where a drag is, in the units of its space, as `now.drag(id)` reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DragNumbers {
    pub start_x: f64,
    pub start_y: f64,
    pub dx: f64,
    pub dy: f64,
    pub active: bool,
}

impl DragNumbers {
    fn part(&self, part: DragPart) -> f64 {
        match part {
            DragPart::StartX => self.start_x,
            DragPart::StartY => self.start_y,
            DragPart::Dx => self.dx,
            DragPart::Dy => self.dy,
            DragPart::X => self.start_x + self.dx,
            DragPart::Y => self.start_y + self.dy,
            DragPart::Active => f64::from(u8::from(self.active)),
        }
    }
}

/// What the window shows of a space: where it is scrolled to and how far zoomed, in its units,
/// and its size in points when it was last laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpaceView {
    /// The unit at its left and top edge.
    pub left: f64,
    pub top: f64,
    pub zoom_x: f64,
    pub zoom_y: f64,
    /// Points per unit at zoom 1.
    pub unit_x: f64,
    pub unit_y: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for SpaceView {
    fn default() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            zoom_x: 1.0,
            zoom_y: 1.0,
            unit_x: 1.0,
            unit_y: 1.0,
            width: 0.0,
            height: 0.0,
        }
    }
}

impl SpaceView {
    /// Points per unit, across and down.
    pub(crate) fn scale(&self) -> (f64, f64) {
        (self.unit_x * self.zoom_x, self.unit_y * self.zoom_y)
    }

    fn part(&self, part: SpacePart) -> f64 {
        let (across, down) = self.scale();
        match part {
            SpacePart::Left => self.left,
            SpacePart::Top => self.top,
            SpacePart::Right => self.left + self.width / across,
            SpacePart::Bottom => self.top + self.height / down,
            SpacePart::PointX => 1.0 / across,
            SpacePart::PointY => 1.0 / down,
        }
    }
}

/// What live values read, as the window has it now.
pub(crate) struct Sources<'a> {
    pub tick: f64,
    pub watches: &'a BTreeMap<String, f32>,
    /// The record, with each field it leaves out at its default.
    pub state: &'a Value,
    pub info: Option<&'a ToolInfo>,
    /// Where each live control was put last.
    pub played: Option<&'a BTreeMap<String, f32>>,
    pub drag: Option<(&'a str, DragNumbers)>,
    pub spaces: &'a HashMap<String, SpaceView>,
    pub feeds: &'a Feeds,
    /// Points per unit of the space it draws in now, across and down.
    pub scale: (f64, f64),
}

impl Sources<'_> {
    pub(crate) fn number(&self, number: &Num) -> f64 {
        match number {
            Num::Fixed(value) => f64::from(*value),
            Num::Live(live) => self.live(live),
        }
    }

    /// What a field of the record holds now.
    pub(crate) fn field(&self, name: &str) -> Option<f32> {
        (self.state.get(name))
            .and_then(Value::as_f64)
            .map(|value| value as f32)
    }

    /// Where a live control was put last, else its default.
    pub(crate) fn control(&self, name: &str) -> Option<f32> {
        let played = self.played.and_then(|played| played.get(name));
        match self.info?.control(name)? {
            (_, Control::Live { default, .. }) => Some(played.copied().unwrap_or(*default)),
            (_, Control::Trigger { .. }) => None,
        }
    }

    fn live(&self, live: &Live) -> f64 {
        let both = |a: &Num, b: &Num| (self.number(a), self.number(b));
        let truth = |holds: bool| f64::from(u8::from(holds));
        let along = |(value, range): (f32, Option<ValueRange>), fraction: bool| {
            let range = range.filter(|_| fraction);
            f64::from(range.map_or(value, |range| range.position(value)))
        };
        let channel = |peaks: [f32; 2], channel: Option<u8>| {
            f64::from(match channel {
                Some(0) => peaks[0],
                Some(_) => peaks[1],
                None => peaks[0].max(peaks[1]),
            })
        };
        match live {
            Live::Tick => self.tick,
            Live::Time => self.feeds.time,
            Live::Watch { name } => self.watches.get(name).copied().map_or(0.0, f64::from),
            Live::Field {
                name,
                fraction,
                child: None,
            } => {
                let value = self.field(name).unwrap_or(0.0);
                along((value, field_range(self.info, name)), *fraction)
            }
            Live::Field {
                name,
                fraction,
                child: Some(child),
            } => {
                let field = self.feeds.children.get(&format!("{child}.{name}"));
                field.map_or(0.0, |field| along(*field, *fraction))
            }
            Live::Played { name, fraction } => {
                let played = self.feeds.played.get(name);
                played.map_or(0.0, |played| along(*played, *fraction))
            }
            Live::Peak {
                name,
                channel: which,
            } => {
                let peaks = self.feeds.peaks.get(name).copied().unwrap_or_default();
                channel(peaks, *which)
            }
            Live::Output { channel: which } => channel(self.feeds.output, *which),
            Live::Points { points, axis } => {
                let scale = match axis {
                    Axis::X => self.scale.0,
                    Axis::Y => self.scale.1,
                };
                if scale > 0.0 {
                    f64::from(*points) / scale
                } else {
                    0.0
                }
            }
            Live::Control { name, fraction } => {
                let value = self.control(name).unwrap_or(0.0);
                let range = control_range(self.info, name).filter(|_| *fraction);
                f64::from(range.map_or(value, |range| range.position(value)))
            }
            Live::Drag { id, part } => match self.drag {
                Some((held, numbers)) if held == id => numbers.part(*part),
                _ => DragNumbers::default().part(*part),
            },
            Live::Space { key, part } => self
                .spaces
                .get(key)
                .copied()
                .unwrap_or_default()
                .part(*part),
            Live::Plus { a, b } => self.number(a) + self.number(b),
            Live::Minus { a, b } => self.number(a) - self.number(b),
            Live::Times { a, b } => self.number(a) * self.number(b),
            Live::Over { a, b } => {
                let (a, b) = both(a, b);
                if b == 0.0 { 0.0 } else { a / b }
            }
            Live::Min { a, b } => {
                let (a, b) = both(a, b);
                a.min(b)
            }
            Live::Max { a, b } => {
                let (a, b) = both(a, b);
                a.max(b)
            }
            Live::Lt { a, b } => {
                let (a, b) = both(a, b);
                truth(a < b)
            }
            Live::Gt { a, b } => {
                let (a, b) = both(a, b);
                truth(a > b)
            }
            Live::Le { a, b } => {
                let (a, b) = both(a, b);
                truth(a <= b)
            }
            Live::Ge { a, b } => {
                let (a, b) = both(a, b);
                truth(a >= b)
            }
            Live::Eq { a, b } => {
                let (a, b) = both(a, b);
                truth(a == b)
            }
            Live::Ne { a, b } => {
                let (a, b) = both(a, b);
                truth(a != b)
            }
            Live::Mod { a, b } => {
                let (a, b) = both(a, b);
                if b == 0.0 { 0.0 } else { a.rem_euclid(b) }
            }
            Live::Pow { a, b } => {
                let (a, b) = both(a, b);
                let power = a.powf(b);
                if power.is_finite() { power } else { 0.0 }
            }
            Live::Snap { a, b } => {
                let (value, step) = both(a, b);
                snap(value, step)
            }
            Live::Negate { x } => -self.number(x),
            Live::Abs { x } => self.number(x).abs(),
            Live::Floor { x } => self.number(x).floor(),
            Live::Round { x } => self.number(x).round(),
            Live::Sin { x } => self.number(x).sin(),
            Live::Cos { x } => self.number(x).cos(),
            Live::Decibels { x } => (20.0 * self.number(x).abs().log10()).max(-120.0),
            Live::Ease { x, ease, key } => self.feeds.ease(key, *ease, self.number(x)),
        }
    }
}

/// How a field turns on its knob: over its range, on a log scale for Hz.
pub(crate) fn field_range(info: Option<&ToolInfo>, name: &str) -> Option<ValueRange> {
    match info?.field(name)? {
        Field::Knob { min, max, unit, .. } => Some(knob_range(*min, *max, *unit)),
        Field::Number { min, max, .. } => Some(ValueRange::linear(*min as f32, *max as f32)),
        _ => None,
    }
}

/// How a live control turns: over its range.
pub(crate) fn control_range(info: Option<&ToolInfo>, name: &str) -> Option<ValueRange> {
    match info?.control(name)? {
        (_, Control::Live { min, max, .. }) => Some(ValueRange::linear(*min, *max)),
        (_, Control::Trigger { .. }) => None,
    }
}

/// The nearest whole number of `step`s; `value` itself for no step.
pub(crate) fn snap(value: f64, step: f64) -> f64 {
    if step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(json: serde_json::Value) -> Num {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_drag_moves_what_binds_to_it_snapped_and_nothing_bound_to_another() {
        let watches = BTreeMap::from([("step".to_string(), 3.4)]);
        let spaces = HashMap::new();
        let numbers = DragNumbers {
            start_x: 960.0,
            dx: 250.0,
            active: true,
            ..DragNumbers::default()
        };
        let sources = Sources {
            tick: 0.0,
            watches: &watches,
            state: &Value::Null,
            info: None,
            played: None,
            drag: Some(("move", numbers)),
            spaces: &spaces,
            feeds: &Feeds::default(),
            scale: (1.0, 1.0),
        };
        let moved = live(serde_json::json!({
            "op": "plus",
            "a": 480,
            "b": { "op": "snap", "a": { "op": "drag", "id": "move", "part": "dx" }, "b": 240 },
        }));
        assert_eq!(sources.number(&moved), 720.0);
        let other = live(serde_json::json!({ "op": "drag", "id": "resize", "part": "dx" }));
        assert_eq!(sources.number(&other), 0.0);
        let lit = live(serde_json::json!({
            "op": "eq",
            "a": { "op": "floor", "x": { "op": "watch", "name": "step" } },
            "b": 3,
        }));
        assert_eq!(sources.number(&lit), 1.0);
    }

    #[test]
    fn a_space_says_what_shows_of_it_in_its_units() {
        let view = SpaceView {
            left: 960.0,
            top: 10.0,
            zoom_x: 2.0,
            unit_x: 0.05,
            unit_y: 12.0,
            width: 400.0,
            height: 240.0,
            ..SpaceView::default()
        };
        assert_eq!(view.part(SpacePart::Right), 960.0 + 400.0 / 0.1);
        assert_eq!(view.part(SpacePart::Bottom), 30.0);
        assert_eq!(view.part(SpacePart::PointX), 10.0);
    }
}
