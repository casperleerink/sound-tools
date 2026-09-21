//! The saved raw take: what arrived while recording, in real time, and the clip it becomes.
//!
//! The take is the record of the performance and the [`Clip`] is the edit of it. Two parts of
//! the application read a take and neither may depend on the other: the MIDI extension writes
//! one when a recording ends, and the fit extension reads one to find the beats and to make the
//! clip again. So the saved form lives in the note contract crate, as the clip does, and the
//! core knows nothing about either.
//!
//! Times are microseconds and never ticks, because ticks mean nothing without a tempo map and
//! the whole point of a fit is that the tempo map changes. Every message therefore carries two:
//! [`RawEvent::time_us`], when it reached this process, which is the performance as it was
//! played, and [`RawEvent::sounded_us`], when the engine sounded it, which is what a clip has
//! to reproduce to sound the same. Both count from the moment recording began, and
//! [`RawTake::start_us`] says where that moment is on the project timeline.
//!
//! A take has a name of its own, never one made from a clip: a clip id changes when the clip
//! moves to another track or an agent renames its file, and then nothing would find the take
//! again. The clip names its take in a saved field instead, which travels with it through
//! every edit. A name is never used twice, and the file is created and never opened again, so
//! nothing this program does can write over a performance.

use serde::{Deserialize, Serialize};
use sound_core::{AssetError, AssetName, Assets, InvalidAssetName, Ticks};

use crate::{Clip, Length, Note, Pedal, PedalChange, Pitch, Velocity};

/// Where raw takes live under `assets/`.
pub const TAKES_FOLDER: &str = "assets/takes";

/// Names are `take-1`, `take-2` and so on, counted by the asset facility of the core. It never
/// writes over a file that is there, so the next free number is always past every take this
/// project ever made, also past the ones an undo took the clip of.
const FOLDER: &str = "takes";
const NAME: &str = "take";
const EXTENSION: &str = "json";

/// The asset of the raw take called `name`, for example `take-1`.
pub fn take_asset(name: &str) -> Result<AssetName, InvalidAssetName> {
    AssetName::new(FOLDER, name, EXTENSION)
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TakeError {
    #[error("{0}")]
    InvalidName(#[from] InvalidAssetName),
    #[error("assets/takes/{name}.json does not exist")]
    Missing { name: String },
    #[error("assets/takes/{name}.json: {message}")]
    Invalid { name: String, message: String },
    #[error("{0}")]
    Asset(String),
}

/// The longest one recording can be: an hour. A take is one performance, and every reader of
/// one lays out an array over its length, so a damaged file with a time of a thousand years in
/// it would ask for memory nobody has. The file itself is never changed: it is refused and
/// named.
pub const MAX_TAKE_MICROS: u64 = 60 * 60 * 1_000_000;

/// The furthest a take may sit on the project timeline: a thousand hours, far past anything a
/// piece is and far below where the arithmetic of a reader could overflow.
pub const MAX_PROJECT_MICROS: u64 = 1_000 * 60 * 60 * 1_000_000;

impl From<AssetError> for TakeError {
    fn from(error: AssetError) -> Self {
        Self::Asset(error.to_string())
    }
}

/// One message of a saved take. Both times count from the moment recording began.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum RawEvent {
    On {
        time_us: u64,
        sounded_us: u64,
        pitch: u8,
        velocity: u8,
    },
    Off {
        time_us: u64,
        sounded_us: u64,
        pitch: u8,
        /// How fast the key came up, 0 to 127. Most keyboards send 0 or 64.
        velocity: u8,
    },
    Pedal {
        time_us: u64,
        sounded_us: u64,
        value: u8,
    },
}

impl RawEvent {
    /// When the message reached this process: the performance as it was played. The beat finder
    /// works from these.
    pub fn time_us(self) -> u64 {
        match self {
            Self::On { time_us, .. } | Self::Off { time_us, .. } | Self::Pedal { time_us, .. } => {
                time_us
            }
        }
    }

    /// When the engine sounded the message, which is the start of the audio block that carried
    /// it. A clip that puts the message here renders what the composer heard.
    pub fn sounded_us(self) -> u64 {
        match self {
            Self::On { sounded_us, .. }
            | Self::Off { sounded_us, .. }
            | Self::Pedal { sounded_us, .. } => sounded_us,
        }
    }

    /// One line of the file. It cannot fail: every field is a number or a fixed name.
    fn json(self) -> String {
        match self {
            Self::On {
                time_us,
                sounded_us,
                pitch,
                velocity,
            } => format!(
                r#"{{"kind":"on","time_us":{time_us},"sounded_us":{sounded_us},"pitch":{pitch},"velocity":{velocity}}}"#
            ),
            Self::Off {
                time_us,
                sounded_us,
                pitch,
                velocity,
            } => format!(
                r#"{{"kind":"off","time_us":{time_us},"sounded_us":{sounded_us},"pitch":{pitch},"velocity":{velocity}}}"#
            ),
            Self::Pedal {
                time_us,
                sounded_us,
                value,
            } => format!(
                r#"{{"kind":"pedal","time_us":{time_us},"sounded_us":{sounded_us},"value":{value}}}"#
            ),
        }
    }
}

/// The saved form of a take. An agent reads it, and the fit extension reads it back.
///
/// It names no clip: the take is written before the clip is made, and a clip id changes. The
/// clip names the take.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTake {
    /// Where recording began on the project timeline, in microseconds from the project start.
    /// This is what keeps the take where it was heard when the tempo map changes under it.
    pub start_us: u64,
    /// Where it ended, in the same unit.
    pub end_us: u64,
    /// The playhead where recording began, under the tempo map of that moment. For reading;
    /// nothing computes from it, because a tempo map change makes it mean something else.
    pub start_tick: u64,
    /// Where it ended, under the same map.
    pub end_tick: u64,
    /// Where the sustain pedal stood when recording began, 0 to 127.
    pub pedal_at_start: u8,
    pub events: Vec<RawEvent>,
}

