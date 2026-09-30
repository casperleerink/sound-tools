//! The note contract: what a tool that sends notes and a tool that plays them agree on.
//!
//! Both sides depend on this crate and not on each other. It holds the saved [`Note`] and
//! [`Clip`] with its expression lanes ([`Point`]), the saved [`RawTake`] a recording writes and
//! a fit reads, the realtime [`NoteEvent`] with the wheels an instrument follows
//! ([`Expression`]), and the port names of
//! an instrument and an effect. The port names live here and not in a crate of their own,
//! because both sides of a track already read this one.
//!
//! It also holds how the bundled instruments play notes, [`Voices`] and [`Wheels`], so they
//! all play them the same way, and the note lengths a time that follows the tempo is picked
//! in, [`Division`] and [`Feel`]. Extensions never depend on each other, so this is the one place
//! they share.
//!
//! Rules for a sender of notes, which no type enforces:
//!
//! - Send [`NoteEvent::AllOff`] at offset 0 when the transport says `stopped_playing` or
//!   `jumped`, before the notes of that block. So no note is ever stuck, whoever sent it.
//! - On one frame, send the pedal and the wheels first, then the offs, then the ons. Else an
//!   off does not see where the pedal stands, a note does not start where the wheels stand, or
//!   the end of one note releases the next note of the same pitch.
//! - A sender whose notes can change while they sound keeps a fixed list of what it started,
//!   and sends the offs from that list. `extensions/arrangement/src/sequencer.rs` does this.
//!
//! A known limit, accepted: `AllOff` releases everything an instrument holds, also the keys
//! held on a MIDI keyboard, because live input plays into the same port, and it puts the wheels
//! back at rest while a hand may still hold one. That is what makes "no note and no bend is ever
//! stuck" a property of the contract and not of every sender.

mod division;
mod expression;
mod lane;
mod take;
mod voices;

use serde::{Deserialize, Serialize};
use sound_core::{Place, State, Ticks};

pub use division::{Division, Feel};
pub use expression::{Expression, Wheels};
pub use lane::{LaneValue, Point, cut, thinned, value_at};
pub use take::{
    MAX_PROJECT_MICROS, MAX_TAKE_MICROS, RawEvent, RawTake, TAKES_FOLDER, TakeError, take_asset,
};
pub use voices::{Voice, Voices, frequency_hz};

/// The event input of an instrument. It carries [`NoteEvent`].
pub const NOTES_INPUT: &str = "notes";

/// The tool that owns clips. Named here because the clip record is: see [`Clip`].
pub const TRACK_TOOL: &str = "arrangement.track";

/// The audio output of an instrument or an effect. Stereo, like every audio port.
pub const AUDIO_OUTPUT: &str = "audio";

/// The audio input of an effect. A tool with this input and [`AUDIO_OUTPUT`] is an effect: the
/// sound of whatever comes before it goes in here and what it makes comes out there.
///
/// It has the same name as the output because inputs and outputs are named apart. So a chain
/// reads as `audio` to `audio`, and no tool has to invent a name for the one thing it takes.
pub const AUDIO_INPUT: &str = "audio";

#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NoteError {
    #[error("pitch must be from 0 to 127, not {0}")]
    Pitch(i64),
    #[error("velocity must be from 1 to 127, not {0}")]
    Velocity(i64),
    #[error("length must be 1 tick or more, not 0")]
    Length,
    #[error("pedal must be from 0 to 127, not {0}")]
    Pedal(i64),
    #[error("bend must be from -8192 to 8191, not {0}")]
    Bend(i64),
    #[error("amount must be from 0 to 127, not {0}")]
    Amount(i64),
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

    /// The pitch nearest to any number: 0 below the range, 127 above it. For interface math
    /// that cannot fail, such as a drag or a transposition that stops at the ends.
    pub fn nearest(number: i64) -> Self {
        Self(number.clamp(0, 127) as u8)
    }

    pub fn number(self) -> u8 {
        self.0
    }

    /// Twelve equal steps per octave, with A4 at 440 Hz.
    pub fn frequency_hz(self) -> f32 {
        frequency_hz(f32::from(self.0))
    }

    /// The name of the note in the app: `C4` is 60, `A4` is 69, `C#4` is 61, `C-1` is 0.
    pub fn name(self) -> String {
        const NAMES: [&str; 12] = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let octave = i32::from(self.0 / 12) - 1;
        format!("{}{octave}", NAMES[usize::from(self.0 % 12)])
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

    /// The velocity nearest to any number: 1 below the range, 127 above it.
    pub fn nearest(value: i64) -> Self {
        Self(value.clamp(1, 127) as u8)
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

/// How far the sustain pedal is pressed, 0 to 127, as it was played.
///
/// It is a number and not a bool because a piano that knows half pedal must lose nothing. The
/// synth of this repository only asks [`is_down`](Self::is_down), which is MIDI's rule: down
/// from [`DOWN_FROM`](Self::DOWN_FROM).
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "i64", into = "u8")]
pub struct Pedal(u8);

