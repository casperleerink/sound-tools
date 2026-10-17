//! Patterns a [`NotePlayer`](crate::NotePlayer) launches: notes that loop from the tick they
//! were launched on.
//!
//! A launched pattern is only its anchor, the tick it was launched on. Where it is at any tick
//! follows from that, so a seek, the loop of the transport and a tempo change need no case of
//! their own.

use std::ops::Range;

use sound_core::{Ticks, TimeSignatures};

use crate::timeline::{end_of, sort};
use crate::{Length, Note, Pitch, PlacedNote};

#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    #[error(
        "notes[{index}].start must be less than the pattern length {length}, not {start}. A note start counts from the start of its pattern"
    )]
    NoteOutside {
        index: usize,
        start: u64,
        length: u64,
    },
}

/// Notes that loop. A note that is longer than the rest of the pattern ends where the pattern
/// ends, as in a clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    length: Length,
    /// Sorted, with starts and ends counted from the start of the pattern.
    notes: Vec<PlacedNote>,
}

impl Pattern {
    /// Fails when a note starts at or after `length`.
    pub fn new(length: Length, notes: &[Note]) -> Result<Self, PatternError> {
        let end = length.ticks();
        let mut placed = Vec::with_capacity(notes.len());
        for (index, note) in notes.iter().enumerate() {
            if note.start >= end {
                return Err(PatternError::NoteOutside {
                    index,
                    start: note.start.0,
                    length: end.0,
                });
            }
            placed.push(PlacedNote {
                start: note.start,
                end: note.end().min(end),
                pitch: note.pitch,
                velocity: note.velocity,
            });
        }
        sort(&mut placed);
        Ok(Self {
            length,
            notes: placed,
        })
    }

    pub(crate) fn length(&self) -> Length {
        self.length
    }

    /// How far into a round of this pattern `tick` is, for a pattern launched on `anchor`.
    /// Also before the anchor: the pattern loops both ways.
    pub(crate) fn phase(&self, anchor: Ticks, tick: Ticks) -> Ticks {
        let length = self.length.ticks().0;
        let (tick, anchor) = (tick.0 % length, anchor.0 % length);
        Ticks(match tick >= anchor {
            true => tick - anchor,
            false => tick + (length - anchor),
        })
    }

    /// Every note that starts in `range` when launched on `anchor`, in time order: at its
    /// place on the timeline, and its start in the pattern.
    pub(crate) fn starting_in(
        &self,
        anchor: Ticks,
        range: Range<Ticks>,
    ) -> impl Iterator<Item = (PlacedNote, Ticks)> + '_ {
        let phase = self.phase(anchor, range.start);
        Rounds {
            notes: &self.notes,
            length: i128::from(self.length.ticks().0),
            round_start: i128::from(range.start.0) - i128::from(phase.0),
            next: self.notes.partition_point(|note| note.start < phase),
            end: i128::from(range.end.0),
        }
    }

    /// Where the note that starts at `start` in this pattern with `pitch` ends, counted from
    /// the start of the pattern. `None` when the pattern has no such note.
    pub(crate) fn end_of(&self, start: Ticks, pitch: Pitch) -> Option<Ticks> {
        end_of(&self.notes, start, pitch)
    }
}

/// The notes of a pattern round after round, from a tick inside a round up to `end`.
struct Rounds<'a> {
    notes: &'a [PlacedNote],
    length: i128,
    /// Where the round of the next note started. Before the project start for a pattern
    /// launched later than where the range is.
    round_start: i128,
    next: usize,
    end: i128,
}

impl Iterator for Rounds<'_> {
    type Item = (PlacedNote, Ticks);

    fn next(&mut self) -> Option<Self::Item> {
        // A round with no note left goes on to the next one, so this ends: every turn either
        // gives a note or moves a whole round on, towards `end`.
        while self.round_start < self.end {
            let Some(note) = self.notes.get(self.next) else {
                self.round_start += self.length;
                self.next = 0;
                continue;
            };
            let start = self.round_start + i128::from(note.start.0);
            if start >= self.end {
                return None;
            }
            self.next += 1;
            let on_timeline = |tick: i128| Ticks(u64::try_from(tick).unwrap_or(u64::MAX));
            let placed = PlacedNote {
                start: on_timeline(start),
                end: on_timeline(self.round_start + i128::from(note.end.0)),
                ..*note
            };
            return Some((placed, note.start));
        }
        None
    }
}

/// When a launch takes effect while the transport plays. While it does not, a launch takes
/// effect at once.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantize {
    Now,
    Beat,
    #[default]
    Bar,
}

impl Quantize {
    /// The first tick at or after `tick` that this lands on. Allocates nothing, for the audio
    /// thread.
    pub(crate) fn point_from(self, tick: Ticks, time_signatures: &TimeSignatures) -> Ticks {
        match self {
            Self::Now => tick,
            Self::Beat => time_signatures.beat_from(tick).0,
            Self::Bar => {
                let bar = time_signatures.bar_at(tick);
                match bar.start == tick {
                    true => tick,
                    false => bar.end(),
                }
            }
        }
    }
}

/// A performance command: play pattern `pattern` of the set, or stop with `None`, from the next
/// point of `quantize`. A pattern that plays goes on until then. An index outside the set plays
/// nothing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub pattern: Option<usize>,
    pub quantize: Quantize,
}
