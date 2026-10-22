//! The `recorder`: a child of a parent of the project that records what it hears while its
//! input `record` is on and the song plays: the notes that reach its input `notes`, and the
//! channels of the audio input its record names. The window writes the takes and gives them to
//! the parent's `onTake` (`runtime`'s recorders); the processor here only says, to the frame,
//! when a take starts and ends and what notes it heard.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sound_core::{
    BehaviourContext, BehaviourError, EventInput, EventTap, InputEndpoint, JsonTool, JsonToolDoc,
    Ports, PrepareConfig, ProcessContext, Processor, State, Tappable, ToolSchema, Watch,
};
use sound_notes::{NOTES_INPUT, NoteEvent, Schedule};
use sound_signals::Number;

/// The tool of a recorder.
pub const RECORDER: &str = "recorder";
/// Its number input that records while it is above 0.
pub(crate) const RECORD: &str = "record";
/// Its number input that keeps the audio input open while it is above 0, recording or not.
pub(crate) const ARM: &str = "arm";
/// The tap of when its takes start and end and what notes they hear, see [`Heard`].
pub const HEARD: &str = "heard";
/// The watch that is 1 while its input `arm` or `record` is on, played or not: the window opens
/// the audio input for it then, so a take has its sound from its first frame.
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
            group: crate::BLOCKS,
            when: "Record notes or the audio input into the song".to_string(),
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
        live: &[RECORD, ARM],
        ..ToolSchema::of::<RecorderState>(&[])
    }
}

/// The record of `state`, or what is wrong with it.
fn record(state: &Value) -> Result<RecorderState, String> {
    let record: RecorderState = sound_core::parse_state(state)?;
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
    context.input(ARM, InputEndpoint::new(node, Recorder::ARM));
    Ok(())
}

pub(crate) struct Recorder {
    heard: EventTap<Heard>,
    armed: Watch,
    /// Its input `record`, from a cable or a performer.
    record: bool,
    /// What a cable last sent: a number sent every block records once.
    cabled: Option<bool>,
    /// Its input `arm`.
    arm: bool,
    /// A take runs.
    taking: bool,
    /// The frame of the song after the last block of the take.
    played_to: u64,
    /// What performers sent for frames of engine time still to come.
    waiting: Schedule<bool>,
}

/// How many sends for later a recorder or a timeline keeps: past that one plays at once.
pub(crate) const MAX_WAITING: usize = 16;

pub(crate) enum RecorderUpdate {
    /// A performer turned `record` on or off: from the frame of engine time it names, or now.
    Record(bool, Option<u64>),
    /// A performer turned `arm` on or off, now.
    Arm(bool),
}

impl Recorder {
    pub(crate) const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub(crate) const RECORD: EventInput<Number> = EventInput::new(1);
    pub(crate) const ARM: EventInput<Number> = EventInput::new(2);

    fn new(heard: EventTap<Heard>, armed: Watch) -> Self {
        Self {
            heard,
            armed,
            record: false,
            cabled: None,
            arm: false,
            taking: false,
            played_to: 0,
            waiting: Schedule::new(MAX_WAITING),
        }
    }
}

