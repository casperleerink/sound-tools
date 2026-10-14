//! The pitch rows of the note editor: the rows of the grid of the clip's editor, top row
//! first, and an extra row for each pitch a note has that the grid has no row for. Pure math,
//! like [`super::roll`].
//!
//! A step is one row, whatever pitches the rows have: up a step is the row above.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use sound_notes::{Grid, NoteShape, Pitch, Row};

use super::layout::Viewport;

pub struct Rows {
    grid: Arc<Grid>,
    /// Top row first: its pitch, and its row of the grid, `None` for an extra row.
    top_first: Vec<(Pitch, Option<usize>)>,
    /// Where in `top_first` each pitch with a row is.
    index: [Option<u8>; 128],
}

/// How one row looks.
pub struct RowLook<'a> {
    pub pitch: Pitch,
    pub shade: bool,
    /// An extra row: no row of the grid has its pitch.
    pub extra: bool,
    pub label: Option<Cow<'a, str>>,
}

impl Rows {
    /// The rows of `grid`, and an extra row for each of `pitches` the grid has no row for.
    pub fn new(grid: Arc<Grid>, pitches: impl IntoIterator<Item = Pitch>) -> Self {
        // Per pitch: no row, an extra row, or the row of the grid.
        let mut rows: [Option<Option<usize>>; 128] = [None; 128];
        for pitch in pitches {
            if let Some(slot) = rows.get_mut(usize::from(pitch.number())) {
                *slot = Some(None);
            }
        }
        for (row, Row { pitch, .. }) in grid.rows.iter().enumerate() {
            if let Some(slot) = rows.get_mut(usize::from(pitch.number())) {
                *slot = Some(Some(row));
            }
        }
        let numbers = rows.iter().enumerate().rev();
        let top_first: Vec<(Pitch, Option<usize>)> = numbers
            .filter_map(|(number, row)| Some((Pitch::nearest(number as i64), (*row)?)))
            .collect();
        let mut index = [None; 128];
        for (place, (pitch, _)) in top_first.iter().enumerate() {
            if let Some(slot) = index.get_mut(usize::from(pitch.number())) {
                *slot = u8::try_from(place).ok();
            }
        }
        Self {
            grid,
            top_first,
            index,
        }
    }

    pub fn count(&self) -> usize {
        self.top_first.len()
    }

    pub fn height(&self) -> f32 {
        self.grid.row_height
    }

    pub fn shape(&self) -> NoteShape {
        self.grid.notes
    }

    pub fn index_of(&self, pitch: Pitch) -> Option<usize> {
        let index = self.index.get(usize::from(pitch.number()))?;
        index.map(usize::from)
    }

