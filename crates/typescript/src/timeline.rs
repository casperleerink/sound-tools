//! The `timeline`: a child of a parent of the project that plays notes in time, out of its
//! output `notes`: once at their place in the song, such as a clip or a cue, or round after
//! round, such as a pattern a launcher starts. Its record says where it plays, so a render
//! sounds; a launch is a number into its input `launch`, from a cable or a performer, and
//! nothing saves it.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, EventInput, EventOutput, InputEndpoint, JsonTool,
    JsonToolDoc, OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, State, Ticks,
    ToolSchema, Watch,
};
use sound_notes::{
    Launch, Length, NO_PATTERN, Note, NoteEvent, NotePlayer, NotePlayerUpdate, Pattern, Placing,
    PlayerWatches, QUEUED_STOP, Quantize,
};
use sound_signals::Number;

use crate::tools::key;

pub(crate) const NAME: &str = "timeline";
/// Its input of launches, which a performer reaches as a number input.
pub(crate) const LAUNCH: &str = "launch";
const NOTES: &str = "notes";
/// The name of its processor in its instance.
pub(crate) const PROCESSOR: &str = "player";

#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Record {
    /// Where its notes count from in the song, in ticks: 3840 is a bar of 4/4.
    #[serde(default)]
    start: Ticks,
    /// One round, in ticks from `start`: a note that is longer ends there, and none may start
    /// at or after it. Without one it is as long as its notes. A loop needs one.
    #[serde(default)]
    length: Option<Length>,
    /// Notes, in ticks from `start`.
    #[serde(default)]
    notes: Vec<Note>,
    /// Plays round after round from `start`, instead of once.
    #[serde(default, rename = "loop")]
    looped: bool,
    /// Whether it plays from `start` before any launch. A launch moves it for the performance.
    #[serde(default = "plays")]
    plays: bool,
    /// Where a launch lands while the piece plays. While it is stopped, at once.
    #[serde(default)]
    quantize: Quantize,
}

fn plays() -> bool {
    true
}

impl State for Record {
    const TOOL: &'static str = NAME;
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: NAME.to_string(),
        check: Arc::new(|state| record(state).and_then(|record| round(&record)).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project plays notes in time: clips or cues in the song, or patterns that loop and launch"
                .to_string(),
            markdown: include_str!("timeline.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
        end: Some(Box::new(end)),
    }
}

/// Where it stops playing in the song, which a render and the transport count as the end of
/// the piece: none for a loop, or for one that waits for a launch. Read from the JSON as it is,
/// with no copy of the notes, as every timeline of a long song is asked after each edit.
fn end(state: &Value) -> Option<Ticks> {
    let flag = |name: &str, default: bool| state.get(name).and_then(Value::as_bool).unwrap_or(default);
    if flag("loop", false) || !flag("plays", true) {
        return None;
    }
    let ticks = |value: &Value, name: &str| value.get(name).and_then(Value::as_u64);
    let start = ticks(state, "start").unwrap_or(0);
    let length = ticks(state, "length").or_else(|| {
        let notes = state.get("notes").and_then(Value::as_array)?;
        let ends = notes.iter().filter_map(|note| Some(ticks(note, "start")? + ticks(note, "length")?));
        ends.max()
    })?;
    Some(Ticks(start + length))
}

/// The record of the timeline, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        live: &[LAUNCH],
        ..ToolSchema::of::<Record>(&[])
    }
}

/// The record of `state`, or what is wrong with it, by its path.
fn record(state: &Value) -> Result<Record, String> {
    serde_path_to_error::deserialize(state).map_err(|error| {
        let path = error.path().to_string();
        match path.as_str() {
            "." => format!("state: {}", error.inner()),
            _ => format!("state.{path}: {}", error.inner()),
        }
    })
}

/// One round of its notes, or what is wrong with them.
fn round(record: &Record) -> Result<Pattern, String> {
    let length = match record.length {
        Some(length) => length,
        None if record.looped => return Err("a timeline that loops needs a `length`".to_string()),
        None => {
            let end = (record.notes.iter()).map(|note| note.start + note.length.ticks());
            Length::at_least_one(end.max().unwrap_or(Ticks(0)))
        }
    };
    Pattern::new(length, &record.notes).map_err(|error| {
        let error = error.to_string();
        error.replace("the pattern length", "the length")
    })
}

