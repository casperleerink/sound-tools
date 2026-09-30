//! The wheels and the key pressure: where they stand, and what the bundled instruments do with
//! them.

use sound_core::{Lfo, LfoShape};

use crate::{Amount, Bend, NoteEvent};

/// Where the wheels and the key pressure of an instrument stand. They belong to the instrument
/// and not to a note: every note follows them, also one that starts after they moved.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Expression {
    pub bend: Bend,
    pub mod_wheel: Amount,
    pub pressure: Amount,
}

impl Expression {
    /// Where everything stands before the first move and after every `AllOff`.
    pub const REST: Self = Self {
        bend: Bend::MIDDLE,
        mod_wheel: Amount::NONE,
        pressure: Amount::NONE,
    };

    /// Follows one event. A wheel or the pressure moves, `AllOff` puts everything at rest, and
    /// a note or the pedal changes nothing.
    pub fn follow(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::Bend(bend) => self.bend = bend,
            NoteEvent::ModWheel(amount) => self.mod_wheel = amount,
            NoteEvent::Pressure(amount) => self.pressure = amount,
            NoteEvent::AllOff => *self = Self::REST,
            NoteEvent::On { .. } | NoteEvent::Off { .. } | NoteEvent::Pedal(_) => {}
        }
    }

    pub fn is_at_rest(&self) -> bool {
        *self == Self::REST
    }
}

/// What the bundled instruments do with the wheels, so they all play the same: the bend wheel
/// moves every note up to [`BEND_SEMITONES`](Self::BEND_SEMITONES) either way, and the
/// modulation wheel adds a vibrato up to [`VIBRATO_SEMITONES`](Self::VIBRATO_SEMITONES) deep.
/// The pressure is kept and not used.
///
/// The pitch moves in steps, once per stretch of frames an instrument renders, which is at most
/// one block. A voice keeps its phase through a step, so it is not heard as a click.
#[derive(Copy, Clone, Debug, Default)]
pub struct Wheels {
    expression: Expression,
    vibrato: Lfo,
}

impl Wheels {
    /// The MIDI default and what most keyboards expect.
    pub const BEND_SEMITONES: f32 = 2.0;
    /// Either way, with the wheel all the way up.
    pub const VIBRATO_SEMITONES: f32 = 0.5;
    pub const VIBRATO_HZ: f32 = 5.5;

    pub fn follow(&mut self, event: NoteEvent) {
        self.expression.follow(event);
    }

    pub fn expression(&self) -> Expression {
        self.expression
    }

    /// How far every note is moved for the next `frames`, as a ratio of its frequency. Moves
    /// the vibrato on by those frames.
    pub fn pitch_ratio(&mut self, frames: usize, sample_rate: f32) -> f32 {
        let bend = self.expression.bend.fraction() * Self::BEND_SEMITONES;
        let depth = self.expression.mod_wheel.fraction() * Self::VIBRATO_SEMITONES;
        let vibrato = depth * self.vibrato.value(LfoShape::Sine, 0.0);
        self.vibrato.advance(frames, Self::VIBRATO_HZ, sample_rate);
        ((bend + vibrato) / 12.0).exp2()
    }
}
