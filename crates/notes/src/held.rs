//! What holds the notes of one instrument, from a performer and from the other senders.
//!
//! An off releases every note of its pitch, and several senders may hold one pitch, such as two
//! clips that overlap, or a clip and a key. So it counts the ons of each pitch and passes an
//! off on only when it ends the last of them.
//!
//! It counts the performer's ons apart too: a performer that goes away, such as code that
//! restarts, sends no off for what it holds, nor lifts the pedal.

use crate::{NoteEvent, Pedal, Pitch};

pub struct Held {
    /// How many ons of each pitch reached the instrument and have not ended. Wide enough that
    /// no count saturates: a saturated one would let `performer_gone` cut another's note.
    ons: [u64; 128],
    /// How many of `ons` the performer played.
    performer: [u64; 128],
    /// Whether the performer last put the pedal down, and whether the other senders did.
    pedal: (bool, bool),
}

impl Default for Held {
    fn default() -> Self {
        Self {
            ons: [0; 128],
            performer: [0; 128],
            pedal: (false, false),
        }
    }
}

impl Held {
    /// Passes `event`, from the performer or not, to `send` unless it is an off another on of
    /// its pitch still holds, and counts it once `send` takes it. False when `send` did not.
    pub fn pass(
        &mut self,
        event: NoteEvent,
        performed: bool,
        send: impl FnOnce(NoteEvent) -> bool,
    ) -> bool {
        match event {
            NoteEvent::Off { pitch } => {
                let index = usize::from(pitch.number());
                let (ons, performer) = (&mut self.ons[index], &mut self.performer[index]);
                if performed {
                    *performer = performer.saturating_sub(1);
                }
                if *ons > 1 {
                    *ons -= 1;
                    *performer = (*performer).min(*ons);
                    return true;
                }
                (*ons, *performer) = (0, 0);
            }
            NoteEvent::AllOff => *self = Self::default(),
            NoteEvent::Pedal(pedal) if performed => self.pedal.0 = pedal.is_down(),
            NoteEvent::Pedal(pedal) => self.pedal.1 = pedal.is_down(),
            _ => {}
        }
        let sent = send(event);
        if sent && let NoteEvent::On { pitch, .. } = event {
            let index = usize::from(pitch.number());
            self.ons[index] = self.ons[index].saturating_add(1);
            if performed {
                self.performer[index] = self.performer[index].saturating_add(1);
            }
        }
        sent
    }

    /// Forgets what the performer holds, and gives `release` what ends it: the pedal up when no
    /// other sender holds it down, then an off for each pitch no other sender holds.
    pub fn performer_gone(&mut self, mut release: impl FnMut(NoteEvent)) {
        if std::mem::take(&mut self.pedal.0) && !self.pedal.1 {
            release(NoteEvent::Pedal(Pedal::UP));
        }
        for (number, (ons, performer)) in (0..).zip(self.ons.iter_mut().zip(&mut self.performer)) {
            if *performer == 0 {
                continue;
            }
            *ons = ons.saturating_sub(std::mem::take(performer));
            if *ons == 0 {
                release(NoteEvent::Off {
                    pitch: Pitch::nearest(number),
                });
            }
        }
    }
}
