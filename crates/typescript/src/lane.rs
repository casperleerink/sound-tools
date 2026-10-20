//! The `lane`: a child of a parent of the project that moves a number over the song, out of its
//! output `value`. Its parent connects that to a number of another child, such as the cutoff of
//! a filter, and the number follows the line.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, EventOutput, FADER_TOP_DB, JsonTool, JsonToolDoc,
    OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, State, ToolSchema,
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
    points: Vec<Point<f32>>,
    /// How the number moves between two points: `"linear"` in a straight line, `"log"` by equal
    /// ratios in equal times, as a frequency or a time sounds even, `"fader"` along a fader's
    /// travel, as a volume in decibels moves. A log lane's values are above 0.
    #[serde(default)]
    scale: Scale,
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
            when: "A parent of this project moves a number over the song: automation of a knob"
                .to_string(),
            markdown: include_str!("lane.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
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
    check_order("state.points", &record.points)?;
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

fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let record = record(state).map_err(BehaviourError::Other)?;
    let node = context.processor(PROCESSOR, LanePlayer::default)?;
    let points: Vec<Point<f32>> = (record.points.iter())
        .map(|point| Point {
            tick: point.tick,
            value: record.scale.position(point.value),
        })
        .collect();
    let bits: Vec<(u64, u32)> = (points.iter())
        .map(|point| (point.tick.0, point.value.to_bits()))
        .collect();
    if context.changed("line", key(&(record.scale, bits))) {
        context.update(node, Arc::new((record.scale, points)))?;
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
    line: Arc<(Scale, Vec<Point<f32>>)>,
}

impl LanePlayer {
    pub(crate) const OUTPUT: EventOutput<Number> = EventOutput::new(0);
}

impl Processor for LanePlayer {
    type Update = Arc<(Scale, Vec<Point<f32>>)>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Self::Update) {
        // The old line rides back to the control thread inside the update.
        std::mem::swap(&mut self.line, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let (scale, points) = &*self.line;
        let (pieces, count) = sound_notes::pieces([points.as_slice()], &context.transport);
        let clock = context.transport.clock;
        for (offset, frame) in pieces.into_iter().take(count) {
            if let Some(position) = sound_notes::value_at_frame(points, frame, clock) {
                let value = Number(scale.value(position));
                context.event_outputs.push(Self::OUTPUT, offset, value);
            }
        }
    }
}
