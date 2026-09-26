//! What cmd-c keeps: clips, with where they were to each other. Interface state in the app
//! only, never on the system clipboard and never saved, so another application cannot paste a
//! clip and a copy never outlives the session. Pure, no GPUI.
//!
//! A paste keeps the shape of what was copied: the clips keep their distance in time and in
//! track rows. The earliest start lands at the playhead and the top row on the selected track.
//! A row that would fall below the last track lands on the last track, so nothing that was
//! copied is lost; clips may overlap, so that is always a valid arrangement.

use sound_core::Ticks;
use sound_notes::Clip;

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
}
