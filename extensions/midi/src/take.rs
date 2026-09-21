//! The raw take: what arrived while recording, in real time, and the clip it becomes.
//!
//! The take is the record of the performance and the clip is the edit of it. The clip holds
//! the tick at which the engine sounded each message, so playing it back renders what was
//! heard. The take holds the times as they arrived and both velocities of every note, which
//! the clip has no room for. Nothing ever writes the take again.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sound_core::{InstanceId, Ticks};
use sound_notes::{Clip, Length, Note, Pedal, PedalChange, Pitch};

use crate::keys::Played;

/// Where raw takes live in the project folder. One file per recorded clip, at the path of the
/// clip: the clip `arrangement/piano/take-1` has `assets/takes/arrangement/piano/take-1.json`.
pub const TAKES_FOLDER: &str = "assets/takes";

/// The file of the raw take of a clip.
pub fn take_path(root: &Path, clip: &InstanceId) -> PathBuf {
    root.join(TAKES_FOLDER).join(format!("{clip}.json"))
}

/// One message of a take: what arrived and when, plus where the engine sounded it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TakeEvent {
    /// Microseconds from the moment recording began.
    pub time_us: u64,
    /// The project tick the engine sounded it on.
    pub tick: Ticks,
    pub played: Played,
}

/// Everything that arrived between the start and the end of one recording.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Take {
    /// The playhead where recording began.
    pub start: Ticks,
    /// The playhead where it ended.
    pub end: Ticks,
    pub events: Vec<TakeEvent>,
}

impl Take {
    /// Whether anything was played. An empty take makes no clip and no file.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The last tick the clip has to cover: its own end, or one tick past the last message
    /// when the recording was stopped before the engine had passed it.
    fn last_tick(&self) -> Ticks {
        let last = self.events.iter().map(|event| event.tick).max();
        let past_last = last.map_or(Ticks(0), |tick| Ticks(tick.0 + 1));
        self.end.max(past_last).max(Ticks(self.start.0 + 1))
    }

    /// The clip of this take, at the place it was played.
    ///
    /// - A note is saved at the tick the engine sounded it, so playing the clip renders what
    ///   was heard.
    /// - A note that was still held at the end ends there.
    /// - A note off with no note on before it, from a key that was already down when recording
    ///   began, is left out: half a note is not music.
    /// - A pedal move that changes nothing is left out, because some pedals send a stream of
    ///   the same value.
    ///
    /// `None` when nothing was played.
    pub fn clip(&self) -> Option<Clip> {
        if self.is_empty() {
            return None;
        }
        let end = self.last_tick();
        let length = Length::at_least_one(Ticks(end.0.saturating_sub(self.start.0)));
        // Note ons waiting for their off, oldest first, so two presses of one pitch get their
        // own off in the order they were played.
        let mut held: Vec<(Pitch, Ticks, sound_notes::Velocity)> = Vec::new();
        let mut notes = Vec::new();
        let mut pedal = Vec::new();
        let mut pedal_value = Pedal::UP;
        for event in &self.events {
            let start = event.tick.0.saturating_sub(self.start.0);
            match event.played {
                Played::On { pitch, velocity } => held.push((pitch, Ticks(start), velocity)),
                Played::Off { pitch, .. } => {
                    let Some(index) = held.iter().position(|(held, ..)| *held == pitch) else {
                        continue;
                    };
                    let (pitch, note_start, velocity) = held.remove(index);
                    notes.push(Note {
                        start: note_start,
                        length: Length::at_least_one(Ticks(start.saturating_sub(note_start.0))),
                        pitch,
                        velocity,
                    });
                }
                Played::Pedal(value) => {
                    if value != pedal_value {
                        pedal_value = value;
                        pedal.push(PedalChange {
                            start: Ticks(start),
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
                length: Length::at_least_one(Ticks(clip_end.0.saturating_sub(start.0))),
                pitch,
                velocity,
            });
        }
        // The agent doc asks an agent to keep notes in this order, so the runtime writes them
        // that way too.
        notes.sort_by_key(|note| (note.start, note.pitch));
        let mut clip = Clip::new(self.start, length, notes);
        clip.pedal = pedal;
        Some(clip)
    }

    /// Writes the raw take next to the clip it became. Called once, when the take ends.
    /// Nothing writes it again, and nothing deletes it, not even undo.
    pub fn write(&self, root: &Path, clip: &InstanceId) -> io::Result<PathBuf> {
        let path = take_path(root, clip);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(&path, self.json(clip)?)?;
        Ok(path)
    }

    /// The take as it is written: one message per line, like a note of a clip, so a minute of
    /// playing is a file an agent can read and git can show.
    pub fn json(&self, clip: &InstanceId) -> serde_json::Result<String> {
        let mut lines = Vec::with_capacity(self.events.len());
        for event in &self.events {
            lines.push(serde_json::to_string(&RawEvent::of(event))?);
        }
        Ok(format!(
            "{{\n  \"clip\": {},\n  \"start_tick\": {},\n  \"end_tick\": {},\n  \"events\": [\n    {}\n  ]\n}}\n",
            serde_json::to_string(clip.as_str())?,
            self.start.0,
            self.last_tick().0,
            lines.join(",\n    ")
        ))
    }
}

/// The saved form of a take. An agent reads it; nothing reads it back into the runtime yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTake {
    /// The clip this take was recorded into, by its instance id.
    pub clip: String,
    /// The playhead where recording began, in ticks.
    pub start_tick: u64,
    /// Where it ended.
    pub end_tick: u64,
    pub events: Vec<RawEvent>,
}

/// One message of a saved take. `time_us` counts from the moment recording began.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum RawEvent {
    On {
        time_us: u64,
        pitch: u8,
        velocity: u8,
    },
    Off {
        time_us: u64,
        pitch: u8,
        /// How fast the key came up, 0 to 127. Most keyboards send 0 or 64.
        velocity: u8,
    },
    Pedal {
        time_us: u64,
        value: u8,
    },
}

impl RawEvent {
    fn of(event: &TakeEvent) -> Self {
        let time_us = event.time_us;
        match event.played {
            Played::On { pitch, velocity } => Self::On {
                time_us,
                pitch: pitch.number(),
                velocity: velocity.value(),
            },
            Played::Off { pitch, velocity } => Self::Off {
                time_us,
                pitch: pitch.number(),
                velocity,
            },
            Played::Pedal(value) => Self::Pedal {
                time_us,
                value: value.value(),
            },
        }
    }
}