impl Pedal {
    /// The pedal fully up. What an instrument holds before any pedal event.
    pub const UP: Self = Self(0);

    /// From this value the pedal counts as down, as MIDI says for controller 64.
    pub const DOWN_FROM: u8 = 64;

    pub fn new(value: u8) -> Result<Self, NoteError> {
        Self::try_from(i64::from(value))
    }

    /// The pedal value nearest to any number: 0 below the range, 127 above it.
    pub fn nearest(value: i64) -> Self {
        Self(value.clamp(0, 127) as u8)
    }

    pub fn value(self) -> u8 {
        self.0
    }

    /// Whether an instrument holds the notes whose key is released.
    pub fn is_down(self) -> bool {
        self.0 >= Self::DOWN_FROM
    }
}

impl TryFrom<i64> for Pedal {
    type Error = NoteError;

    fn try_from(value: i64) -> Result<Self, NoteError> {
        match u8::try_from(value) {
            Ok(value @ 0..=127) => Ok(Self(value)),
            _ => Err(NoteError::Pedal(value)),
        }
    }
}

impl From<Pedal> for u8 {
    fn from(pedal: Pedal) -> u8 {
        pedal.0
    }
}

/// Where the pitch bend wheel stands, -8192 to 8191, with 0 in the middle: MIDI's 14 bits with
/// the middle moved to 0. How far a full bend goes is the instrument's choice.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "i64", into = "i16")]
pub struct Bend(i16);

impl Bend {
    /// The wheel in the middle, where it springs back to: no bend.
    pub const MIDDLE: Self = Self(0);

    pub fn new(value: i16) -> Result<Self, NoteError> {
        Self::try_from(i64::from(value))
    }

    /// The bend nearest to any number: -8192 below the range, 8191 above it.
    pub fn nearest(value: i64) -> Self {
        Self(value.clamp(-8192, 8191) as i16)
    }

    pub fn value(self) -> i16 {
        self.0
    }

    /// From -1 all the way down to 1 all the way up. MIDI has one step more below the middle
    /// than above it, so each side is divided by its own length and both ends are a full bend.
    pub fn fraction(self) -> f32 {
        match self.0 {
            value if value < 0 => f32::from(value) / 8192.0,
            value => f32::from(value) / 8191.0,
        }
    }
}

impl TryFrom<i64> for Bend {
    type Error = NoteError;

    fn try_from(value: i64) -> Result<Self, NoteError> {
        match i16::try_from(value) {
            Ok(value @ -8192..=8191) => Ok(Self(value)),
            _ => Err(NoteError::Bend(value)),
        }
    }
}

impl From<Bend> for i16 {
    fn from(bend: Bend) -> i16 {
        bend.0
    }
}

/// How far the modulation wheel is turned or how hard the keys are pressed, 0 to 127, as MIDI
/// sends it. What it does is the instrument's choice.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(try_from = "i64", into = "u8")]
pub struct Amount(u8);

impl Amount {
    /// Nothing: the wheel all the way down, no pressure on the keys.
    pub const NONE: Self = Self(0);

    pub fn new(value: u8) -> Result<Self, NoteError> {
        Self::try_from(i64::from(value))
    }

    /// The amount nearest to any number: 0 below the range, 127 above it.
    pub fn nearest(value: i64) -> Self {
        Self(value.clamp(0, 127) as u8)
    }

    pub fn value(self) -> u8 {
        self.0
    }

    /// From 0 to 1.
    pub fn fraction(self) -> f32 {
        f32::from(self.0) / 127.0
    }
}

impl TryFrom<i64> for Amount {
    type Error = NoteError;

