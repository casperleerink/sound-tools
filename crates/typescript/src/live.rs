//! Live values: numbers of a tree the window works out again on every frame, so what moves on
//! a card or a page moves without Bun. Both windows work them out here, each from sources of
//! its own, see [`Leaves`].

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;
use sound_core::{Clock, InstanceId, Project, Ticks, ValueRange};
use sound_ui::components::knob::{
    decibels_readout, hertz_readout, milliseconds_readout, pan_readout, percent_readout,
    seconds_readout, semitones_readout, short,
};

use crate::feeds::Feeds;
pub use crate::tools::Unit;
use crate::tools::{Input, ToolInfo};
use crate::tree::named;
pub use crate::tree::{DragPart, Ease, Live, Num, Readout, SpacePart, TickReadout};

/// How close an ease comes before it is there, or a smooth one would move for a minute.
const SETTLED: f64 = 1e-6;

/// What a live value reads: the values that read no other, and what eases keep.
pub trait Leaves {
    /// The number of a live value that reads no other, such as the playhead or a peak.
    fn leaf(&self, live: &Live) -> f64;
    /// `value` eased as `ease` says, under `key`: once a frame, the same wherever it is drawn.
    fn ease(&self, key: &str, ease: Ease, value: f64) -> f64;
}

/// The number `number` is now.
pub fn number(number: &Num, leaves: &(impl Leaves + ?Sized)) -> f64 {
    match number {
        Num::Fixed(value) => f64::from(*value),
        Num::Live(live) => evaluate(live, leaves),
    }
}

/// The number `live` is now.
pub fn evaluate(live: &Live, leaves: &(impl Leaves + ?Sized)) -> f64 {
    let both = |a: &Num, b: &Num| (number(a, leaves), number(b, leaves));
    let truth = |holds: bool| f64::from(u8::from(holds));
    match live {
        Live::Plus { a, b } => number(a, leaves) + number(b, leaves),
        Live::Minus { a, b } => number(a, leaves) - number(b, leaves),
        Live::Times { a, b } => number(a, leaves) * number(b, leaves),
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
        Live::Negate { x } => -number(x, leaves),
        Live::Abs { x } => number(x, leaves).abs(),
        Live::Floor { x } => number(x, leaves).floor(),
        Live::Round { x } => number(x, leaves).round(),
        Live::Sin { x } => number(x, leaves).sin(),
        Live::Cos { x } => number(x, leaves).cos(),
        Live::Decibels { x } => (20.0 * number(x, leaves).abs().log10()).max(-120.0),
        Live::Ease { x, ease, key } => leaves.ease(key, *ease, number(x, leaves)),
        Live::Tick
        | Live::Bpm
        | Live::Elapsed
        | Live::Value { .. }
        | Live::Held { .. }
        | Live::Peak { .. }
        | Live::DevicePeak { .. }
        | Live::Input { .. }
        | Live::Drag { .. }
        | Live::Space { .. }
        | Live::Hover { .. }
        | Live::Touched { .. } => leaves.leaf(live),
    }
}

/// What an ease keeps from frame to frame.
#[derive(Clone, Copy, Debug)]
pub struct Eased {
    /// The frame it last moved in.
    pub frame: u64,
    pub value: f64,
    /// How long a hold has held.
    held: f64,
}

impl Eased {
    pub fn new(frame: u64, value: f64) -> Self {
        Self {
            frame,
            value,
            held: 0.0,
        }
    }

    /// One frame `dt` seconds after the last, toward `value`. Whether it still moves.
    pub fn step(&mut self, ease: Ease, value: f64, dt: f64) -> bool {
        let before = self.value;
        match ease {
            Ease::Smooth { ms } => {
                let seconds = f64::from(ms.max(1.0)) / 1000.0;
                self.value += (value - self.value) * (1.0 - (-dt / seconds).exp());
            }
            Ease::Fall { per_second } => {
                self.value = value.max(self.value - f64::from(per_second) * dt);
            }
            Ease::Hold { seconds } => {
                self.held += dt;
                if value >= self.value || self.held > f64::from(seconds) {
                    self.value = value;
                    self.held = 0.0;
                }
            }
        }
        if !self.value.is_finite() || (value - self.value).abs() < SETTLED {
            self.value = value;
        }
        self.value != before || self.value != value
    }
}

