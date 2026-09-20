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
use sound_notes::{Clip, NoteEvent, Pitch, PlacedNote, Velocity};

/// How long a preview note sounds. Its off comes from the processor after this time, so no
/// interface can leave one sounding.
pub const PREVIEW_SECONDS: f32 = 0.3;

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

/// What the control side sends to a [`Sequencer`].
pub enum SequencerUpdate {
    /// The notes of the track, from its behaviour on every change.
    Snapshot(Arc<TrackSnapshot>),
    /// A note from an interface that sounds now, for [`PREVIEW_SECONDS`], also while the
    /// project does not play. A new one ends the one before it.
    Preview { pitch: Pitch, velocity: Velocity },
}

/// The preview note that sounds, and for how many more frames.
#[derive(Copy, Clone)]
struct Previewed {
    pitch: Pitch,
    frames_left: usize,
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
    /// A preview that came since the last block.
    preview_wanted: Option<(Pitch, Velocity)>,
    previewed: Option<Previewed>,
    preview_frames: usize,
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
            preview_wanted: None,
            previewed: None,
            preview_frames: 0,
        }
    }
}

impl Processor for Sequencer {
    type Update = SequencerUpdate;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.preview_frames = (config.sample_rate as f32 * PREVIEW_SECONDS) as usize;
    }

    fn update(&mut self, update: &mut SequencerUpdate) {
        match update {
            SequencerUpdate::Snapshot(snapshot) => {
                // The old snapshot rides back to the control thread inside the update.
                std::mem::swap(&mut self.snapshot, snapshot);
                self.swapped = true;
            }
            SequencerUpdate::Preview { pitch, velocity } => {
                self.preview_wanted = Some((*pitch, *velocity));
            }
        }
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
            preview_wanted,
            previewed,
            preview_frames,
        } = self;
        let mut sender = Sender {
            event_outputs,
            full: false,
        };

        if transport.jumped || transport.stopped_playing {
            // After this block the range is empty or somewhere else, so no off of the list
            // would come. The buffer is still empty here, so the event fits.
            sender.send(0, NoteEvent::AllOff);
            held.clear();
            *previewed = None;
        }
        preview(
            preview_wanted,
            previewed,
            *preview_frames,
            context.frames,
            held,
            &mut sender,
        );

        // A held note follows the new snapshot: it takes its new end, or it ends now when its
        // note is gone or moved. A note the edit did not touch is found with the same end, so
        // nothing is sent for it: a swap neither cuts nor starts it again.
        if std::mem::take(swapped) {
            let mut index = 0;
            while let Some(note) = held.get_mut(index) {
                match snapshot.end_of(note.start, note.pitch) {
                    Some(end) if end > range.start => {
                        note.end = end;
                        index += 1;
                    }
                    // Removed: the note that took its place at `index` is looked at next.
                    _ if end_note(held, index, 0, &mut sender) => {}
                    _ => index += 1,
                }
            }
        }

        // In time order, and on one tick the offs before the ons. Else the end of one note
        // would release the next note of the same pitch that starts there.
        for note in snapshot.starting_in(&range) {
            release_before(held, note.start + Ticks(1), transport, &mut sender);
            let Some(offset) = transport.offset_of(note.start) else {
                continue;
            };
            if held.len() >= HELD_CAPACITY {
                sender.event_outputs.count_dropped();
                continue;
            }
            let on = NoteEvent::On {
                pitch: note.pitch,
                velocity: note.velocity,
            };
            // An on that does not fit is not played, so it is not held either.
            if sender.send_or_drop(offset, on) {
                held.push(Held {
                    start: note.start,
                    end: note.end,
                    pitch: note.pitch,
                });
            }
        }
        release_before(held, range.end, transport, &mut sender);
    }
}

