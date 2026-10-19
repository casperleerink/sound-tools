//! The `fader`: a child of a parent of the project that turns a sound up or down on a fader's
//! scale, pans it and mutes it, with no click, and keeps the level of what it sends on for a
//! view. A mixer strip, a send or a master volume is one.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::mixer::{self, MAX_GAIN_DB, Mix, Mixer};
use sound_core::{
    BehaviourContext, BehaviourError, InputEndpoint, JsonTool, JsonToolDoc, OutputEndpoint, State,
    ToolSchema, decibels,
};

use crate::meter::LEVEL;

pub(crate) const NAME: &str = "fader";
const PROCESSOR: &str = "fader";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// How much louder or quieter it plays, in decibels, up to 6. `"-inf"` is silence.
    #[serde(default, with = "decibels")]
    #[schemars(with = "Gain")]
    gain_db: f32,
    /// Where it sits between the two channels: -1 hard left, 0 the middle, 1 hard right.
    #[serde(default)]
    pan: f32,
    /// Silent, with a short fade.
    #[serde(default)]
    mute: bool,
}

/// What a gain field reads: a number of decibels, or `"-inf"` for silence.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum Gain {
    Decibels(f32),
    Silence(Silence),
}

#[derive(JsonSchema)]
#[allow(dead_code)]
enum Silence {
    #[serde(rename = "-inf")]
    Silence,
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
            when: "A parent of this project turns a sound up or down, pans or mutes it: a mixer strip, a send or a master volume"
                .to_string(),
            markdown: include_str!("fader.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
        end: None,
    }
}

/// The record of the fader, for the SDK: its gain and its pan are numbers a cable moves.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        numbers: vec![
            (mixer::GAIN.info(), mixer::GAIN.default),
            (mixer::PAN.info(), mixer::PAN.default),
        ],
        ..ToolSchema::of::<Record>(&[])
    }
}

fn record(state: &Value) -> Result<Record, String> {
    let record: Record =
        serde_json::from_value(state.clone()).map_err(|error| format!("state: {error}"))?;
    decibels::check("state.gain_db", record.gain_db, MAX_GAIN_DB)?;
    if !(-1.0..=1.0).contains(&record.pan) {
        return Err(format!(
            "state.pan must be from -1 to 1, not {}",
            record.pan
        ));
    }
    Ok(record)
}

fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let record = record(state).map_err(BehaviourError::Other)?;
    let mix = Mix {
        gain_db: record.gain_db,
        pan: record.pan,
        silent: record.mute,
    };
    let peaks = context.peaks(LEVEL);
    let fader = context.processor(PROCESSOR, || Mixer::new(mix, peaks))?;
    context.update(fader, mix)?;
    context.input("audio", InputEndpoint::new(fader, Mixer::INPUT));
    context.automation_of(fader, Mixer::AUTOMATION, &mix);
    context.output("audio", OutputEndpoint::new(fader, Mixer::OUTPUT));
    Ok(())
}
