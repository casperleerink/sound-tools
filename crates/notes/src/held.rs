//! What holds the notes a feed passes to one instrument, from a performer and from the other
//! senders.
//!
//! An off releases every note of its pitch, and several senders may hold one pitch, such as two
//! clips that overlap, or a clip and a key. So it counts the ons of each pitch and passes an
//! off on only when it ends the last of them.
//!
//! It counts the performer's ons apart from the others': a performer that goes away, such as
//! code that restarts, sends no off for what it holds, nor lifts the pedal. An off ends only the
//! ons of its own side.

use crate::{NoteEvent, Pedal, Pitch};

pub(crate) struct Held {
    /// How many ons of each pitch reached the instrument and have not ended: the performer's,
    /// and the other senders'. Wide enough that no count saturates.
    ons: [(u64, u64); 128],
    /// Whether the performer last put the pedal down, and whether the other senders did.
    pedal: (bool, bool),
}

impl Default for Held {
    fn default() -> Self {
        Self {
            ons: [(0, 0); 128],
            pedal: (false, false),
        }
    }
}

impl Held {
    /// Passes `event`, from the performer or not, to `send` unless it is an off another on of
    /// its pitch still holds, and counts it once `send` takes it. False when `send` did not.
    pub(crate) fn pass(
        &mut self,
        event: NoteEvent,
        performed: bool,
        send: impl FnOnce(NoteEvent) -> bool,
    ) -> bool {
        match event {
            NoteEvent::Off { pitch } => match self.ons(pitch, performed) {
                // An off of a note its side never played.
                (0, _) => return true,
                (1, 0) => {}
                (own, _) => {
                    *own -= 1;
                    return true;
                }
            },
            NoteEvent::AllOff => *self = Self::default(),
            NoteEvent::Pedal(pedal) if performed => self.pedal.0 = pedal.is_down(),
            NoteEvent::Pedal(pedal) => self.pedal.1 = pedal.is_down(),
            _ => {}
        }
        let sent = send(event);
        match event {
            NoteEvent::On { pitch, .. } if sent => *self.ons(pitch, performed).0 += 1,
            // A performer sends an off that did not fit again; another sender does not.
            NoteEvent::Off { pitch } if sent || !performed => *self.ons(pitch, performed).0 = 0,
            _ => {}
        }
        sent
    }

    /// The ons of `pitch` of the side `performed` names, and those of the other side.
    fn ons(&mut self, pitch: Pitch, performed: bool) -> (&mut u64, &mut u64) {
        let (performer, others) = &mut self.ons[usize::from(pitch.number())];
        if performed {
            (performer, others)
        } else {
            (others, performer)
        }
    }

    /// Forgets what the performer holds, and gives `release` what ends it: the pedal up when no
    /// other sender holds it down, then an off for each pitch no other sender holds.
    pub(crate) fn performer_gone(&mut self, mut release: impl FnMut(NoteEvent)) {
        if std::mem::take(&mut self.pedal.0) && !self.pedal.1 {
            release(NoteEvent::Pedal(Pedal::UP));
        }
        for (number, (performer, others)) in (0..).zip(&mut self.ons) {
            if std::mem::take(performer) > 0 && *others == 0 {
                release(NoteEvent::Off {
                    pitch: Pitch::nearest(number),
                });
            }
        }
    }
}
