//! The `lane`: a child of a parent of the project that moves a number over the song, out of its
//! output `value`. Its parent connects that to a number of another child, such as the cutoff of
//! a filter, and the number follows the line.

use std::collections::BTreeSet;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, EventOutput, FADER_TOP_DB, JsonTool, JsonToolDoc,
    OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, State, Ticks, ToolSchema,
    ValueRange,
};
use sound_notes::{LaneValue, Point, check_order};
use sound_signals::Number;

use crate::tools::key;

pub(crate) const NAME: &str = "lane";
/// Its output, the number as it moves.
pub(crate) const VALUE: &str = "value";
pub(crate) const PROCESSOR: &str = "player";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// Points in ticks from the start of the song, in tick order, one per tick, at least one,
    /// in the units of the number it moves. Before the first point it holds the first value,
    /// after the last the last.
    points: Vec<LanePoint>,
    /// How the number moves between two points: `"linear"` in a straight line, `"log"` by equal
    /// ratios in equal times, as a frequency or a time sounds even, `"fader"` along a fader's
    /// travel, as a volume in decibels moves. A log lane's values are above 0.
    #[serde(default)]
    scale: Scale,
}

/// One point of a lane, and how the line bends from it to the next.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Point")]
struct LanePoint {
    tick: Ticks,
    value: f32,
    /// How the line bends from this point to the next, -1 to 1: 0 is straight, above 0 starts
    /// slow and ends fast, below 0 starts fast and ends slow.
    #[serde(default, skip_serializing_if = "is_straight")]
    curve: f32,
}

