//! The `recorder`: a child of a parent of the project that records what it hears while its
//! input `record` is on and the song plays. The notes that reach its input `notes` become a
//! `timeline` next to it, and the channels of the audio input its record names a `region`,
//! each where it was heard, in one undo step. The window does the writing (`runtime`'s
//! recorders); the processor here only says, to the frame, when a take starts and ends and what
//! notes it heard.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, EventInput, EventTap, InputEndpoint, JsonTool, JsonToolDoc,
    Ports, PrepareConfig, ProcessContext, Processor, State, Tappable, ToolSchema, Watch,
};
use sound_notes::{NOTES_INPUT, NoteEvent};
use sound_signals::Number;

/// The tool of a recorder.
pub const RECORDER: &str = "recorder";
/// Its number input that records while it is above 0.
pub(crate) const RECORD: &str = "record";
/// The tap of when its takes start and end and what notes they hear, see [`Heard`].
pub const HEARD: &str = "heard";
/// The watch that is 1 while its input `record` is on, played or not: the window opens the
/// audio input for it then, so a take has its sound from its first frame.
pub const ARMED: &str = "armed";
pub(crate) const PROCESSOR: &str = "recorder";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecorderState {
    /// The channels of the audio input it records, counted from 0: none for notes alone, one
    /// for a mono take, two side by side for a stereo one.
    #[serde(default)]
    pub channels: Vec<usize>,
}

impl State for RecorderState {
    const TOOL: &'static str = RECORDER;
}

/// What a recorder heard, at a frame of the song.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Heard {
    /// A take started here: `record` came on while the song played, or the song started to play
    /// while it was on.
    Started,
    /// A note event reached it during a take.
    Note(NoteEvent),
    /// The take ended here: `record` went off, or the song stopped or jumped.
    Ended,
}

/// A tag above the 24 bits of a note event.
impl Tappable for Heard {
    fn to_bits(self) -> u64 {
        match self {
            Self::Note(event) => event.to_bits(),
            Self::Started => 1 << 32,
            Self::Ended => 2 << 32,
        }
    }

    fn from_bits(bits: u64) -> Option<Self> {
        match bits >> 32 {
            0 => NoteEvent::from_bits(bits).map(Self::Note),
            1 => Some(Self::Started),
            2 => Some(Self::Ended),
            _ => None,
        }
    }
}

/// The tool for the core, which the tools of the project define with theirs.
pub(crate) fn json_tool() -> JsonTool {
    JsonTool {
        name: RECORDER.to_string(),
        check: Arc::new(|state| record(state).map(drop)),
        doc: Some(JsonToolDoc {
            when: "A parent of this project records notes or the audio input into the song"
                .to_string(),
            markdown: include_str!("recorder.md").to_string(),
        }),
        behaviour: Box::new(apply),
        asset_folders: Vec::new(),
        owns_children: false,
        end: None,
    }
}

/// The record of the recorder, for the SDK.
pub(crate) fn schema() -> ToolSchema {
    ToolSchema {
        live: &[RECORD],
        ..ToolSchema::of::<RecorderState>(&[])
    }
}

/// The record of `state`, or what is wrong with it.
fn record(state: &Value) -> Result<RecorderState, String> {
    let record: RecorderState = serde_path_to_error::deserialize(state).map_err(|error| {
        let path = error.path().to_string();
        match path.as_str() {
            "." => format!("state: {}", error.inner()),
            _ => format!("state.{path}: {}", error.inner()),
        }
    })?;
    match record.channels.as_slice() {
        [] | [_] => Ok(record),
        [left, right] if *right == left + 1 => Ok(record),
        channels => Err(format!(
            "state.channels must be none, one channel, or two side by side such as [0, 1], not {channels:?}"
        )),
    }
}

fn apply(state: &Value, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    record(state).map_err(BehaviourError::Other)?;
    let (tap, armed) = (context.tap::<Heard>(HEARD), context.watch(ARMED));
    let node = context.processor(PROCESSOR, || Recorder::new(tap, armed))?;
    context.input(NOTES_INPUT, InputEndpoint::new(node, Recorder::NOTES));
    context.input(RECORD, InputEndpoint::new(node, Recorder::RECORD));
    Ok(())
}

pub(crate) struct Recorder {
    heard: EventTap<Heard>,
    armed: Watch,
    /// Its input `record`, from a cable or a performer.
    record: bool,
    /// What a cable last sent: a number sent every block records once.
    cabled: Option<bool>,
    /// A take runs.
    taking: bool,
    /// The frame of the song after the last block of the take.
    played_to: u64,
}

pub(crate) enum RecorderUpdate {
    /// A performer turned `record` on or off.
    Record(bool),
}

impl Recorder {
    pub(crate) const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub(crate) const RECORD: EventInput<Number> = EventInput::new(1);

    fn new(heard: EventTap<Heard>, armed: Watch) -> Self {
        Self {
            heard,
            armed,
            record: false,
            cabled: None,
            taking: false,
            played_to: 0,
        }
    }
}

impl Processor for Recorder {
    type Update = RecorderUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .event_input(Self::RECORD)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut RecorderUpdate) {
        match update {
            RecorderUpdate::Record(on) => self.record = *on,
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(timed) = context.event_inputs.get(Self::RECORD).last() {
            let on = timed.event.0 > 0.0;
            if self.cabled != Some(on) {
                self.cabled = Some(on);
                self.record = on;
            }
        }
        self.armed.set(if self.record { 1.0 } else { 0.0 });
        let transport = &context.transport;
        let range = transport.frame_range.start.0..transport.frame_range.end.0;
        let playing = transport.playing && range.start < range.end;
        // Where the song is at an offset in the block: right after a play in a project with
        // latency the song starts later than the block.
        let first = context
            .frames
            .saturating_sub((range.end - range.start) as usize);
        let frame_at = |offset: usize| range.start + offset.saturating_sub(first) as u64;
        if self.taking && (!playing || transport.jumped || !self.record) {
            // It ends where the song last played to, which a stop or a jump left behind.
            self.taking = false;
            let ended = (self.played_to, Heard::Ended);
            self.heard.write(std::iter::once(ended));
        }
        if !self.taking && self.record && playing {
            self.taking = true;
            self.heard
                .write(std::iter::once((range.start, Heard::Started)));
        }
        if self.taking {
            let notes = context.event_inputs.get(Self::NOTES).iter();
            self.heard
                .write(notes.map(|timed| (frame_at(timed.offset), Heard::Note(timed.event))));
            self.played_to = range.end;
        }
    }
}
