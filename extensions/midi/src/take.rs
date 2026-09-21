//! The raw take: what arrived while recording, in real time, and the clip it becomes.
//!
//! The take is the record of the performance and the clip is the edit of it. The clip holds
//! the tick at which the engine sounded each message, so playing it back renders what was
//! heard. The take holds the times as they arrived and both velocities of every note, which
//! the clip has no room for.
//!
//! A take has a name of its own, never one made from a clip: a clip id changes when the clip
//! moves to another track or an agent renames its file, and then nothing would find the take
//! again. The clip names its take in a saved field instead, which travels with it through
//! every edit. A name is never used twice, and the file is created and never opened again, so
//! nothing this program does can write over a performance.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sound_core::Ticks;
use sound_notes::{Clip, Length, Note, Pedal, PedalChange, Pitch};

use crate::keys::Played;

/// Where raw takes live in the project folder.
pub const TAKES_FOLDER: &str = "assets/takes";

/// Names are `take-1`, `take-2` and so on. The runtime never removes one, so the next free
/// number is always past every take this project ever made, also past the ones an undo took
/// the clip of.
const NAME: &str = "take";

/// The file of the raw take called `name`.
pub fn take_path(root: &Path, name: &str) -> PathBuf {
    root.join(TAKES_FOLDER).join(format!("{name}.json"))
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
    /// Where the sustain pedal stood when recording began. A take that begins under a held
    /// pedal plays back as it sounded, though the keys that were already down are left out.
    pub pedal_at_start: Pedal,
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

    /// Where the pedal stands at the end of the take.
    fn pedal_at_end(&self) -> Pedal {
        let last = self
            .events
            .iter()
            .rev()
            .find_map(|event| match event.played {
                Played::Pedal(value) => Some(value),
                _ => None,
            });
        last.unwrap_or(self.pedal_at_start)
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
    /// - A take that began under a held pedal starts with that value, so it plays back as it
    ///   sounded, and one that ends with the pedal down lifts it at its end, as a held note
    ///   ends there. Without the lift a clip would sustain for the rest of the piece.
    ///
    /// `None` when nothing was played.
    pub fn clip(&self) -> Option<Clip> {
        if self.is_empty() {
            return None;
        }
        // The clip is one tick longer when it ends under the pedal, so that the lift fits
        // inside it: a pedal move, like a note, starts inside its clip.
        let lifts = self.pedal_at_end().is_down();
        let end = Ticks(self.last_tick().0 + u64::from(lifts));
        let length = Length::at_least_one(Ticks(end.0.saturating_sub(self.start.0)));
        // Note ons waiting for their off, oldest first, so two presses of one pitch get their
        // own off in the order they were played.
        let mut held: Vec<(Pitch, Ticks, sound_notes::Velocity)> = Vec::new();
        let mut notes = Vec::new();
        let mut pedal = Vec::new();
        let mut pedal_value = Pedal::UP;
        if self.pedal_at_start.is_down() {
            pedal_value = self.pedal_at_start;
            pedal.push(PedalChange {
                start: Ticks(0),
                value: pedal_value,
            });
        }
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
        let mut clip = Clip::new(self.start, length, notes);
        clip.pedal = pedal;
        Some(clip)
    }

    /// Writes the raw take under a name of its own and gives that name, for the clip to keep.
    ///
    /// The file is created, never opened: a name that is taken is never written to, so no
    /// performance this program has written can be lost, whatever happened to its clip. The
    /// runtime never writes it again and never removes it, not even on undo.
    pub fn write(&self, root: &Path) -> io::Result<String> {
        let folder = root.join(TAKES_FOLDER);
        std::fs::create_dir_all(&folder)?;
        let json = self.json();
        let mut number = 1_u32;
        loop {
            let name = format!("{NAME}-{number}");
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(folder.join(format!("{name}.json")));
            match file {
                Ok(mut file) => {
                    use std::io::Write as _;
                    file.write_all(json.as_bytes())?;
                    return Ok(name);
                }
                // Taken, by a take of this session or of an earlier one. Never written to.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => number += 1,
                Err(error) => return Err(error),
            }
        }
    }

    /// The take as it is written: one message per line, like a note of a clip, so a minute of
    /// playing is a file an agent can read and git can show.
    pub fn json(&self) -> String {
        let lines: Vec<String> = self
            .events
            .iter()
            .map(|event| RawEvent::of(event).json())
            .collect();
        format!(
            "{{\n  \"start_tick\": {},\n  \"end_tick\": {},\n  \"pedal_at_start\": {},\n  \"events\": [\n    {}\n  ]\n}}\n",
            self.start.0,
            self.last_tick().0,
            self.pedal_at_start.value(),
            lines.join(",\n    ")
        )
    }
}

/// The saved form of a take. An agent reads it; nothing reads it back into the runtime yet.
///
/// It names no clip: the take is written before the clip is made, and a clip id changes. The
/// clip names the take.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawTake {
    /// The playhead where recording began, in ticks.
    pub start_tick: u64,
    /// Where it ended.
    pub end_tick: u64,
    /// Where the sustain pedal stood when recording began, 0 to 127.
    pub pedal_at_start: u8,
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

    /// One line of the file. It cannot fail: every field is a number or a fixed name.
    fn json(self) -> String {
        match self {
            Self::On {
                time_us,
                pitch,
                velocity,
            } => format!(
                r#"{{"kind":"on","time_us":{time_us},"pitch":{pitch},"velocity":{velocity}}}"#
            ),
            Self::Off {
                time_us,
                pitch,
                velocity,
            } => format!(
                r#"{{"kind":"off","time_us":{time_us},"pitch":{pitch},"velocity":{velocity}}}"#
            ),
            Self::Pedal { time_us, value } => {
                format!(r#"{{"kind":"pedal","time_us":{time_us},"value":{value}}}"#)
            }
        }
    }
}