/// A field of `instance` as it plays, by its name as `now.value` gives it (of an instance
/// inside it, `path.field`): what a lane plays into it at `tick`, else its record.
pub fn field_now(
    project: &Project,
    instance: &InstanceId,
    child: Option<&str>,
    name: &str,
    tick: Ticks,
) -> f64 {
    let id = match child {
        Some(path) => instance.child(path).ok(),
        None => Some(instance.clone()),
    };
    let Some(id) = id else {
        return 0.0;
    };
    let at = crate::feeds::Moment::at(tick);
    if let Some((value, _)) = crate::feeds::lane_value(project, &id, name, at) {
        return f64::from(value);
    }
    let state = project.state_json(&id);
    let state = state.and_then(|state| serde_json::from_str::<Value>(&state).ok());
    let value =
        state.and_then(|state| crate::path::at(&state, name).and_then(crate::tree::number_of));
    value.map_or(0.0, f64::from)
}

/// Where a drag is, in the units of its space, as `now.drag(id)` reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DragNumbers {
    pub start_x: f64,
    pub start_y: f64,
    pub dx: f64,
    pub dy: f64,
    pub active: bool,
    pub alt: bool,
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
            DragPart::Alt => f64::from(u8::from(self.alt)),
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
    /// Whether its `fit` was applied, once it was laid out.
    pub fitted: bool,
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
            fitted: false,
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
    /// The record, with each field it leaves out at its default.
    pub state: &'a Value,
    pub info: Option<&'a ToolInfo>,
    /// Where each live control was put last.
    pub played: Option<&'a BTreeMap<String, f32>>,
    pub drag: Option<(&'a str, DragNumbers)>,
    pub spaces: &'a HashMap<String, SpaceView>,
    /// The key of the shape the pointer is over.
    pub hovered: Option<&'a str>,
    /// The controls the pointer or the keys are on, or a drag holds, as `Live::Touched` names
    /// them.
    pub touched: &'a [String],
    pub feeds: &'a Feeds,
}

impl Sources<'_> {
    pub(crate) fn number(&self, value: &Num) -> f64 {
        number(value, self)
    }

    /// What a field of the record holds now.
    pub(crate) fn field(&self, name: &str) -> Option<f32> {
        crate::path::at(self.state, name).and_then(crate::tree::number_of)
    }

    /// Where a number input was put last, else where it starts.
    pub(crate) fn input(&self, name: &str) -> Option<f32> {
        let played = self.played.and_then(|played| played.get(name));
        let (_, input) = self.info?.input(name)?;
        played.copied().or(input.start())
    }

    /// The number `name` of the instance: a field as it plays, else a number input, else an
    /// output.
    fn own(&self, name: &str, fraction: bool) -> f64 {
        let along = |value: f32, range: Option<ValueRange>| {
            let range = range.filter(|_| fraction);
            f64::from(range.map_or(value, |range| range.position(value)))
        };
        let field = self.info.map_or(self.field(name).is_some(), |info| {
            info.field_at(name).is_some()
        });
        if field {
            let played = self.feeds.played.get(name).map(|(played, _)| *played);
            let value = played.unwrap_or_else(|| self.field(name).unwrap_or(0.0));
            return along(value, field_range(self.info, name));
        }
        if let Some(value) = self.input(name) {
            return along(value, input_range(self.info, name));
        }
        let output = self.feeds.played.get(name);
        output.map_or(0.0, |(value, _)| f64::from(*value))
    }
}

impl Leaves for Sources<'_> {
    fn leaf(&self, live: &Live) -> f64 {
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
            Live::Bpm => (self.feeds.clock.as_ref()).map_or(0.0, |clock| {
                let tick = sound_core::Ticks(self.tick.max(0.0) as u64);
                clock.tempo_map().change_at(tick).bpm.bpm()
            }),
            Live::Elapsed => self.feeds.time,
            Live::Value {
                name,
                child: None,
                fraction,
            } => self.own(name, *fraction),
            Live::Value {
                name,
                child: Some(child),
                fraction,
            } => {
                let named = format!("{child}.{name}");
                let field =
                    (self.feeds.played.get(&named)).or_else(|| self.feeds.inside.get(&named));
                field.map_or(0.0, |field| along(*field, *fraction))
            }
            Live::Held { name, child } => {
                truth(self.feeds.held.contains(&named(child.as_deref(), name)))
            }
            Live::Peak {
                name,
                channel: which,
            } => {
                let peaks = self.feeds.peaks.get(name).copied().unwrap_or_default();
                channel(peaks, *which)
            }
            Live::DevicePeak { channel: which } => channel(self.feeds.output, *which),
            Live::Input { channel } => self
                .feeds
                .input
                .get(*channel)
                .copied()
                .map_or(0.0, f64::from),
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
            Live::Hover { key } => truth(self.hovered == Some(key.as_str())),
            Live::Touched { name } => {
                let touched = self.touched.contains(name);
                #[cfg(feature = "test-support")]
                crate::feeds::tell_tests_shown(
                    format!("touched:{name}"),
                    f32::from(u8::from(touched)),
                );
                truth(touched)
            }
            _ => 0.0,
        }
    }

    fn ease(&self, key: &str, ease: Ease, value: f64) -> f64 {
        self.feeds.ease(key, ease, value)
    }
}

