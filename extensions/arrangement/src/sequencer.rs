//! Plays the clips of one track: one immutable snapshot of its notes on the control side, one
//! processor that sends them from the transport tick range on the audio thread.
//!
//! The rule that shapes this file: a note that started always gets its off. The processor keeps
//! a fixed list of the notes it started, with the tick of their off. Offs come from that list
//! and never from the snapshot, so they arrive also when the clip changed, moved or went away
//! while the note sounded.

use std::ops::Range;
use std::sync::Arc;

use sound_core::{
    EventOutput, EventOutputs, Ports, PrepareConfig, ProcessContext, Processor, Ticks, Transport,
};
use sound_notes::{Clip, NoteEvent, Pitch, PlacedNote};

/// How many notes one track can hold at a time. A note that would be one more is not played
/// and counts in `EngineStatus::event_overflows`. The synth has 16 voices.
pub const HELD_CAPACITY: usize = 128;

/// Every note of a track at its place on the timeline, sorted by start and then pitch, so a
/// block finds its notes with one binary search.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TrackSnapshot {
    notes: Vec<PlacedNote>,
}

impl TrackSnapshot {
    /// Clips may overlap: the notes of all of them play.
    pub fn new<'a>(clips: impl IntoIterator<Item = &'a Clip>) -> Self {
        let mut notes: Vec<PlacedNote> = clips.into_iter().flat_map(Clip::placed_notes).collect();
        // The whole note is the key, so the same clips always give the same order.
        notes.sort_unstable_by_key(|note| (note.start, note.pitch, note.end, note.velocity));
        Self { notes }
    }

    pub fn notes(&self) -> &[PlacedNote] {
        &self.notes
    }

    fn starting_in(&self, range: &Range<Ticks>) -> &[PlacedNote] {
        let first = self.notes.partition_point(|note| note.start < range.start);
        let last = self.notes.partition_point(|note| note.start < range.end);
        self.notes.get(first..last).unwrap_or_default()
    }

    /// Where the note that started at `start` with `pitch` ends now. `None` when it is gone.
    /// Of several such notes, in clips that overlap, the last end.
    fn end_of(&self, start: Ticks, pitch: Pitch) -> Option<Ticks> {
        let first = self
            .notes
            .partition_point(|note| (note.start, note.pitch) < (start, pitch));
        let same = self.notes.get(first..).unwrap_or_default().iter();
        same.take_while(|note| (note.start, note.pitch) == (start, pitch))
            .map(|note| note.end)
            .max()
    }
}

/// A note this processor started and has not ended yet.
#[derive(Copy, Clone)]
struct Held {
    start: Ticks,
    end: Ticks,
    pitch: Pitch,
}

pub struct Sequencer {
    snapshot: Arc<TrackSnapshot>,
    /// A new snapshot came since the last block.
    swapped: bool,
    /// Never grows past [`HELD_CAPACITY`], so it never allocates on the audio thread.
    held: Vec<Held>,
}

impl Sequencer {
    pub const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Default for Sequencer {
    fn default() -> Self {
        Self {
            snapshot: Arc::default(),
            swapped: false,
            held: Vec::with_capacity(HELD_CAPACITY),
        }
    }
}

impl Processor for Sequencer {
    type Update = Arc<TrackSnapshot>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Arc<TrackSnapshot>) {
        // The old snapshot rides back to the control thread inside the update.
        std::mem::swap(&mut self.snapshot, update);
        self.swapped = true;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let ProcessContext {
            transport,
            event_outputs,
            ..
        } = context;
        let range = transport.tick_range.clone();
        let Self {
            snapshot,
            swapped,
            held,
        } = self;

        if transport.jumped || transport.stopped_playing {
            // After this block the range is empty or somewhere else, so no off of the list
            // would come. The buffer is still empty here, so the event fits.
            event_outputs.push(Self::NOTES, 0, NoteEvent::AllOff);
            held.clear();
        }

        // A held note follows the new snapshot: it takes its new end, or it ends now when its
        // note is gone or moved. A note the edit did not touch is found with the same end, so
        // nothing is sent for it: a swap neither cuts nor starts it again.
        if std::mem::take(swapped) {
            held.retain_mut(|note| match snapshot.end_of(note.start, note.pitch) {
                Some(end) if end > range.start => {
                    note.end = end;
                    true
                }
                _ => !event_outputs.push(Self::NOTES, 0, NoteEvent::Off { pitch: note.pitch }),
            });
        }

        // In time order, and on one tick the offs before the ons. Else the end of one note
        // would release the next note of the same pitch that starts there. An event that
        // does not fit stays out of the list (an on) or in it (an off, sent again next block).
        for note in snapshot.starting_in(&range) {
            release_before(held, note.start + Ticks(1), transport, event_outputs);
            let Some(offset) = transport.offset_of(note.start) else {
                continue;
            };
            if held.len() >= HELD_CAPACITY {
                event_outputs.count_dropped();
                continue;
            }
            let on = NoteEvent::On {
                pitch: note.pitch,
                velocity: note.velocity,
            };
            if event_outputs.push(Self::NOTES, offset, on) {
                held.push(Held {
                    start: note.start,
                    end: note.end,
                    pitch: note.pitch,
                });
            }
        }
        release_before(held, range.end, transport, event_outputs);
    }
}

/// Sends the off of every held note that ends before `limit`.
fn release_before(
    held: &mut Vec<Held>,
    limit: Ticks,
    transport: &Transport<'_>,
    event_outputs: &mut EventOutputs<'_>,
) {
    held.retain(|note| {
        // An end before this block is an off that did not fit earlier.
        let offset = transport.offset_of(note.end).unwrap_or(0);
        let off = NoteEvent::Off { pitch: note.pitch };
        note.end >= limit || !event_outputs.push(Sequencer::NOTES, offset, off)
    });
}