/// The preview note, in engine time, so it sounds while the project does not play. It goes out
/// at the start of the block, before the notes of the timeline.
///
/// A preview and a note of the timeline may share a pitch, and an off releases every note of
/// its pitch. So the preview sends no off while the timeline holds its pitch: the off of that
/// note ends both. The other way round, a note of the timeline that ends cuts a preview of its
/// pitch short, which nobody hears as a fault.
fn preview(
    wanted: &mut Option<(Pitch, Velocity)>,
    previewed: &mut Option<Previewed>,
    preview_frames: usize,
    frames: usize,
    held: &[Held],
    sender: &mut Sender<'_, '_>,
) {
    let end = |previewed: &mut Option<Previewed>, sender: &mut Sender<'_, '_>| {
        let Some(note) = *previewed else {
            return true;
        };
        let shared = held.iter().any(|held| held.pitch == note.pitch);
        let ended = shared || sender.send(0, NoteEvent::Off { pitch: note.pitch });
        if ended {
            *previewed = None;
        }
        ended
    };
    if let Some((pitch, velocity)) = *wanted {
        // An off that does not fit waits for the next block, and the new note with it.
        if !end(previewed, sender) {
            return;
        }
        *wanted = None;
        if sender.send_or_drop(0, NoteEvent::On { pitch, velocity }) {
            *previewed = Some(Previewed {
                pitch,
                frames_left: preview_frames,
            });
        }
    } else if let Some(note) = previewed {
        note.frames_left = note.frames_left.saturating_sub(frames);
        if note.frames_left == 0 {
            end(previewed, sender);
        }
    }
}

/// The event output for one block. Once an event did not fit, the buffer is full for the rest
/// of the block and nothing more is pushed. So the engine counts that one event, and not one
/// more for every later try.
struct Sender<'a, 'b> {
    event_outputs: &'a mut EventOutputs<'b>,
    full: bool,
}

impl Sender<'_, '_> {
    /// For an event that is sent again later when it does not fit: an off.
    fn send(&mut self, offset: usize, event: NoteEvent) -> bool {
        if !self.full {
            self.full = !self.event_outputs.push(Sequencer::NOTES, offset, event);
        }
        !self.full
    }

    /// For an event that is lost when it does not fit: an on. Each lost one counts once, by
    /// the buffer when it refused the push, else here.
    fn send_or_drop(&mut self, offset: usize, event: NoteEvent) -> bool {
        let refused_before = self.full;
        let sent = self.send(offset, event);
        if refused_before {
            self.event_outputs.count_dropped();
        }
        sent
    }
}

/// Ends the held note at `index` and takes it off the list. While another held note has the
/// same pitch, no off is sent: an off releases every note of its pitch, so it goes out with
/// the last holder. The pitch then sounds until its last note ends, in any order of ends and
/// with or without a snapshot swap in between. Returns false when the off did not fit: the
/// note stays listed and ends in a later block.
fn end_note(
    held: &mut Vec<Held>,
    index: usize,
    offset: usize,
    sender: &mut Sender<'_, '_>,
) -> bool {
    let Some(note) = held.get(index).copied() else {
        return false;
    };
    let mut others = held.iter().enumerate().filter(|(other, _)| *other != index);
    let shared = others.any(|(_, other)| other.pitch == note.pitch);
    if shared || sender.send(offset, NoteEvent::Off { pitch: note.pitch }) {
        held.swap_remove(index);
        return true;
    }
    false
}

/// Ends every held note that ends before `limit`, the earliest end first, so that of two
/// notes of one pitch the later end sends the off.
fn release_before(
    held: &mut Vec<Held>,
    limit: Ticks,
    transport: &Transport<'_>,
    sender: &mut Sender<'_, '_>,
) {
    loop {
        let due = held.iter().enumerate().filter(|(_, note)| note.end < limit);
        let Some((index, note)) = due.min_by_key(|(_, note)| note.end) else {
            return;
        };
        // An end before this block is an off that did not fit earlier.
        let offset = transport.offset_of(note.end).unwrap_or(0);
        if !end_note(held, index, offset, sender) {
            return;
        }
    }
}
