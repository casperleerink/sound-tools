//! The `timeline`: a child of a parent of the project that plays events at their positions in
//! the song, once, out of its outputs, as the transport passes them. Notes for now. A parent
//! that has many, each its own record, edits one by changing one file: the clips of a track,
//! the cues of a score.

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

pub(crate) const NAME: &str = "timeline";
const NOTES: &str = "notes";
const PROCESSOR: &str = "player";

#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// Where its events count from in the song, in ticks: 3840 is a bar of 4/4.
    #[serde(default)]
    start: Ticks,
    /// Where it ends, in ticks from `start`: a note that is longer ends there, and none may
    /// start after it. Without one it plays every note to its end.
    #[serde(default)]
    length: Option<Length>,
    /// Notes, in ticks from `start`.
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
        check: Arc::new(|state| placed(state).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project plays notes at positions in the song, such as clips or cues"
                .to_string(),
            markdown: include_str!("timeline.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
    }
}

/// The record of the timeline, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema::of::<Record>(&[])
}

/// The notes of `state` placed in the song, or what is wrong with it, by its path.
fn placed(state: &Value) -> Result<Clip, String> {
    let record: Record = serde_path_to_error::deserialize(state).map_err(|error| {
        let path = error.path().to_string();
        match path.as_str() {
            "." => format!("state: {}", error.inner()),
            _ => format!("state.{path}: {}", error.inner()),
        }
    })?;
    let length = match record.length {
        Some(length) => {
            let mut notes = record.notes.iter().enumerate();
            if let Some((index, note)) = notes.find(|(_, note)| note.start >= length.ticks()) {
                return Err(format!(
                    "notes[{index}].start must be less than the length {}, not {}: a note counts from `start`",
                    length.ticks().0,
                    note.start.0
                ));
            }
            length
        }
        None => {
            let end = (record.notes.iter()).map(|note| note.start + note.length.ticks());
            Length::at_least_one(end.max().unwrap_or(Ticks(0)))
        }
    };
    Ok(Clip::new(record.start, length, record.notes))
}

/// One note player, sent the notes again when the record changes. Its offs come from what it
/// started, so notes that move or go end where they sounded.
fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let placed = placed(state).map_err(BehaviourError::Other)?;
    let node = context.processor(PROCESSOR, NotePlayer::default)?;
    if context.changed("notes", key(state)) {
        let timeline = NoteTimeline::new([&placed]);
        context.update(node, NotePlayerUpdate::Timeline(Arc::new(timeline)))?;
    }
    context.output(NOTES, OutputEndpoint::new(node, NotePlayer::NOTES));
    Ok(())
}