    fn try_from(value: i64) -> Result<Self, NoteError> {
        match u8::try_from(value) {
            Ok(value @ 0..=127) => Ok(Self(value)),
            _ => Err(NoteError::Amount(value)),
        }
    }
}

impl From<Amount> for u8 {
    fn from(amount: Amount) -> u8 {
        amount.0
    }
}

/// How long a note or a clip is: 1 tick or more.
///
/// A note of no length would get its off before its on, because offs go first on a frame, and
/// would sound until the next `AllOff`. A clip of no length could hold no note. So it cannot
/// be built.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "Ticks", into = "Ticks")]
pub struct Length(Ticks);

impl Length {
    pub fn new(ticks: Ticks) -> Result<Self, NoteError> {
        Self::try_from(ticks)
    }

    /// The length nearest to `ticks`: one tick for 0. For interface math that cannot fail,
    /// such as a drag that already keeps a minimum.
    pub fn at_least_one(ticks: Ticks) -> Self {
        Self(ticks.max(Ticks(1)))
    }

    pub fn ticks(self) -> Ticks {
        self.0
    }
}

impl TryFrom<Ticks> for Length {
    type Error = NoteError;

    fn try_from(ticks: Ticks) -> Result<Self, NoteError> {
        if ticks == Ticks(0) {
            return Err(NoteError::Length);
        }
        Ok(Self(ticks))
    }
}

impl From<Length> for Ticks {
    fn from(length: Length) -> Ticks {
        length.0
    }
}

/// A saved note. `start` counts from the start of whatever holds the note, for example a clip.
/// Ordered by start, then length, pitch and velocity, so an interface can keep notes in a set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub start: Ticks,
    pub length: Length,
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

/// A saved move of the sustain pedal. `start` counts from the start of the clip, like a note.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PedalChange {
    pub start: Ticks,
    pub value: Pedal,
}

/// A pedal move at its place on the project timeline.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlacedPedal {
    pub start: Ticks,
    pub value: Pedal,
}

/// A saved clip: a stretch of the timeline of whatever owns it, with the notes that play in it.
/// This is the record other extensions read, so it lives here and not in the arrangement.
///
/// The rules, the same for everyone who plays or draws a clip:
/// - `start` is a position on the project timeline. Note starts count from the clip start.
/// - Every note starts inside the clip. A record with a note at or after `length` does not
///   load, so a note written with a timeline position instead is an error and not silence.
/// - A note that is longer than the rest of the clip ends where the clip ends.
/// - `pedal` is the sustain pedal as it was played, and follows the same rules as the notes.
///   A clip that was not recorded leaves it out, and is written back without it.
/// - `bend`, `mod_wheel` and `pressure` are the expression lanes: points inside the clip, in
///   tick order, one per tick, read with [`value_at`]. An empty lane is left out of the file
///   and moves nothing; a lane with points holds for the whole clip, and ends with it.
/// - `take` names the raw take this clip was recorded from, when it was recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub start: Ticks,
    pub length: Length,
    pub notes: Vec<Note>,
    /// Left out of a clip with no pedal, so a clip of before the pedal existed loads unchanged
    /// and is written back byte for byte as it was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pedal: Vec<PedalChange>,
    /// The bend wheel. Left out when empty, like the pedal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bend: Vec<Point<Bend>>,
    /// The modulation wheel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mod_wheel: Vec<Point<Amount>>,
    /// The key pressure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pressure: Vec<Point<Amount>>,
    /// The raw take this clip came from: a saved reference, the name of a file under
    /// `assets/takes/` without `.json`. It owns nothing and keeps nothing alive, like every
    /// other reference. A clip that was not recorded leaves it out.
    ///
    /// It is a field and not the path of the clip, because the path changes: a clip moves to
    /// another track, and an agent renames its file. The reference travels with the clip
    /// through every edit, because every edit keeps the rest of the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take: Option<String>,
}

impl Clip {
    /// A clip with notes, no pedal, no lanes and no take.
    pub fn new(start: Ticks, length: Length, notes: Vec<Note>) -> Self {
        Self {
            start,
            length,
            notes,
            pedal: Vec::new(),
            bend: Vec::new(),
            mod_wheel: Vec::new(),
            pressure: Vec::new(),
            take: None,
        }
    }