/// One player. A change of its notes swaps them, so held notes follow; a change of where it
/// plays places it again, which ends what it holds.
fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let record = record(state).map_err(BehaviourError::Other)?;
    let pattern = round(&record).map_err(BehaviourError::Other)?;
    // Only a loop shows what plays: a song of many timelines keeps no watches it never reads.
    let watches = record.looped.then(|| TimelineWatches {
        playing: context.watch("playing"),
        queued: context.watch("queued"),
        phase: context.watch("phase"),
    });
    let node = context.processor(PROCESSOR, || Timeline::new(watches))?;
    if context.changed(
        "notes",
        key(&(&record.length, &record.notes, record.looped)),
    ) {
        let patterns = NotePlayerUpdate::Patterns(Arc::new(vec![pattern]));
        context.update(node, TimelineUpdate::Player(patterns))?;
    }
    if context.changed("quantize", key(&(record.quantize, record.looped))) {
        let how = (record.quantize, record.looped);
        context.update(node, TimelineUpdate::Launches(how))?;
    }
    if context.changed("place", key(&(record.start, record.plays, record.looped))) {
        let placing = Placing {
            pattern: record.plays.then_some(0),
            at: record.start,
            once: !record.looped,
        };
        context.update(
            node,
            TimelineUpdate::Player(NotePlayerUpdate::Place(placing)),
        )?;
    }
    context.output(NOTES, OutputEndpoint::new(node, Timeline::NOTES));
    context.input(LAUNCH, InputEndpoint::new(node, Timeline::LAUNCH));
    Ok(())
}

/// What a looping timeline shows: whether it plays, 1 or 0; a launch that waits, 1 to start
/// and -1 to stop, else 0; and how far into its round it is, 0 to 1.
struct TimelineWatches {
    playing: Watch,
    queued: Watch,
    phase: Watch,
}

/// A [`NotePlayer`] of one round that launches what its input `launch` hears.
pub(crate) struct Timeline {
    player: NotePlayer,
    /// What the player shows, by pattern index, which `shown` turns into the watches above.
    inner: PlayerWatches,
    shown: Option<TimelineWatches>,
    quantize: Quantize,
    looped: bool,
    /// What the input last launched: a number a cable sends every block launches once.
    heard: Option<bool>,
}

pub(crate) enum TimelineUpdate {
    /// Its round, or where it is placed. The old round rides back in here.
    Player(NotePlayerUpdate),
    /// Where a launch lands, and whether it loops.
    Launches((Quantize, bool)),
    /// Starts at the next point of its quantize, or stops there with `false`.
    Launch(bool),
}

impl Timeline {
    pub(crate) const LAUNCH: EventInput<Number> = EventInput::new(0);
    pub(crate) const NOTES: EventOutput<NoteEvent> = NotePlayer::NOTES;

    fn new(shown: Option<TimelineWatches>) -> Self {
        let inner = PlayerWatches::default();
        Self {
            player: NotePlayer::watched(inner.clone()),
            inner,
            shown,
            quantize: Quantize::default(),
            looped: false,
            heard: None,
        }
    }

    fn launch(&mut self, start: bool) {
        let launch = Launch {
            pattern: start.then_some(0),
            quantize: self.quantize,
            once: !self.looped,
        };
        self.player.update(&mut NotePlayerUpdate::Launch(launch));
    }
}

/// Whether a number starts a timeline: above 0. 0 and below stop it.
pub(crate) fn launched(value: f32) -> bool {
    value > 0.0
}

impl Processor for Timeline {
    type Update = TimelineUpdate;

    fn ports(&self) -> Ports {
        self.player.ports().event_input(Self::LAUNCH)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.player.prepare(config);
    }

    fn update(&mut self, update: &mut TimelineUpdate) {
        match update {
            TimelineUpdate::Player(update) => self.player.update(update),
            TimelineUpdate::Launches((quantize, looped)) => {
                (self.quantize, self.looped) = (*quantize, *looped);
            }
            TimelineUpdate::Launch(start) => self.launch(*start),
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        // The last of the block: two cables would launch each other again every block.
        if let Some(timed) = context.event_inputs.get(Self::LAUNCH).last() {
            let start = launched(timed.event.0);
            if self.heard != Some(start) {
                self.heard = Some(start);
                self.launch(start);
            }
        }
        self.player.process(context);
        if let Some(shown) = &self.shown {
            let playing = self.inner.playing.get() != NO_PATTERN;
            shown.playing.set(if playing { 1.0 } else { 0.0 });
            let queued = self.inner.queued.get();
            let queued = if queued == NO_PATTERN {
                0.0
            } else if queued == QUEUED_STOP {
                -1.0
            } else {
                1.0
            };
            shown.queued.set(queued);
            shown.phase.set(self.inner.phase.get());
        }
    }
}
