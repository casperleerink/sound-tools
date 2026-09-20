//! The note contract: what a tool that sends notes and a tool that plays them agree on.
//!
//! Both sides depend on this crate and not on each other. It holds the saved [`Note`], the
//! realtime [`NoteEvent`] and the port names of an instrument. `README.md` in this crate is
//! the guide.

use serde::{Deserialize, Serialize};
use sound_core::Ticks;

/// The event input of an instrument. It carries [`NoteEvent`].
pub const NOTES_INPUT: &str = "notes";

/// The audio output of an instrument. Mono for now.
pub const AUDIO_OUTPUT: &str = "audio";

#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NoteError {
    #[error("pitch must be from 0 to 127, not {0}")]
    Pitch(i64),
    #[error("velocity must be from 1 to 127, not {0}")]
    Velocity(i64),
    #[error("length must be 1 tick or more, not 0")]
    Length,
}

/// A MIDI note number, 0 to 127. 60 is middle C and 69 is A4.
///
/// It loads from any JSON integer, so a value out of range gets the message of [`NoteError`]
/// and not a message about integer sizes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "u8")]
pub struct Pitch(u8);

impl Pitch {
    pub fn new(number: u8) -> Result<Self, NoteError> {
        Self::try_from(i64::from(number))
    }

    pub fn number(self) -> u8 {
        self.0
    }

    /// Twelve equal steps per octave, with A4 at 440 Hz.
    pub fn frequency_hz(self) -> f32 {
        440.0 * ((f32::from(self.0) - 69.0) / 12.0).exp2()
    }
}

impl TryFrom<i64> for Pitch {
    type Error = NoteError;

    fn try_from(number: i64) -> Result<Self, NoteError> {
        match u8::try_from(number) {
            Ok(number @ 0..=127) => Ok(Self(number)),
            _ => Err(NoteError::Pitch(number)),
        }
    }
}

impl From<Pitch> for u8 {
    fn from(pitch: Pitch) -> u8 {
        pitch.0
    }
}

/// How hard a note is played, 1 to 127. There is no velocity 0: MIDI uses it for note off.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "u8")]
pub struct Velocity(u8);

impl Velocity {
    pub fn new(value: u8) -> Result<Self, NoteError> {
        Self::try_from(i64::from(value))
    }

    pub fn value(self) -> u8 {
        self.0
    }
}

impl TryFrom<i64> for Velocity {
    type Error = NoteError;

    fn try_from(value: i64) -> Result<Self, NoteError> {
        match u8::try_from(value) {
            Ok(value @ 1..=127) => Ok(Self(value)),
            _ => Err(NoteError::Velocity(value)),
        }
    }
}

impl From<Velocity> for u8 {
    fn from(velocity: Velocity) -> u8 {
        velocity.0
    }
}

/// How long a note is held: 1 tick or more.
///
/// A note of no length would get its off before its on, because offs go first on a frame, and
/// would sound until the next `AllOff`. So it cannot be built.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "Ticks", into = "Ticks")]
pub struct NoteLength(Ticks);

impl NoteLength {
    pub fn new(ticks: Ticks) -> Result<Self, NoteError> {
        Self::try_from(ticks)
    }

    pub fn ticks(self) -> Ticks {
        self.0
    }
}

impl TryFrom<Ticks> for NoteLength {
    type Error = NoteError;

    fn try_from(ticks: Ticks) -> Result<Self, NoteError> {
        if ticks == Ticks(0) {
            return Err(NoteError::Length);
        }
        Ok(Self(ticks))
    }
}

impl From<NoteLength> for Ticks {
    fn from(length: NoteLength) -> Ticks {
        length.0
    }
}

/// A saved note. `start` counts from the start of whatever holds the note, for example a clip.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub start: Ticks,
    pub length: NoteLength,
    pub pitch: Pitch,
    pub velocity: Velocity,
}

impl Note {
    /// The tick of the note off.
    pub fn end(&self) -> Ticks {
        self.start + self.length.ticks()
    }

    pub fn on(&self) -> NoteEvent {
        NoteEvent::On {
            pitch: self.pitch,
            velocity: self.velocity,
        }
    }

    pub fn off(&self) -> NoteEvent {
        NoteEvent::Off { pitch: self.pitch }
    }
}

/// What travels from a sender of notes to an instrument, at a frame offset.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NoteEvent {
    On {
        pitch: Pitch,
        velocity: Velocity,
    },
    /// Releases every held note of this pitch.
    Off {
        pitch: Pitch,
    },
    /// Releases every held note, so a sender does not have to track what it started. Send it
    /// when the transport says `stopped_playing` or `jumped`. Release tails still sound.
    AllOff,
}