impl RawTake {
    /// Reads the take called `name`, for example `take-1`.
    pub fn read(assets: &Assets, name: &str) -> Result<Self, TakeError> {
        let asset = take_asset(name)?;
        let bytes = assets.read(&asset)?;
        let bytes = bytes.ok_or_else(|| TakeError::Missing {
            name: name.to_string(),
        })?;
        let take: Self = serde_json::from_slice(&bytes).map_err(|error| TakeError::Invalid {
            name: name.to_string(),
            message: error.to_string(),
        })?;
        take.validate().map_err(|message| TakeError::Invalid {
            name: name.to_string(),
            message,
        })?;
        Ok(take)
    }

    /// Whether the times in this take are times: in order, inside the bounds above, and small
    /// enough that no reader has to guard its own arithmetic. A take this program wrote always
    /// is; a file that was damaged or written by hand may not be, and then it is refused with
    /// the reason instead of being read.
    pub fn validate(&self) -> Result<(), String> {
        let bound = |field: &str, value: u64, limit: u64| match value <= limit {
            true => Ok(()),
            false => Err(format!(
                "{field} is {value} microseconds, and the most a take may hold is {limit}"
            )),
        };
        bound("start_us", self.start_us, MAX_PROJECT_MICROS)?;
        bound("end_us", self.end_us, MAX_PROJECT_MICROS)?;
        if self.end_us < self.start_us {
            return Err(format!(
                "end_us is {} and start_us is {}: a recording does not end before it begins",
                self.end_us, self.start_us
            ));
        }
        bound("the take", self.end_us - self.start_us, MAX_TAKE_MICROS)?;
        let mut latest = 0;
        for (index, event) in self.events.iter().enumerate() {
            bound(
                &format!("events[{index}].time_us"),
                event.time_us(),
                MAX_TAKE_MICROS,
            )?;
            bound(
                &format!("events[{index}].sounded_us"),
                event.sounded_us(),
                MAX_TAKE_MICROS,
            )?;
            if event.time_us() < latest {
                return Err(format!(
                    "events[{index}].time_us is {} and the message before it arrived at {latest}: the messages of a take are in the order they arrived",
                    event.time_us()
                ));
            }
            latest = event.time_us();
        }
        Ok(())
    }

    /// Writes the take under a name of its own and gives that name, for the clip to keep.
    ///
    /// [`Assets::create`] creates the file and never opens one that is there, so no
    /// performance this program has written can be lost, whatever happened to its clip. The
    /// runtime never writes it again and never removes it, not even on undo.
    pub fn write(&self, assets: &Assets) -> Result<String, TakeError> {
        let name = AssetName::new(FOLDER, NAME, EXTENSION)?;
        let written = assets.create(&name, self.json().as_bytes())?;
        Ok(written.name().to_string())
    }

    /// The take as it is written: one message per line, like a note of a clip, so a minute of
    /// playing is a file an agent can read and git can show.
    pub fn json(&self) -> String {
        let lines: Vec<String> = self.events.iter().map(|event| event.json()).collect();
        format!(
            "{{\n  \"start_us\": {},\n  \"end_us\": {},\n  \"start_tick\": {},\n  \"end_tick\": {},\n  \"pedal_at_start\": {},\n  \"events\": [\n    {}\n  ]\n}}\n",
            self.start_us,
            self.end_us,
            self.start_tick,
            self.end_tick,
            self.pedal_at_start,
            lines.join(",\n    ")
        )
    }

