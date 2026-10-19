//! The `region`: a child of a parent of the project that plays a stretch of an audio file at its
//! place in the song, out of its output `audio`, as an audio clip of the arrangement plays.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, JsonTool, JsonToolDoc, OutputEndpoint, State, Ticks,
    ToolSchema,
};
use sound_media::{AUDIO_FOLDER, AudioAsset, AudioPlayer, AudioUpdate, Region};

pub(crate) const NAME: &str = "region";
const PROCESSOR: &str = "player";

/// The loudest a region plays over its file, in decibels.
const MAX_GAIN_DB: f32 = 24.0;
/// What a region at this gain or below plays: silence.
const SILENT_DB: f32 = -96.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// The file, under `assets/audio/`, such as `"take-1.wav"`.
    asset: AudioAsset,
    /// Where it starts in the song, in ticks: 3840 is a bar of 4/4.
    #[serde(default)]
    start: Ticks,
    /// Where in the file it starts playing.
    #[serde(default)]
    file_start_seconds: f64,
    /// Where in the file it stops. The end of the file when left out.
    #[serde(default)]
    file_end_seconds: Option<f64>,
    /// How much louder or quieter it plays than its file, -96 (silence) to 24.
    #[serde(default)]
    gain_db: f32,
    /// A straight line from silence at its start to its level.
    #[serde(default)]
    fade_in_ms: f32,
    /// A straight line from its level to silence at its end.
    #[serde(default)]
    fade_out_ms: f32,
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
            when: "A parent of this project plays an audio file at a place in the song: an audio clip or a take"
                .to_string(),
            markdown: include_str!("region.md").to_string(),
        }),
        behaviour: Box::new(apply),
        // A file that arrives plays a region that waited for it.
        asset_folders: vec![AUDIO_FOLDER],
        owns_children: false,
        end: None,
    }
}

/// The record of the region, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema::of::<Record>(&[])
}

/// The record of `state`, or what is wrong with it, by its path.
fn record(state: &Value) -> Result<Record, String> {
    let record: Record = serde_path_to_error::deserialize(state).map_err(|error| {
        let path = error.path().to_string();
        match path.as_str() {
            "." => format!("state: {}", error.inner()),
            _ => format!("state.{path}: {}", error.inner()),
        }
    })?;
    let seconds = |field: &str, value: f64| match value.is_finite() && value >= 0.0 {
        true => Ok(()),
        false => Err(format!(
            "state.{field} must be a number of seconds, 0 or more, not {value}"
        )),
    };
    seconds("file_start_seconds", record.file_start_seconds)?;
    if let Some(end) = record.file_end_seconds {
        seconds("file_end_seconds", end)?;
        if end <= record.file_start_seconds {
            return Err(format!(
                "state.file_end_seconds must be after file_start_seconds {}, not {end}",
                record.file_start_seconds
            ));
        }
    }
    if !(SILENT_DB..=MAX_GAIN_DB).contains(&record.gain_db) {
        return Err(format!(
            "state.gain_db must be from {SILENT_DB} to {MAX_GAIN_DB}, not {}",
            record.gain_db
        ));
    }
    for (field, value) in [
        ("fade_in_ms", record.fade_in_ms),
        ("fade_out_ms", record.fade_out_ms),
    ] {
        if !(value.is_finite() && value >= 0.0) {
            return Err(format!(
                "state.{field} must be a number of milliseconds, 0 or more, not {value}"
            ));
        }
    }
    Ok(record)
}

/// One player of one region. A file that is not there is a problem: the region keeps its place
/// and is silent until the file arrives.
fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let record = record(state).map_err(BehaviourError::Other)?;
    let gain = match record.gain_db <= SILENT_DB {
        true => 0.0,
        false => 10_f32.powf(record.gain_db / 20.0),
    };
    let region = Region {
        asset: record.asset,
        start: record.start,
        file_start_seconds: record.file_start_seconds,
        file_end_seconds: record.file_end_seconds,
        gain,
        fade_in_ms: record.fade_in_ms,
        fade_out_ms: record.fade_out_ms,
    };
    let rate = context.prepare_config().sample_rate;
    let what = format!("the region {:?}", context.id().name());
    let (snapshot, problems) = sound_media::snapshot([(what, region)], context.assets(), rate);
    for problem in problems {
        context.problem(problem);
    }
    let player = context.processor(PROCESSOR, AudioPlayer::new)?;
    context.update(player, AudioUpdate::new(snapshot))?;
    context.output("audio", OutputEndpoint::new(player, AudioPlayer::OUTPUT));
    Ok(())
}
