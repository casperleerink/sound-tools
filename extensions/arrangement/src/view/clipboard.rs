//! What cmd-c keeps: clips, with where they were to each other, or notes of a clip. Interface
//! state in the app only, never on the system clipboard and never saved, so another
//! application cannot paste a clip and a copy never outlives the session. Pure, no GPUI.
//!
//! The window has one clipboard, shared by the timeline and the note editor, as the snap is: it
//! holds clips or notes, and a copy of one replaces the other, as on the system clipboard. A
//! paste of clips in the note editor or of notes in the timeline does nothing.
//!
//! A paste keeps the shape of what was copied: the clips keep their distance in time and in
//! track rows. The earliest start lands at the playhead and the top row on the selected track.
//! A row that would fall below the last track lands on the last track, so nothing that was
//! copied is lost; clips may overlap, so that is always a valid arrangement. Notes keep their
//! distance in time and their pitch; a note that would start after the end of its clip is left
//! out, because every note starts inside its clip.

use std::cell::RefCell;
use std::rc::Rc;

use sound_core::Ticks;
use sound_notes::{Clip, Length, Note};

/// Clips copied in the window.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedClips {
    /// Each clip with its row from the top copied row, its name and its start from `start`.
    clips: Vec<(usize, String, Clip)>,
    /// Where the copied clips were: the earliest start and the top row.
    start: Ticks,
    top: usize,
}

impl CopiedClips {
    /// The clips, each with the row of its track in the arrangement and its name. `None` when
    /// there is nothing to copy.
    pub fn new(clips: impl IntoIterator<Item = (usize, String, Clip)>) -> Option<Self> {
        let clips: Vec<_> = clips.into_iter().collect();
        let start = clips.iter().map(|(_, _, clip)| clip.start).min()?;
        let top = clips.iter().map(|(row, _, _)| *row).min()?;
        let clips = clips
            .into_iter()
            .map(|(row, name, clip)| {
                let start = Ticks(clip.start.0 - start.0);
                (row - top, name, Clip { start, ..clip })
            })
            .collect();
        Some(Self { clips, start, top })
    }

    /// The earliest start and the top row of what was copied.
    pub fn origin(&self) -> (Ticks, usize) {
        (self.start, self.top)
    }

    /// From the earliest start to the latest end: where a duplicate goes after the original.
    pub fn span(&self) -> Ticks {
        let end = self.clips.iter().map(|(_, _, clip)| clip.end()).max();
        end.unwrap_or_default()
    }

    /// How many clips there are, for the undo label. Never none: `new` gives `None` then.
    pub(crate) fn len(&self) -> usize {
        self.clips.len()
    }

    /// Where each clip lands for a paste at `at` with the top row on `top`, in an arrangement of
    /// `rows` tracks: its row, its name and the clip. A row below the last track is the last
    /// track. Nothing without tracks.
    pub fn placed(&self, at: Ticks, top: usize, rows: usize) -> Vec<(usize, &str, Clip)> {
        let Some(last) = rows.checked_sub(1) else {
            return Vec::new();
        };
        let placed = self.clips.iter().map(|(row, name, clip)| {
            let start = at + clip.start;
            let row = (top + row).min(last);
            (
                row,
                name.as_str(),
                Clip {
                    start,
                    ..clip.clone()
                },
            )
        });
        placed.collect()
    }
}

/// Notes copied in the note editor.
#[derive(Clone, Debug, PartialEq)]
pub struct CopiedNotes {
    /// The notes, each with its start from `start`.
    notes: Vec<Note>,
    /// Where the earliest copied note started in its clip.
    start: Ticks,
}

impl CopiedNotes {
    /// `None` when there is nothing to copy.
    pub fn new(notes: impl IntoIterator<Item = Note>) -> Option<Self> {
        let notes: Vec<Note> = notes.into_iter().collect();
        let start = notes.iter().map(|note| note.start).min()?;
        let notes = notes
            .into_iter()
            .map(|note| Note {
                start: Ticks(note.start.0 - start.0),
                ..note
            })
            .collect();
        Some(Self { notes, start })
    }