    /// Whether anything was played. An empty take makes no clip and no file.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The clip of this take, with every message at the tick `tick_of` gives for its place on
    /// the project timeline in microseconds.
    ///
    /// This is the one place that turns a performance into a clip. Recording calls it with the
    /// clock of the moment, so a note lands on the tick the engine sounded it. A fit calls it
    /// with the clock of the fitted tempo map, so the notes keep their place in real time under
    /// the new grid. The rules are the same either way:
    ///
    /// - A note that was still held at the end ends there.
    /// - A note off with no note on before it, from a key that was already down when recording
    ///   began, is left out: half a note is not music.
    /// - A pedal move that changes nothing is left out, because some pedals send a stream of
    ///   the same value.
    /// - A take that began under a held pedal starts with that value, so it plays back as it
    ///   sounded, and one that ends with the pedal down lifts it at its end, as a held note
    ///   ends there. Without the lift a clip would sustain for the rest of the piece.
    ///
    /// `None` when nothing was played.
    pub fn clip(&self, tick_of: impl Fn(u64) -> Ticks) -> Option<Clip> {
        if self.is_empty() {
            return None;
        }
        let start = tick_of(self.start_us);
        let at = |event: &RawEvent| tick_of(self.start_us + event.sounded_us()).max(start);
        // The last tick the clip has to cover: its own end, or one tick past the last message
        // when the recording was stopped before the engine had passed it.
        let last = self.events.iter().map(at).max().unwrap_or(start);
        let last_tick = tick_of(self.end_us)
            .max(Ticks(last.0 + 1))
            .max(Ticks(start.0 + 1));
        // The clip is one tick longer when it ends under the pedal, so that the lift fits
        // inside it: a pedal move, like a note, starts inside its clip.
        let lifts = self.pedal_at_end().is_down();
        let end = Ticks(last_tick.0 + u64::from(lifts));
        let length = Length::at_least_one(end.saturating_sub(start));

        // Note ons waiting for their off, oldest first, so two presses of one pitch get their
        // own off in the order they were played.
        let mut held: Vec<(Pitch, Ticks, Velocity)> = Vec::new();
        let mut notes = Vec::new();
        let mut pedal = Vec::new();
        let mut pedal_value = Pedal::UP;
        if Pedal::nearest(i64::from(self.pedal_at_start)).is_down() {
            pedal_value = Pedal::nearest(i64::from(self.pedal_at_start));
            pedal.push(PedalChange {
                start: Ticks(0),
                value: pedal_value,
            });
        }
        for event in &self.events {
            let in_clip = at(event).saturating_sub(start);
            match *event {
                RawEvent::On {
                    pitch, velocity, ..
                } => held.push((
                    Pitch::nearest(i64::from(pitch)),
                    in_clip,
                    Velocity::nearest(i64::from(velocity)),
                )),
                RawEvent::Off { pitch, .. } => {
                    let pitch = Pitch::nearest(i64::from(pitch));
                    let Some(index) = held.iter().position(|(held, ..)| *held == pitch) else {
                        continue;
                    };
                    let (pitch, note_start, velocity) = held.remove(index);
                    notes.push(Note {
                        start: note_start,
                        length: Length::at_least_one(in_clip.saturating_sub(note_start)),
                        pitch,
                        velocity,
                    });
                }
                RawEvent::Pedal { value, .. } => {
                    let value = Pedal::nearest(i64::from(value));
                    if value != pedal_value {
                        pedal_value = value;
                        pedal.push(PedalChange {
                            start: in_clip,
                            value,
                        });
                    }
                }
            }
        }
        // Still held when recording ended: the note ends there.
        let clip_end = length.ticks();
        for (pitch, start, velocity) in held {
            notes.push(Note {
                start,
                length: Length::at_least_one(clip_end.saturating_sub(start)),
                pitch,
                velocity,
            });
        }
        // A take that ends with the pedal down lifts it at its end, as a held note ends there.
        if lifts {
            pedal.push(PedalChange {
                start: Ticks(length.ticks().0 - 1),
                value: Pedal::UP,
            });
        }
        // The agent doc asks an agent to keep notes in this order, so the runtime writes them
        // that way too.
        notes.sort_by_key(|note| (note.start, note.pitch));
        let mut clip = Clip::new(start, length, notes);
        clip.pedal = pedal;
        Some(clip)
    }

    /// Where the pedal stands at the end of the take.
    fn pedal_at_end(&self) -> Pedal {
        let last = self.events.iter().rev().find_map(|event| match event {
            RawEvent::Pedal { value, .. } => Some(Pedal::nearest(i64::from(*value))),
            _ => None,
        });
        last.unwrap_or_else(|| Pedal::nearest(i64::from(self.pedal_at_start)))
    }
}