    /// Whether `name` may name a raw take: it becomes a file name, so it follows the rule of
    /// an instance name. Without this a record could point outside the project folder.
    pub fn is_valid_take_name(name: &str) -> bool {
        !name.is_empty()
            && name.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || "-_".contains(character)
            })
    }

    /// The first tick after the clip.
    pub fn end(&self) -> Ticks {
        self.start + self.length.ticks()
    }

    /// The notes at their place on the project timeline, in saved order.
    pub fn placed_notes(&self) -> impl Iterator<Item = PlacedNote> {
        self.notes.iter().map(|note| PlacedNote {
            start: self.start + note.start,
            end: (self.start + note.end()).min(self.end()),
            pitch: note.pitch,
            velocity: note.velocity,
        })
    }

    /// The pedal moves at their place on the project timeline, in saved order.
    pub fn placed_pedal(&self) -> impl Iterator<Item = PlacedPedal> {
        self.pedal.iter().map(|change| PlacedPedal {
            start: self.start + change.start,
            value: change.value,
        })
    }

    /// Sets the length and drops the notes, pedal moves and lane points that would start
    /// outside, which a clip cannot hold. A lane keeps the value it had at the new end.
    pub fn set_length(&mut self, length: Length) {
        let inside = Ticks(0)..length.ticks();
        self.length = length;
        self.notes.retain(|note| note.start < length.ticks());
        self.pedal.retain(|change| change.start < length.ticks());
        self.bend = cut(&self.bend, inside.clone());
        self.mod_wheel = cut(&self.mod_wheel, inside.clone());
        self.pressure = cut(&self.pressure, inside);
    }
}

impl State for Clip {
    const TOOL: &'static str = "arrangement.clip";
    /// Only a track plays clips. Anywhere else a clip would load and never sound.
    const PLACE: Place = Place::In(TRACK_TOOL);

    fn validate(&self) -> Result<(), String> {
        let length = self.length.ticks();
        let inside = |field: &str, what: &str, index: usize, start: Ticks| {
            format!(
                "{field}[{index}].start must be less than the clip length {}, not {}. A {what} start counts from the start of its clip, not from the start of the project",
                length.0, start.0
            )
        };
        let mut notes = self.notes.iter().enumerate();
        if let Some((index, note)) = notes.find(|(_, note)| note.start >= length) {
            return Err(inside("notes", "note", index, note.start));
        }
        let mut pedal = self.pedal.iter().enumerate();
        if let Some((index, change)) = pedal.find(|(_, change)| change.start >= length) {
            return Err(inside("pedal", "pedal", index, change.start));
        }
        lane::check("bend", &self.bend, length)?;
        lane::check("mod_wheel", &self.mod_wheel, length)?;
        lane::check("pressure", &self.pressure, length)?;
        match &self.take {
            Some(take) if !Self::is_valid_take_name(take) => Err(format!(
                "take must be the name of a file under assets/takes/ without `.json`: lowercase letters, digits, `-` and `_`, not {take:?}"
            )),
            _ => Ok(()),
        }
    }
}

/// A note of a clip on the project timeline. `end` is the tick of the note off, which is at
/// most the end of the clip.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlacedNote {
    pub start: Ticks,
    pub end: Ticks,
    pub pitch: Pitch,
    pub velocity: Velocity,
}

/// What travels from a sender of notes to an instrument, at a frame offset.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NoteEvent {
    On {
        pitch: Pitch,
        velocity: Velocity,
    },
    /// Releases every held note of this pitch, unless the pedal holds it.
    Off {
        pitch: Pitch,
    },
    /// Moves the sustain pedal. While it is down ([`Pedal::is_down`]) an `Off` does not
    /// release: the note sounds on until the pedal comes up.
    Pedal(Pedal),
    /// Moves the pitch bend wheel. Every note of the instrument bends with it, the ones that
    /// sound and the ones that start later, until it moves again.
    Bend(Bend),
    /// Moves the modulation wheel, MIDI controller 1. Like the bend it holds for every note.
    ModWheel(Amount),
    /// How hard the held keys are pressed, MIDI channel pressure. One value for the whole
    /// instrument, like the wheels.
    Pressure(Amount),
    /// Releases every held note, puts the pedal up and the wheels and the pressure at rest
    /// ([`Expression::REST`]), so a sender does not have to track what it started. Send it when
    /// the transport says `stopped_playing` or `jumped`. Release tails still sound.
    AllOff,
}
