//! The computer keyboard as a MIDI keyboard, as in DAWs: the letter rows play notes, and `z`
//! and `x` move them an octave down and up.
//!
//! This only turns key names into messages. The window sends them into the [`Input`] of the
//! MIDI keyboard, so they play, record and reach a tool as a keyboard's messages do.
//!
//! [`Input`]: crate::Input

use sound_notes::{Pitch, Velocity};

use crate::Played;

/// The keys that play, from C up an octave and a half: the middle row are the white keys,
/// the row above it the black ones.
const NOTES: [&str; 18] = [
    "a", "w", "s", "e", "d", "f", "t", "g", "y", "h", "u", "j", "k", "o", "l", "p", ";", "'",
];
const OCTAVE_DOWN: &str = "z";
const OCTAVE_UP: &str = "x";

/// The pitch of `a` before an octave key moves it: middle C.
const MIDDLE_C: i64 = 60;
/// How far the octave keys go each way: `a` down to pitch 0, `'` up to 125. Every key is then
/// a pitch of its own.
const LOWEST_OCTAVE: i64 = -5;
const HIGHEST_OCTAVE: i64 = 4;
/// A computer key has no velocity, so every note gets this one.
const VELOCITY: i64 = 100;
/// What a keyboard without release velocity sends.
const RELEASE_VELOCITY: u8 = 64;

/// Which keys hold a note, and the octave they play in.
#[derive(Debug, Default)]
pub struct ComputerKeys {
    octave: i64,
    /// The pitch each key of [`NOTES`] started and still holds. A key ends that one, also
    /// after an octave key moved the others.
    held: [Option<Pitch>; NOTES.len()],
}

impl ComputerKeys {
    /// Whether the key is one of the keys that play, the octave keys too.
    pub fn plays(key: &str) -> bool {
        NOTES.contains(&key) || key == OCTAVE_DOWN || key == OCTAVE_UP
    }

    /// A key went down: the note it starts. Nothing for a key that holds a note already, as a
    /// held key repeats, and nothing for the octave keys.
    pub fn down(&mut self, key: &str) -> Option<Played> {
        match key {
            OCTAVE_DOWN => self.octave = (self.octave - 1).max(LOWEST_OCTAVE),
            OCTAVE_UP => self.octave = (self.octave + 1).min(HIGHEST_OCTAVE),
            _ => {}
        }
        let step = NOTES.iter().position(|note| *note == key)?;
        let held = self.held.get_mut(step)?;
        if held.is_some() {
            return None;
        }
        let pitch = Pitch::nearest(MIDDLE_C + 12 * self.octave + step as i64);
        *held = Some(pitch);
        Some(Played::On {
            pitch,
            velocity: Velocity::nearest(VELOCITY),
        })
    }

    /// A key came up: the end of the note it started. Nothing when it started none.
    pub fn up(&mut self, key: &str) -> Option<Played> {
        let step = NOTES.iter().position(|note| *note == key)?;
        let pitch = self.held.get_mut(step)?.take()?;
        Some(off(pitch))
    }

    /// The end of every note a key holds, for when the keys stop playing or their key ups
    /// will not come, such as while cmd is down.
    pub fn release(&mut self) -> impl Iterator<Item = Played> + '_ {
        self.held.iter_mut().filter_map(Option::take).map(off)
    }
}

fn off(pitch: Pitch) -> Played {
    Played::Off {
        pitch,
        velocity: RELEASE_VELOCITY,
    }
}