fn is_straight(curve: &f32) -> bool {
    *curve == 0.0
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scale {
    #[default]
    Linear,
    Log,
    Fader,
}

impl Scale {
    /// Where `value` is on the line the lane moves along.
    fn position(self, value: f32) -> f32 {
        match self {
            Self::Linear => value,
            Self::Log => value.ln(),
            Self::Fader => {
                sound_core::Scale::Fader.position(f32::NEG_INFINITY, FADER_TOP_DB, value)
            }
        }
    }

    /// The value at a place on that line.
    fn value(self, position: f32) -> f32 {
        match self {
            Self::Linear => position,
            Self::Log => position.exp(),
            Self::Fader => {
                sound_core::Scale::Fader.value(f32::NEG_INFINITY, FADER_TOP_DB, position)
            }
        }
    }
}

impl State for Record {
    const TOOL: &'static str = NAME;
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: NAME.to_string(),
        check: Arc::new(|state| record(state).map(drop)),
        doc: Some(JsonToolDoc {
            group: crate::BLOCKS,
            when: "Move a number over the song: automation of a knob".to_string(),
            markdown: include_str!("lane.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
        required_children: Vec::new(),
        end: None,
    }
}

/// The record of the lane, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        outputs: &[VALUE],
        ..ToolSchema::of::<Record>(&[])
    }
}

/// The record of `state`, or what is wrong with it, by its path.
fn record(state: &Value) -> Result<Record, String> {
    let record: Record = sound_core::parse_state(state)?;
    if record.points.is_empty() {
        return Err("state.points must hold at least one point".to_string());
    }
    check_order("state.points", &line(&record.points))?;
    let mut curves = record.points.iter().enumerate();
    if let Some((index, point)) = curves.find(|(_, point)| !(-1.0..=1.0).contains(&point.curve)) {
        return Err(format!(
            "state.points[{index}].curve must be from -1 to 1, not {}",
            point.curve
        ));
    }
    let lowest = match record.scale {
        Scale::Linear | Scale::Fader => f32::NEG_INFINITY,
        Scale::Log => 0.0,
    };
    let mut points = record.points.iter().enumerate();
    if let Some((index, point)) =
        points.find(|(_, point)| !point.value.is_finite() || point.value <= lowest)
    {
        let rule = match record.scale {
            Scale::Linear | Scale::Fader => "a number",
            Scale::Log => "a number above 0 in a lane of scale \"log\"",
        };
        return Err(format!(
            "state.points[{index}].value must be {rule}, not {}",
            point.value
        ));
    }
    Ok(record)
}

/// How long a line takes to go back to what a lane had, after a hand that wrote into it lets go:
/// a 1/32 note.
const RETURN: Ticks = Ticks(120);

/// The lane of `state` with what a hand played written into it: `runs` of samples of rising
/// ticks, each in the units of the number, spliced in turn into the line as the runs before
/// left it. Each run keeps the line up to the tick before it, holds its last value, and goes
/// back to the line that was there within [`RETURN`]. `travel` is the travel of the number, on which a
/// sample goes that the line passes close to.
pub(crate) fn written(
    state: &Value,
    runs: &[Vec<(Ticks, f32)>],
    travel: ValueRange,
) -> Result<Value, String> {
    let mut lane = record(state)?;
    for run in runs {
        lane.points = spliced(&lane, run, travel);
    }
    serde_json::to_value(&lane).map_err(|error| error.to_string())
}

fn spliced(record: &Record, run: &[(Ticks, f32)], travel: ValueRange) -> Vec<LanePoint> {
    let (Some(&(first, _)), Some(&(last, held))) = (run.first(), run.last()) else {
        return record.points.clone();
    };
    // A run of one tick still holds its value for one.
    let last = if last == first {
        Ticks(first.0 + 1)
    } else {
        last
    };
    let end = Ticks(last.0 + RETURN.0);
    let at = |tick: Ticks| record.value_at(tick);
    let point = |tick: Ticks, value: f32| LanePoint {
        tick,
        value,
        curve: 0.0,
    };
    let places: Vec<Point<f32>> = (run.iter())
        .map(|&(tick, value)| Point {
            tick,
            value: travel.position(value),
        })
        .collect();
    let kept: BTreeSet<Ticks> = (sound_notes::thinned_within(&places, TOLERANCE).iter())
        .map(|point| point.tick)
        .collect();
    let played = (run.iter())
        .filter(|(tick, _)| *tick < last && kept.contains(tick))
        .map(|&(tick, value)| point(tick, value));
    // The line as it was up to the tick before the hand took it.
    let edge = first.0.checked_sub(1).map(Ticks);
    let before = (record.points.iter()).filter(|point| edge.is_some_and(|edge| point.tick < edge));
    let guard = edge.map(|tick| point(tick, at(tick)));
    let after = record.points.iter().filter(|point| point.tick > end);
    let back = LanePoint {
        curve: record.curve_at(end),
        ..point(end, at(end))
    };
    (before.copied())
        .chain(guard)
        .chain(played)
        .chain([point(last, held), back])
        .chain(after.copied())
        .collect()
}

/// How far, on the travel of its number from 0 to 1, a written line may pass from a sample it
/// drops.
const TOLERANCE: f32 = 0.005;

impl Record {
    /// The value at `tick`, as the player plays it.
    fn value_at(&self, tick: Ticks) -> f32 {
        let after = self.points.partition_point(|point| point.tick <= tick);
        let before = after
            .checked_sub(1)
            .and_then(|index| self.points.get(index));
        match (before, self.points.get(after)) {
            (Some(before), Some(next)) => {
                let done = (tick.0 - before.tick.0) as f64 / (next.tick.0 - before.tick.0) as f64;
                let done = sound_notes::bent(done, before.curve);
                let (from, to) = (
                    self.scale.position(before.value),
                    self.scale.position(next.value),
                );
                self.scale.value(f32::between(from, to, done))
            }
            (Some(only), None) | (None, Some(only)) => only.value,
            (None, None) => 0.0,
        }
    }

    /// The curve of the line at `tick`.
    fn curve_at(&self, tick: Ticks) -> f32 {
        let after = self.points.partition_point(|point| point.tick <= tick);
        let before = after
            .checked_sub(1)
            .and_then(|index| self.points.get(index));
        before.map_or(0.0, |point| point.curve)
    }
}

/// The points without their curves.
fn line(points: &[LanePoint]) -> Vec<Point<f32>> {
    let point = |point: &LanePoint| Point {
        tick: point.tick,
        value: point.value,
    };
    points.iter().map(point).collect()
}

fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let record = record(state).map_err(BehaviourError::Other)?;
    let node = context.processor(PROCESSOR, LanePlayer::default)?;
    let mut points = line(&record.points);
    for point in &mut points {
        point.value = record.scale.position(point.value);
    }
    let curves: Vec<f32> = record.points.iter().map(|point| point.curve).collect();
    let bits: Vec<(u64, u32, u32)> = (points.iter().zip(&record.points))
        .map(|(point, saved)| (point.tick.0, point.value.to_bits(), saved.curve.to_bits()))
        .collect();
    if context.changed("line", key(&(record.scale, bits))) {
        let line = Arc::new(Line {
            scale: record.scale,
            points,
            curves,
        });
        context.update(node, LaneUpdate::Line(line))?;
    }
    context.output(VALUE, OutputEndpoint::new(node, LanePlayer::OUTPUT));
    Ok(())
}

/// Plays one lane: every block, its value where the block ends, at offset 0, also while the
/// song stands still; where the line bends inside the block, again at that frame with its value
/// where the next piece ends, so a fast ramp starts and ends on its frame. Every block and not
/// only when it moved: a number that hears nothing goes back to its record. While a hand holds
/// the number, it sends nothing, so the number follows the hand.
#[derive(Default)]
pub(crate) struct LanePlayer {
    line: Arc<Line>,
    held: bool,
}