impl Processor for Recorder {
    type Update = RecorderUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .event_input(Self::RECORD)
            .event_input(Self::ARM)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut RecorderUpdate) {
        match update {
            RecorderUpdate::Record(on, None) => {
                self.waiting.clear();
                self.record = *on;
            }
            RecorderUpdate::Record(on, Some(at)) => {
                if !self.waiting.add(Some(*at), *on) {
                    self.record = *on;
                }
            }
            RecorderUpdate::Arm(on) => self.arm = *on,
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let (start, frames) = (context.start_frame, context.frames);
        self.waiting.playing(start, frames);
        let transport = &context.transport;
        let range = transport.frame_range.start.0..transport.frame_range.end.0;
        let playing = transport.playing && range.start < range.end;
        // Where the song is at an offset in the block: right after a play in a project with
        // latency the song starts later than the block.
        let first = frames.saturating_sub((range.end - range.start) as usize);
        let frame_at = |offset: usize| range.start + offset.saturating_sub(first) as u64;
        let mut cabled = context.event_inputs.get(Self::RECORD).iter().peekable();
        let notes = context.event_inputs.get(Self::NOTES);
        let mut read = 0;
        // The block in stretches between what turns `record` on or off, each at its frame.
        let mut offset = 0;
        loop {
            if self.taking && (!playing || (offset == 0 && transport.jumped) || !self.record) {
                // It ends where the song last played to, which a stop or a jump left behind.
                self.taking = false;
                let end = if offset == 0 {
                    self.played_to
                } else {
                    frame_at(offset)
                };
                self.heard.write(std::iter::once((end, Heard::Ended)));
            }
            if !self.taking && self.record && playing {
                self.taking = true;
                let begin = frame_at(offset);
                self.heard.write(std::iter::once((begin, Heard::Started)));
            }
            let scheduled = (self.waiting.next())
                .filter(|at| *at < start + frames as u64)
                .map(|at| at.saturating_sub(start) as usize);
            let cable = cabled.peek().map(|timed| timed.offset);
            let next = scheduled.into_iter().chain(cable).min().unwrap_or(frames);
            let heard = &notes[read..];
            let heard = &heard[..heard.partition_point(|timed| timed.offset < next)];
            read += heard.len();
            if self.taking {
                self.heard.write(
                    heard
                        .iter()
                        .map(|timed| (frame_at(timed.offset), Heard::Note(timed.event))),
                );
                self.played_to = if next >= frames {
                    range.end
                } else {
                    frame_at(next)
                };
            }
            if next >= frames {
                break;
            }
            if scheduled == Some(next) {
                if let Some((_, on)) = self.waiting.take(start + next as u64) {
                    self.record = on;
                }
            } else if let Some(timed) = cabled.next() {
                // A number sent every block records once.
                let on = timed.event.0 > 0.0;
                if self.cabled != Some(on) {
                    self.cabled = Some(on);
                    self.record = on;
                }
            }
            offset = next;
        }
        if let Some(armed) = context.event_inputs.get(Self::ARM).last() {
            self.arm = armed.event.0 > 0.0;
        }
        self.armed
            .set(if self.record || self.arm { 1.0 } else { 0.0 });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::sync::Arc;

    use sound_core::{Connection, Engine, EngineConfig, EngineControl, Node, Ticks};
    use sound_notes::{Length, Note, NotePlayerUpdate, Pattern, Pitch, Quantize, Velocity};

    use super::*;
    use crate::timeline::{Timeline, TimelineUpdate};

    /// A timeline that waits for a launch, whose notes a recorder hears.
    struct Rig {
        control: EngineControl,
        engine: Engine,
        recorder: Node<Recorder>,
        timeline: Node<Timeline>,
        heard: EventTap<Heard>,
    }

    fn rig() -> Rig {
        let (mut control, engine) = Engine::new(EngineConfig::new(48_000, 2));
        let heard = EventTap::new();
        let mut edit = control.edit();
        let recorder = Recorder::new(heard.clone(), Watch::new());
        let recorder = edit.add_processor("recorder", recorder).unwrap();
        let timeline = edit.add_processor("timeline", Timeline::new()).unwrap();
        edit.connect(Connection::new(
            timeline.id(),
            Timeline::NOTES,
            recorder.id(),
            Recorder::NOTES,
        ))
        .unwrap();
        edit.commit().unwrap();
        let note = Note {
            start: Ticks(0),
            length: Length::new(Ticks(960)).unwrap(),
            pitch: Pitch::new(60).unwrap(),
            velocity: Velocity::new(100).unwrap(),
        };
        let pattern = Pattern::new(Length::new(Ticks(3_840)).unwrap(), &[note]).unwrap();
        let patterns = NotePlayerUpdate::Patterns(Arc::new(vec![pattern]));
        (control.update(timeline, TimelineUpdate::Player(patterns))).unwrap();
        let launches = TimelineUpdate::Launches((Quantize::Now, false));
        control.update(timeline, launches).unwrap();
        control.play();
        Rig {
            control,
            engine,
            recorder,
            timeline,
            heard,
        }
    }

    impl Rig {
        fn record(&mut self, on: bool, at: Option<u64>) {
            let update = RecorderUpdate::Record(on, at);
            self.control.update(self.recorder, update).unwrap();
        }

        /// Plays `frames` in blocks of 64 and gives what the recorder heard so far.
        fn render(&mut self, frames: usize) -> Vec<(u64, Heard)> {
            let mut output = vec![0.0; frames * 2];
            for block in output.chunks_mut(64 * 2) {
                self.engine.process_block(block);
            }
            self.control.poll().unwrap();
            let mut heard = Vec::new();
            self.heard.read(0, &mut heard);
            heard
        }
    }

    #[test]
    fn a_take_sent_for_later_starts_and_ends_at_its_frames() {
        let mut rig = rig();
        rig.record(true, Some(32));
        rig.record(false, Some(96));
        let heard = rig.render(256);
        assert_eq!(heard, [(32, Heard::Started), (96, Heard::Ended)]);
    }

    #[test]
    fn a_send_for_now_drops_what_waits() {
        let mut rig = rig();
        rig.record(true, Some(1_000));
        rig.record(false, None);
        assert_eq!(rig.render(2_048), []);
    }

    #[test]
    fn a_timeline_that_waits_launches_at_the_frame_it_was_sent_for() {
        let mut rig = rig();
        rig.record(true, None);
        rig.render(256);
        let launch = TimelineUpdate::Launch(true, Some(512 + 32));
        rig.control.update(rig.timeline, launch).unwrap();
        let heard = rig.render(2_048);
        let on = heard
            .iter()
            .find(|(_, heard)| matches!(heard, Heard::Note(_)));
        // On the first tick from that frame: a tick is 25 frames at 120 bpm.
        let frame = on.map(|(frame, _)| *frame).unwrap();
        assert!((544..569).contains(&frame), "{frame}");
    }
}