    pub fn look(&self, index: usize) -> Option<RowLook<'_>> {
        let (pitch, row) = *self.top_first.get(index)?;
        let row = row.and_then(|row| self.grid.rows.get(row));
        Some(match row {
            Some(row) => RowLook {
                pitch,
                shade: row.shade,
                extra: false,
                label: row.label.as_deref().map(Cow::Borrowed),
            },
            None => RowLook {
                pitch,
                shade: false,
                extra: true,
                label: Some(Cow::Owned(pitch.name())),
            },
        })
    }

    /// The top of a row.
    pub fn top_of(&self, viewport: &Viewport, index: usize) -> f32 {
        (index as f64 * f64::from(self.height()) - viewport.scroll_y) as f32
    }

    /// The top of the row of a pitch. `None` when it has no row.
    pub fn y_of(&self, viewport: &Viewport, pitch: Pitch) -> Option<f32> {
        Some(self.top_of(viewport, self.index_of(pitch)?))
    }

    fn row_at(&self, viewport: &Viewport, y: f32) -> f64 {
        ((f64::from(y) + viewport.scroll_y) / f64::from(self.height())).floor()
    }

    /// The pitch of the row at `y`. `None` above the top row and below the bottom one.
    pub fn pitch_at(&self, viewport: &Viewport, y: f32) -> Option<Pitch> {
        let row = self.row_at(viewport, y);
        let in_range = (0.0..self.count() as f64).contains(&row);
        let (pitch, _) = self.top_first.get(in_range.then_some(row as usize)?)?;
        Some(*pitch)
    }

    /// The row a drag is at: the nearest one when the pointer is above or below all of them.
    pub fn nearest(&self, viewport: &Viewport, y: f32) -> usize {
        let last = self.count().saturating_sub(1);
        self.row_at(viewport, y).clamp(0.0, last as f64) as usize
    }

    /// The row whose pitch is nearest to `number`.
    pub fn nearest_to(&self, number: u8) -> usize {
        let rows = self.top_first.iter().enumerate();
        let distance =
            |(_, (pitch, _)): &(usize, &(Pitch, Option<usize>))| pitch.number().abs_diff(number);
        rows.min_by_key(distance).map_or(0, |(index, _)| index)
    }

    /// The pitch `steps` rows up, down when negative. It stops at the top and the bottom row.
    pub fn stepped(&self, pitch: Pitch, steps: i64) -> Pitch {
        let Some(index) = self.index_of(pitch) else {
            return pitch;
        };
        let last = self.count().saturating_sub(1) as i64;
        let moved = (index as i64 - steps).clamp(0, last) as usize;
        self.top_first.get(moved).map_or(pitch, |(pitch, _)| *pitch)
    }

    /// The steps an octave up or down from a pitch is: the rows up to 12 semitones away. On
    /// a piano roll that is 12 rows, and in a scale of seven notes 7.
    pub fn octave(&self, pitch: Pitch, up: bool) -> i64 {
        let number = i64::from(pitch.number());
        let within = |(other, _): &&(Pitch, Option<usize>)| {
            let distance = i64::from(other.number()) - number;
            match up {
                true => (1..=12).contains(&distance),
                false => (-12..=-1).contains(&distance),
            }
        };
        let count = self.top_first.iter().filter(within).count() as i64;
        if up { count } else { -count }
    }

    /// The rows that show, whole or in part, in a note area of this height.
    pub fn visible(&self, viewport: &Viewport, height: f32) -> Range<usize> {
        if self.top_first.is_empty() {
            return 0..0;
        }
        self.nearest(viewport, 0.0)..self.nearest(viewport, height - 0.01) + 1
    }

    pub fn content_height(&self) -> f64 {
        self.count() as f64 * f64::from(self.height())
    }
}

#[cfg(test)]
mod tests {
    use sound_notes::{View, piano_roll};

    use super::*;

    fn pitch(number: u8) -> Pitch {
        Pitch::new(number).unwrap()
    }

    fn piano(extra: &[u8]) -> Rows {
        let grid = piano_roll().grid_for(&View::default());
        Rows::new(grid, extra.iter().map(|number| pitch(*number)))
    }

    /// C major from C4 to C5, 20 pt rows.
    fn c_major(extra: &[u8]) -> Rows {
        let rows = [60, 62, 64, 65, 67, 69, 71, 72].map(|number| Row {
            pitch: pitch(number),
            label: None,
            shade: false,
        });
        let grid = Grid {
            rows: rows.to_vec(),
            row_height: 20.0,
            notes: NoteShape::Hit,
        };
        Rows::new(Arc::new(grid), extra.iter().map(|number| pitch(*number)))
    }

