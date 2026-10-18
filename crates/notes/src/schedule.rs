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
    /// Note ons a full schedule dropped, for the processor to count.
    dropped: u64,
}

impl<T: Scheduled> Schedule<T> {
    pub fn new(room: usize) -> Self {
        Self {
            waiting: Vec::with_capacity(room),
            room,
            next_frame: 0,
            dropped: 0,
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
    /// pedal up would hold notes for ever. What waits that would undo it goes: an off takes the
    /// note and end of its key along, as the end would end a newer note; a controller its older
    /// moves; `AllOff` everything older. `AllOff` also drops the notes sent before it to play
    /// after it.
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
                } else {
                    self.dropped += 1;
                }
                None
            }
            Performed::Send { event, at } => {
                if let NoteEvent::AllOff = event {
                    // A stop: what was sent to play after it would play on.
                    let at = at.max(self.next_frame);
                    (self.waiting)
                        .retain(|(waiting_at, what)| *waiting_at <= at || what.note().is_none());
                }
                if self.add(Some(at), event.into()) {
                    return None;
                }
                if let NoteEvent::On { .. } = event {
                    self.dropped += 1;
                    return None;
                }
                self.waiting.retain(|(waiting_at, what)| {
                    !what
                        .note()
                        .is_some_and(|older| undoes(older, *waiting_at, (event, at)))
                });
                Some(event)
            }
        }
    }

    /// How many note ons it dropped since the last call.
    pub fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
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

/// Whether `waiting`, kept for `waiting_at`, would undo `played` if it played after it: the
/// note or end of its key for an off, an older move of the same controller, or anything older
/// for `AllOff`.
fn undoes(waiting: NoteEvent, waiting_at: u64, (played, at): (NoteEvent, u64)) -> bool {
    use NoteEvent::{AllOff, Bend, Off, On, Pressure};
    match (played, waiting) {
        (Off { pitch }, On { pitch: key, .. } | Off { pitch: key }) => key == pitch,
        _ if waiting_at > at => false,
        (AllOff, _) => true,
        (Bend(_), Bend(_)) | (Pressure(_), Pressure(_)) => true,
        _ => played
            .as_controller()
            .zip(waiting.as_controller())
            .is_some_and(|((played, _), (waiting, _))| played == waiting),
    }
}