pub(crate) enum LaneUpdate {
    Line(Arc<Line>),
    /// A hand holds the number it moves, or lets go of it.
    Held(bool),
}

/// What a lane plays: its points as places on its scale, and the curve out of each.
#[derive(Default)]
pub(crate) struct Line {
    scale: Scale,
    points: Vec<Point<f32>>,
    curves: Vec<f32>,
}

impl LanePlayer {
    pub(crate) const OUTPUT: EventOutput<Number> = EventOutput::new(0);
}

impl Processor for LanePlayer {
    type Update = LaneUpdate;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Self::Update) {
        match update {
            // The old line rides back to the control thread inside the update.
            LaneUpdate::Line(line) => std::mem::swap(&mut self.line, line),
            LaneUpdate::Held(held) => self.held = *held,
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if self.held {
            return;
        }
        let Line {
            scale,
            points,
            curves,
        } = &*self.line;
        let (pieces, count) = sound_notes::pieces([points.as_slice()], &context.transport);
        let clock = context.transport.clock;
        for (offset, frame) in pieces.into_iter().take(count) {
            if let Some(position) = sound_notes::value_at_frame(points, curves, frame, clock) {
                let value = Number(scale.value(position));
                context.event_outputs.push(Self::OUTPUT, offset, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sound_core::{Ticks, ValueRange};

    use super::written;

    fn points(state: &serde_json::Value) -> Vec<(u64, f64)> {
        let points = state["points"].as_array().cloned().unwrap_or_default();
        let point = |point: &serde_json::Value| {
            let tick = point["tick"].as_u64().unwrap_or_default();
            (tick, point["value"].as_f64().unwrap_or_default())
        };
        points.iter().map(point).collect()
    }

    fn run(samples: &[(u64, f32)]) -> Vec<(Ticks, f32)> {
        samples
            .iter()
            .map(|&(tick, value)| (Ticks(tick), value))
            .collect()
    }

    /// A hand that holds 0.8 from tick 400 to 500 on a line from 0 to 1 over 1000 ticks: the line
    /// before it and after its return stays where it was, and the still hand is two points.
    #[test]
    fn a_run_goes_between_the_line_before_it_and_the_line_after_it() {
        let lane = json!({"points": [{"tick": 0, "value": 0}, {"tick": 1000, "value": 1}]});
        let still: Vec<(u64, f32)> = (400..=500).step_by(10).map(|tick| (tick, 0.8)).collect();
        let travel = ValueRange::linear(0., 1.);
        let state = written(&lane, &[run(&still)], travel).unwrap();
        let expected = [
            (0, 0.),
            (399, 0.399),
            (400, 0.8),
            (500, 0.8),
            (620, 0.62),
            (1000, 1.),
        ];
        let points = points(&state);
        assert_eq!(points.len(), expected.len(), "{points:?}");
        for ((tick, value), (want_tick, want)) in points.iter().zip(expected) {
            assert_eq!(*tick, want_tick, "{points:?}");
            assert!((value - want).abs() < 1e-3, "{points:?}");
        }
    }

    /// Two passes of a loop over the same ticks: the later one wins, its edge holds the first
    /// pass's value, and it goes back to what the first pass left.
    #[test]
    fn a_later_pass_of_a_loop_writes_over_an_earlier_one() {
        let lane = json!({"points": [{"tick": 0, "value": 0}]});
        let travel = ValueRange::linear(0., 1.);
        let first = run(&[(100, 0.5), (300, 0.5)]);
        let second = run(&[(200, 0.9), (250, 0.9)]);
        let state = written(&lane, &[first, second], travel).unwrap();
        let points = points(&state);
        let expected = [
            (0, 0.),
            (99, 0.),
            (100, 0.5),
            (199, 0.5),
            (200, 0.9),
            (250, 0.9),
        ];
        for ((tick, value), (want_tick, want)) in points.iter().zip(expected) {
            assert_eq!(*tick, want_tick, "{points:?}");
            assert!((value - want).abs() < 1e-6, "{points:?}");
        }
        // It goes back to the line the first pass left, which is on its own way back by then.
        let back: Vec<_> = points.iter().skip(expected.len()).copied().collect();
        let on_return = 0.5 * (1. - 70. / 120.);
        assert_eq!(back.len(), 2, "{points:?}");
        assert_eq!((back[0].0, back[1]), (370, (420, 0.)), "{points:?}");
        assert!((back[0].1 - on_return).abs() < 1e-6, "{points:?}");
    }
}
