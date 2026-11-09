//! The `lane`: a child of a parent of the project that moves a number over the song, out of its
//! output `value`. Its parent connects that to a number of another child, such as the cutoff of
//! a filter, and the number follows the line.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, EventOutput, FADER_TOP_DB, JsonTool, JsonToolDoc,
    OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, State, Ticks, ToolSchema,
};
use sound_notes::{Point, check_order};
use sound_signals::Number;

use crate::tools::key;

pub(crate) const NAME: &str = "lane";
/// Its output, the number as it moves.
pub(crate) const VALUE: &str = "value";
const PROCESSOR: &str = "player";

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
        context.update(
            node,
            Arc::new(Line {
                scale: record.scale,
                points,
                curves,
            }),
        )?;
    }
    context.output(VALUE, OutputEndpoint::new(node, LanePlayer::OUTPUT));
    Ok(())
}

/// Plays one lane: every block, its value where the block ends, at offset 0, also while the
/// song stands still; where the line bends inside the block, again at that frame with its value
/// where the next piece ends, so a fast ramp starts and ends on its frame. Every block and not
/// only when it moved: a number that hears nothing goes back to its record.
#[derive(Default)]
pub(crate) struct LanePlayer {
    line: Arc<Line>,
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
    type Update = Arc<Line>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Self::Update) {
        // The old line rides back to the control thread inside the update.
        std::mem::swap(&mut self.line, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
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