    /// Where the earliest copied note started in its clip.
    pub fn start(&self) -> Ticks {
        self.start
    }

    /// From the earliest start to the latest end: where a duplicate goes after the original.
    pub fn span(&self) -> Ticks {
        let end = self.notes.iter().map(Note::end).max();
        end.unwrap_or_default()
    }

    /// The notes for a paste at `at`, a tick of a clip of `length`. A note that would start at
    /// or after the end of the clip is left out.
    pub fn placed(&self, at: Ticks, length: Length) -> Vec<Note> {
        let notes = self.notes.iter().map(|note| Note {
            start: at + note.start,
            ..*note
        });
        notes.filter(|note| note.start < length.ticks()).collect()
    }
}

/// What the clipboard of the window holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Copied {
    Clips(CopiedClips),
    Notes(CopiedNotes),
}

/// The one clipboard of the window, for the timeline and the note editor. Interface state.
pub type SharedClipboard = Rc<RefCell<Option<Copied>>>;

#[cfg(test)]
mod tests {
    use sound_notes::Length;

    use super::*;

    const BAR: u64 = 3840;

    fn clip(start: u64, length: u64) -> Clip {
        Clip::new(
            Ticks(start),
            Length::new(Ticks(length)).unwrap(),
            Vec::new(),
        )
    }

    #[test]
    fn a_paste_keeps_the_distances_in_time_and_rows() {
        let copied = CopiedClips::new([
            (2, "b".to_string(), clip(3 * BAR, BAR)),
            (1, "a".to_string(), clip(2 * BAR, BAR)),
        ])
        .unwrap();
        assert_eq!(copied.origin(), (Ticks(2 * BAR), 1));
        assert_eq!(copied.span(), Ticks(2 * BAR));
        assert_eq!(copied.len(), 2);

        let placed = copied.placed(Ticks(10 * BAR), 0, 5);
        assert_eq!(
            placed,
            [(1, "b", clip(11 * BAR, BAR)), (0, "a", clip(10 * BAR, BAR))]
        );
    }

    #[test]
    fn rows_past_the_last_track_land_on_the_last_track() {
        let copied = CopiedClips::new([
            (0, "a".to_string(), clip(0, BAR)),
            (2, "b".to_string(), clip(0, BAR)),
        ])
        .unwrap();
        let rows: Vec<_> = copied
            .placed(Ticks(0), 1, 2)
            .into_iter()
            .map(|(row, _, _)| row)
            .collect();
        assert_eq!(rows, [1, 1]);
        assert!(copied.placed(Ticks(0), 0, 0).is_empty());
        assert_eq!(CopiedClips::new([]), None);
    }

    fn note(start: u64, length: u64, pitch: u8) -> Note {
        Note {
            start: Ticks(start),
            length: Length::new(Ticks(length)).unwrap(),
            pitch: sound_notes::Pitch::new(pitch).unwrap(),
            velocity: sound_notes::Velocity::new(90).unwrap(),
        }
    }

    #[test]
    fn a_paste_of_notes_keeps_their_distances_and_leaves_out_what_starts_past_the_clip() {
        let copied = CopiedNotes::new([note(1200, 240, 64), note(960, 480, 60)]).unwrap();
        assert_eq!(copied.start(), Ticks(960));
        assert_eq!(copied.span(), Ticks(480));
        let length = Length::new(Ticks(BAR)).unwrap();
        assert_eq!(
            copied.placed(Ticks(0), length),
            [note(240, 240, 64), note(0, 480, 60)]
        );
        // The second note would start at the end of the clip.
        assert_eq!(
            copied.placed(Ticks(BAR - 240), length),
            [note(BAR - 240, 480, 60)]
        );
        assert!(copied.placed(Ticks(BAR), length).is_empty());
        assert_eq!(CopiedNotes::new([]), None);
    }
}
