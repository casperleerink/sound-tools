//! The `clip`: a child of a parent of the project that plays its notes once, at its place in the
//! song, out of its output `notes`, as a clip of the arrangement does. A track of the project is
//! a parent whose clips are children of this tool, each its own record, so an edit of one clip
//! touches only that file.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, JsonTool, JsonToolDoc, OutputEndpoint, State, Ticks,
    ToolSchema,
};
use sound_notes::{Clip, Length, Note, NotePlayer, NotePlayerUpdate, NoteTimeline};

use crate::tools::key;

pub(crate) const NAME: &str = "clip";
const NOTES: &str = "notes";
const PROCESSOR: &str = "player";

#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// Where it starts in the song, in ticks: 3840 is a bar of 4/4.
    start: Ticks,
    /// In ticks. A note that is longer ends with the clip.
    length: Length,
    /// Notes that start inside it, in ticks from its start.
    #[serde(default)]
    notes: Vec<Note>,
}

impl State for Record {
    const TOOL: &'static str = NAME;
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: NAME.to_string(),
        check: Arc::new(|state| clip(state).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project plays notes at a place in the song, as a clip of a track"
                .to_string(),
            markdown: include_str!("clip.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
    }
}

/// The record of the clip, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema::of::<Record>(&[])
}

/// The clip of `state`, or what is wrong with it, by its path.
fn clip(state: &Value) -> Result<Clip, String> {
    let record: Record = serde_path_to_error::deserialize(state).map_err(|error| {
        let path = error.path().to_string();
        match path.as_str() {
            "." => format!("state: {}", error.inner()),
            _ => format!("state.{path}: {}", error.inner()),
        }
    })?;
    let clip = Clip::new(record.start, record.length, record.notes);
    clip.validate()?;
    Ok(clip)
}

/// One note player for the clip, sent a new timeline when the record changes. Its offs come
/// from what it started, so a moved or shortened clip ends its notes.
fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let clip = clip(state).map_err(BehaviourError::Other)?;
    let node = context.processor(PROCESSOR, NotePlayer::default)?;
    if context.changed("clip", key(state)) {
        let timeline = NoteTimeline::new([&clip]);
        context.update(node, NotePlayerUpdate::Timeline(Arc::new(timeline)))?;
    }
    context.output(NOTES, OutputEndpoint::new(node, NotePlayer::NOTES));
    Ok(())
}