    #[test]
    fn pitches_and_rows_of_the_piano_roll_convert_both_ways() {
        let rows = piano(&[]);
        let viewport = Viewport {
            scroll_y: 120.0,
            ..Viewport::default()
        };
        let top = Viewport::default();
        assert_eq!(rows.y_of(&top, pitch(127)), Some(0.0));
        assert_eq!(rows.y_of(&top, pitch(0)), Some(127.0 * 12.0));
        assert_eq!(rows.y_of(&viewport, pitch(117)), Some(0.0));
        for number in [0, 1, 59, 60, 126, 127] {
            let y = rows.y_of(&viewport, pitch(number)).unwrap();
            assert_eq!(rows.pitch_at(&viewport, y), Some(pitch(number)));
            assert_eq!(rows.pitch_at(&viewport, y + 11.9), Some(pitch(number)));
        }
        assert_eq!(rows.pitch_at(&top, -0.1), None);
        assert_eq!(rows.pitch_at(&top, 128.0 * 12.0), None);
        assert_eq!(rows.nearest(&top, -500.0), 0);
        assert_eq!(rows.nearest(&top, 100_000.0), 127);
        assert_eq!(rows.nearest_to(60), 67);
        // Half of 127, all of 126 and half of 125 in 24 px.
        let half = Viewport {
            scroll_y: 6.0,
            ..Viewport::default()
        };
        assert_eq!(rows.visible(&half, 24.0), 0..3);
        assert_eq!(rows.visible(&top, 24.0), 0..2);
    }

    #[test]
    fn a_scale_has_its_rows_only_with_their_own_height() {
        let rows = c_major(&[]);
        let top = Viewport::default();
        assert_eq!(rows.count(), 8);
        assert_eq!(rows.y_of(&top, pitch(72)), Some(0.0));
        assert_eq!(rows.y_of(&top, pitch(71)), Some(20.0));
        assert_eq!(rows.y_of(&top, pitch(61)), None);
        assert_eq!(rows.pitch_at(&top, 159.0), Some(pitch(60)));
        assert_eq!(rows.pitch_at(&top, 160.0), None);
        assert_eq!(rows.content_height(), 160.0);
        // D and E are as near to 63: the higher one.
        assert_eq!(rows.look(rows.nearest_to(63)).unwrap().pitch, pitch(64));
        assert_eq!(rows.nearest_to(0), 7);
    }

    #[test]
    fn a_pitch_without_a_row_gets_an_extra_row_in_its_place() {
        let rows = c_major(&[66, 64, 100]);
        // F# between G and F, and 100 above all of them.
        let pitches: Vec<u8> = (0..rows.count())
            .map(|index| rows.look(index).unwrap().pitch.number())
            .collect();
        assert_eq!(pitches, [100, 72, 71, 69, 67, 66, 65, 64, 62, 60]);
        let extra = rows.look(5).unwrap();
        assert!(extra.extra);
        assert_eq!(extra.label.as_deref(), Some("F#4"));
        let row = rows.look(4).unwrap();
        assert!(!row.extra && row.label.is_none());
    }

    #[test]
    fn a_step_is_a_row_and_stops_at_the_ends() {
        let rows = c_major(&[66]);
        assert_eq!(rows.stepped(pitch(64), 2), pitch(66));
        assert_eq!(rows.stepped(pitch(64), 3), pitch(67));
        assert_eq!(rows.stepped(pitch(64), -1), pitch(62));
        assert_eq!(rows.stepped(pitch(64), 100), pitch(72));
        assert_eq!(rows.stepped(pitch(64), -100), pitch(60));
        // A pitch without a row stays.
        assert_eq!(rows.stepped(pitch(61), 1), pitch(61));
        let piano = piano(&[]);
        assert_eq!(piano.stepped(pitch(60), 3), pitch(63));
        assert_eq!(piano.stepped(pitch(126), 5), pitch(127));
    }

    #[test]
    fn an_octave_is_the_rows_up_to_twelve_semitones_away() {
        let piano = piano(&[]);
        assert_eq!(piano.octave(pitch(60), true), 12);
        assert_eq!(piano.octave(pitch(60), false), -12);
        // Near the ends there are fewer rows.
        assert_eq!(piano.octave(pitch(120), true), 7);
        assert_eq!(piano.octave(pitch(5), false), -5);
        let scale = c_major(&[]);
        assert_eq!(scale.octave(pitch(60), true), 7);
        assert_eq!(
            scale.stepped(pitch(60), scale.octave(pitch(60), true)),
            pitch(72)
        );
        assert_eq!(scale.octave(pitch(64), true), 5);
        assert_eq!(scale.octave(pitch(72), true), 0);
    }
}
