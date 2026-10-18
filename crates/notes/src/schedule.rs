//! What a performer plays at frames of engine time, kept until its frame comes: the notes and
//! triggers a tool's code sends with `at`, a little ahead of the sound.

use crate::{NoteEvent, Pitch, Velocity};

/// What a performer plays. Give `at` as now or later: a processor made since the last block
/// does not know where engine time is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Performed {
    /// Holds a key for `frames` from the frame `at` of engine time, or from the next frame once
    /// that has passed.
    Play {
        pitch: Pitch,
        velocity: Velocity,
        at: u64,
        frames: u64,
    },
    /// Sends one event at the frame `at`, or in the next frame once that has passed.
    Send { event: NoteEvent, at: u64 },
}

/// What a [`Schedule`] holds: a note, or more, such as a trigger.
pub trait Scheduled: Copy + From<NoteEvent> {
    fn note(&self) -> Option<NoteEvent>;
}

impl Scheduled for NoteEvent {
    fn note(&self) -> Option<NoteEvent> {
        Some(*self)
    }
}

/// What waits for its frame of engine time, the latest first. It never grows past the room it
/// is made with, so the audio thread does not allocate.
pub struct Schedule<T> {
    waiting: Vec<(u64, T)>,
    room: usize,
    /// The first frame of the next block: what is asked for earlier plays there.
    next_frame: u64,
}

impl<T: Scheduled> Schedule<T> {
    pub fn new(room: usize) -> Self {
        Self {
            waiting: Vec::with_capacity(room),
            room,
            next_frame: 0,
        }
    }

    /// The block of `frames` from `start` plays now.
    pub fn playing(&mut self, start: u64, frames: usize) {
        self.next_frame = start + frames as u64;
    }

    /// Keeps `what` for the frame `at`, or the next block when there is none or it has passed,
    /// after what is already there for that frame. A full schedule drops it, and says so.
    pub fn add(&mut self, at: Option<u64>, what: T) -> bool {
        if self.waiting.len() == self.room {
            return false;
        }
        let at = at.unwrap_or(0).max(self.next_frame);
        let index = self.waiting.partition_point(|(other, _)| *other > at);
        self.waiting.insert(index, (at, what));
        true
    }

    /// Keeps what a performer plays for its frame. A full schedule drops a note on, which
    /// leaves nothing sounding, and gives back any other event to play now: a dropped off or
    /// pedal up would hold notes for ever. An off that plays now takes the waiting note and end
    /// of its key along, as the end would end a newer note.
    #[must_use]
    pub fn perform(&mut self, performed: Performed) -> Option<NoteEvent> {
        match performed {
            Performed::Play {
                pitch,
                velocity,
                at,
                frames,
            } => {
                // Both or neither, so no key stays down.
                if self.waiting.len() + 2 <= self.room {
                    let start = at.max(self.next_frame);
                    let end = start.saturating_add(frames.max(1));
                    self.add(Some(start), NoteEvent::On { pitch, velocity }.into());
                    self.add(Some(end), NoteEvent::Off { pitch }.into());
                }
                None
            }
            Performed::Send { event, at } => {
                if self.add(Some(at), event.into()) {
                    return None;
                }
                match event {
                    NoteEvent::On { .. } => None,
                    NoteEvent::Off { pitch } => {
                        self.waiting.retain(|(_, what)| match what.note() {
                            Some(
                                NoteEvent::On { pitch: waiting, .. }
                                | NoteEvent::Off { pitch: waiting },
                            ) => waiting != pitch,
                            _ => true,
                        });
                        Some(event)
                    }
                    _ => Some(event),
                }
            }
        }
    }

    /// The frame of what comes next.
    pub fn next(&self) -> Option<u64> {
        self.waiting.last().map(|(at, _)| *at)
    }

    /// What is due at `frame`, the earliest first, with its frame.
    pub fn take(&mut self, frame: u64) -> Option<(u64, T)> {
        self.waiting.pop_if(|(at, _)| *at <= frame)
    }

    /// Puts back what [`Self::take`] gave, to be taken first again.
    pub fn put_back(&mut self, taken: (u64, T)) {
        self.waiting.push(taken);
    }
}