/// How a number field turns on its knob: over its range, on a log scale for Hz.
pub(crate) fn field_range(info: Option<&ToolInfo>, name: &str) -> Option<ValueRange> {
    info?.field_at(name)?.range()
}

/// How a number input turns: over its range, on a log scale for Hz.
pub(crate) fn input_range(info: Option<&ToolInfo>, name: &str) -> Option<ValueRange> {
    match info?.input(name)? {
        (_, Input::Number { min, max, unit, .. }) => {
            Some(crate::tools::knob_range(*min, *max, *unit))
        }
        (_, Input::Bang { .. }) => None,
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

/// A number as a readout of `unit`.
fn readout(value: f32, unit: Option<Unit>) -> String {
    match unit {
        Some(Unit::Hz) => hertz_readout(value),
        Some(Unit::Ms) => milliseconds_readout(value),
        Some(Unit::Seconds) => seconds_readout(value),
        Some(Unit::Db) => decibels_readout(value),
        Some(Unit::Percent) => percent_readout(value),
        Some(Unit::Semitones) => semitones_readout(value),
        Some(Unit::Cents) => format!("{} ct", short(value)),
        Some(Unit::Octaves) => format!("{} oct", short(value)),
        Some(Unit::Pitch) => pitch_readout(f64::from(value)),
        Some(Unit::Pan) => pan_readout(value),
        None => short(value),
    }
}

/// A live number as text shows it: in its unit, or a tick as a place in the piece (`5.3`), as
/// minutes and seconds (`1:07`), or as the time signature there (`7/8`).
pub fn text_of(value: f64, shown: Option<Readout>, clock: Option<&Clock>) -> String {
    let shown = match shown {
        None => return readout(value as f32, None),
        // A pitch keeps its cents, which an `f32` of a high note would round.
        Some(Readout::Unit(Unit::Pitch)) => return pitch_readout(value),
        Some(Readout::Unit(unit)) => return readout(value as f32, Some(unit)),
        Some(Readout::Tick(shown)) => shown,
    };
    let tick = sound_core::Ticks(value.max(0.0) as u64);
    let Some(clock) = clock else {
        return String::new();
    };
    let signatures = clock.tempo_map().time_signatures();
    match shown {
        TickReadout::Position => {
            let position = signatures.bar_beat_of(tick);
            format!("{}.{}", position.bar, position.beat)
        }
        TickReadout::Time => {
            let seconds = clock.seconds_of(tick).max(0.0) as u64;
            format!("{}:{:02}", seconds / 60, seconds % 60)
        }
        TickReadout::Signature => signatures.bar_at(tick).signature.to_string(),
        TickReadout::Beats => signatures.bar_at(tick).signature.numerator().to_string(),
        TickReadout::Note => signatures.bar_at(tick).signature.denominator().to_string(),
    }
}

/// A pitch as its note, `C2` for 36, and how many cents off it is where it is not whole:
/// `D2 +30`, so two pitches tuned apart do not read the same.
fn pitch_readout(value: f64) -> String {
    let note = value.round();
    let cents = ((value - note) * 100.0).round() as i64;
    match sound_notes::Pitch::try_from(note as i64) {
        Ok(pitch) if cents == 0 => pitch.name(),
        Ok(pitch) => format!("{} {cents:+}", pitch.name()),
        Err(_) => readout(value as f32, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pitch_that_is_not_whole_reads_its_note_and_its_cents() {
        assert_eq!(pitch_readout(36.0), "C2");
        assert_eq!(pitch_readout(38.3), "D2 +30");
        assert_eq!(pitch_readout(37.6), "D2 -40");
        assert_eq!(pitch_readout(36.004), "C2");
    }

    fn live(json: serde_json::Value) -> Num {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_drag_moves_what_binds_to_it_snapped_and_nothing_bound_to_another() {
        let mut feeds = Feeds::default();
        feeds.played.insert("step".to_string(), (3.4, None));
        let spaces = HashMap::new();
        let numbers = DragNumbers {
            start_x: 960.0,
            dx: 250.0,
            active: true,
            ..DragNumbers::default()
        };
        let sources = Sources {
            tick: 0.0,
            state: &Value::Null,
            info: None,
            played: None,
            drag: Some(("move", numbers)),
            spaces: &spaces,
            hovered: None,
            touched: &[],
            feeds: &feeds,
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
            "a": { "op": "floor", "x": { "op": "value", "name": "step", "fraction": false } },
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
