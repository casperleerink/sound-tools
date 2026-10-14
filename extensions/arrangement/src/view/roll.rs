//! Where things are in the note editor, and what a drag or a key does to a note. Pure math,
//! no GPUI and no project, like [`super::layout`].
//!
//! The editor shows one clip in the rows of its clip editor, the piano roll unless the clip
//! names another ([`super::rows`]). Time runs across on the project timeline, with the same
//! [`Viewport`] math as the arrangement, so bar numbers and the playhead mean the same in
//! both. Rows run down from the highest pitch: `y` 0 is the top of the top row when nothing
//! is scrolled. The coordinates are those of the note area, right of the keys and below the
//! ruler.
//!
//! The lowest 56 pt of the editor is the velocity lane: a bar at the start of each note, as tall
//! as its velocity. Its coordinates start at the top of the lane.

use std::ops::RangeInclusive;

use sound_core::{TICKS_PER_QUARTER, Ticks, TimeSignatures};
use sound_notes::{Clip, Length, Note, NoteShape, Pitch, Velocity};

use super::gesture::{Zone, shortest, zone_at};
use super::layout::{LEAD_IN, RULER_HEIGHT, Rect, Viewport, shifted};
use super::rows::Rows;
use super::snap::Grid;

/// The height of the editor panel, ruler included.
pub const EDITOR_HEIGHT: f32 = 352.0;
/// The width of the key strip, at the right edge of the header column.
pub const KEYS_WIDTH: f32 = 32.0;
/// The height of the velocity lane at the bottom of the editor.
pub const VELOCITY_HEIGHT: f32 = 56.0;
/// The height of the pitch rows: the editor less its ruler and the velocity lane.
pub const ROLL_HEIGHT: f32 = EDITOR_HEIGHT - RULER_HEIGHT - VELOCITY_HEIGHT;
/// The width of a velocity bar.
pub const VELOCITY_BAR_WIDTH: f32 = 3.0;
/// Air in the lane above a bar of velocity 127, and under every bar.
pub(super) const VELOCITY_TOP: f32 = 8.0;
pub(super) const VELOCITY_BOTTOM: f32 = 4.0;
/// How far beside a bar a press still takes it, so a trackpad hits a bar of 3 pt.
const VELOCITY_REACH: f32 = 4.0;
/// The velocity of a note that is drawn. The lane changes it afterwards.
pub const DRAWN_VELOCITY: i64 = 100;
/// The pitch whose row is in the middle of the editor of a clip without notes: middle C.
const MIDDLE_PITCH: u8 = 60;

/// Zoom limits when the editor opens. It fits the clip, but a long clip stays readable and a
/// short one does not fill the screen with one bar.
const OPENING_PIXELS_PER_QUARTER: RangeInclusive<f64> = 24.0..=192.0;
/// Room right of the clip when the editor opens.
const OPENING_ROOM: f32 = 96.0;

/// Where a note of the clip is drawn: a bar that ends where the note ends, or with its clip
/// when it is longer, as it sounds; a hit is a square at its start. `None` when its pitch has
/// no row.
pub fn note_rect(viewport: &Viewport, rows: &Rows, clip: &Clip, note: &Note) -> Option<Rect> {
    let x = viewport.x_of(clip.start + note.start);
    // A gap of a pixel to the next note and to the next row.
    let height = rows.height() - 1.0;
    let width = match rows.shape() {
        NoteShape::Bar => {
            let end = (clip.start + note.end()).min(clip.end());
            (viewport.x_of(end) - x - 1.0).max(2.0)
        }
        NoteShape::Hit => height,
    };
    Some(Rect {
        x,
        y: rows.y_of(viewport, note.pitch)?,
        width,
        height,
    })
}

/// The note on top at a position, as its index in the clip, with the zone that was hit. A
/// note has no left edge: only the end of a bar is dragged, and a hit has none.
pub fn note_at(
    viewport: &Viewport,
    rows: &Rows,
    clip: &Clip,
    x: f32,
    y: f32,
) -> Option<(usize, Zone)> {
    let notes = clip.notes.iter().enumerate().rev();
    let hit = notes
        .filter_map(|(index, note)| Some((index, note_rect(viewport, rows, clip, note)?)))
        .find(|(_, rect)| rect.contains(x, y));
    hit.map(|(index, rect)| match (rows.shape(), zone_at(rect, x)) {
        (NoteShape::Bar, Zone::RightEdge) => (index, Zone::RightEdge),
        _ => (index, Zone::Body),
    })
}

/// The viewport of an editor that opens for a clip in a note area of this size: the clip
/// starts at the left and fits the width, and the middle of the rows of its notes is in the
/// middle of the area.
pub fn opened(clip: &Clip, rows: &Rows, width: f32, height: f32) -> Viewport {
    let quarters = clip.length.ticks().0 as f64 / TICKS_PER_QUARTER as f64;
    let fitted = f64::from((width - LEAD_IN - OPENING_ROOM).max(1.0)) / quarters;
    let pixels_per_quarter = fitted.clamp(
        *OPENING_PIXELS_PER_QUARTER.start(),
        *OPENING_PIXELS_PER_QUARTER.end(),
    );
    let indices = clip
        .notes
        .iter()
        .filter_map(|note| rows.index_of(note.pitch));
    let middle = match (indices.clone().min(), indices.max()) {
        (Some(top), Some(bottom)) => (top + bottom) as f64 / 2.0,
        _ => rows.nearest_to(MIDDLE_PITCH) as f64,
    };
    let middle_y = (middle + 0.5) * f64::from(rows.height());
    Viewport {
        pixels_per_quarter,
        scroll_x: clip.start.0 as f64 * pixels_per_quarter / TICKS_PER_QUARTER as f64,
        scroll_y: (middle_y - f64::from(height) / 2.0).round(),
    }
}

/// Keeps the scroll inside the rows and the time up to the end of the clip.
pub fn clamped(
    viewport: &Viewport,
    rows: &Rows,
    clip: &Clip,
    time_signatures: &TimeSignatures,
    width: f32,
    height: f32,
) -> Viewport {
    let content_height = rows.content_height();
    viewport.clamped_to(clip.end(), content_height, time_signatures, width, height)
}

/// The note that a drag draws: it starts in the grid cell where the mouse went down, with the
/// pitch of that row, and ends at the pointer, at least one unit of the grid later and at most where
/// the clip ends. A hit is one unit long wherever the pointer is. `None` when the mouse went down
/// outside the clip: a note must start inside. Both ticks are project positions.
pub fn drawn_note(
    clip: &Clip,
    down: Ticks,
    pointer: Ticks,
    pitch: Pitch,
    grid: &Grid,
    shape: NoteShape,
) -> Option<Note> {
    if down < clip.start || down >= clip.end() {
        return None;
    }
    let pointer = match shape {
        NoteShape::Bar => pointer,
        NoteShape::Hit => down,
    };
    // The grid is that of the project. In a clip that starts off the grid, the first cell
    // starts with the clip.
    let start = grid.floor(down).max(clip.start);
    let end = grid
        .snap(pointer)
        .max(start + grid.unit_at(start))
        .min(clip.end());
    Some(Note::new(
        Ticks(start.0 - clip.start.0),
        Length::at_least_one(Ticks(end.0 - start.0)),
        pitch,
        Velocity::nearest(DRAWN_VELOCITY),
    ))
}

/// The latest start of a note in a clip: where its end meets the clip end. A note that already
/// reaches past the clip end can go left and does not have to.
fn latest_start(clip_length: Length, note: &Note) -> Ticks {
    let room = clip_length.ticks().0.saturating_sub(note.length.ticks().0);
    Ticks(room.max(note.start.0))
}

/// The note moved by a delta in time and by rows. It stays whole inside the clip: it stops
/// at the clip start, where its end meets the clip end, and at the top and the bottom row.
pub fn moved_note(rows: &Rows, clip_length: Length, origin: Note, delta: i64, steps: i64) -> Note {
    Note {
        start: shifted(origin.start, delta).min(latest_start(clip_length, &origin)),
        pitch: rows.stepped(origin.pitch, steps),
        ..origin
    }
}

/// Several notes moved by one delta in time and by rows, as a whole: the delta stops where the
/// first of them meets the clip start, the clip end, the top or the bottom row, so they keep
/// their distances to each other, in rows.
pub fn moved_notes(
    rows: &Rows,
    clip_length: Length,
    origins: &[Note],
    delta: i64,
    steps: i64,
) -> Vec<Note> {
    let ticks = |ticks: Ticks| ticks.0 as i64;
    let earliest = origins.iter().map(|note| ticks(note.start)).min();
    let room = origins
        .iter()
        .map(|note| ticks(latest_start(clip_length, note)) - ticks(note.start))
        .min();
    let delta = delta.clamp(-earliest.unwrap_or(0), room.unwrap_or(0));
    let indices = origins.iter().filter_map(|note| rows.index_of(note.pitch));
    let (top, bottom) = (indices.clone().min(), indices.max());
    let last = rows.count().saturating_sub(1);
    let steps = match (top, bottom) {
        (Some(top), Some(bottom)) => steps.clamp(bottom as i64 - last as i64, top as i64),
        _ => 0,
    };
    let moved = origins.iter();
    moved
        .map(|origin| moved_note(rows, clip_length, origin.clone(), delta, steps))
        .collect()
}

/// The notes whose rectangles a rectangle of the note area touches, as indices of the clip.
pub fn notes_in(viewport: &Viewport, rows: &Rows, clip: &Clip, area: Rect) -> Vec<usize> {
    let touches = |rect: Rect| {
        rect.x < area.x + area.width
            && area.x < rect.x + rect.width
            && rect.y < area.y + area.height
            && area.y < rect.y + rect.height
    };
    let notes = clip.notes.iter().enumerate();
    notes
        .filter(|(_, note)| note_rect(viewport, rows, clip, note).is_some_and(touches))
        .map(|(index, _)| index)
        .collect()
}

/// The top of the bar of a velocity in the lane.
pub fn velocity_y(velocity: Velocity) -> f32 {
    let bottom = VELOCITY_HEIGHT - VELOCITY_BOTTOM;
    bottom - (bottom - VELOCITY_TOP) * f32::from(velocity.value()) / 127.0
}

/// The velocity whose bar has its top at `y` in the lane. Above the lane it is 127, below 1.
pub fn velocity_at(y: f32) -> Velocity {
    let bottom = VELOCITY_HEIGHT - VELOCITY_BOTTOM;
    let value = (bottom - y) / (bottom - VELOCITY_TOP) * 127.0;
    Velocity::nearest(value.round() as i64)
}

/// A velocity moved as the pointer moves `dy` in the lane, so the top of its bar stays under
/// the pointer: up is louder. It stops at 1 and at 127.
pub fn moved_velocity(origin: Velocity, dy: f32) -> Velocity {
    if dy == 0.0 {
        return origin;
    }
    velocity_at(velocity_y(origin) + dy)
}

/// The bar of a note in the lane, at the start of the note.
pub fn velocity_bar(viewport: &Viewport, clip: &Clip, note: &Note) -> Rect {
    let y = velocity_y(note.velocity);
    Rect {
        x: viewport.x_of(clip.start + note.start),
        y,
        width: VELOCITY_BAR_WIDTH,
        height: VELOCITY_HEIGHT - VELOCITY_BOTTOM - y,
    }
}

/// The notes whose bars are within reach of `x`, as indices of the clip. Every note of a chord
/// has its bar at the same place.
pub fn velocity_bars_at(viewport: &Viewport, clip: &Clip, x: f32) -> Vec<usize> {
    velocity_bars_between(viewport, clip, x - VELOCITY_REACH, x + VELOCITY_REACH)
}

/// The notes whose bars are between two places across, as indices of the clip: what a draw in
/// the lane passes over between two mouse moves.
pub fn velocity_bars_between(viewport: &Viewport, clip: &Clip, from: f32, to: f32) -> Vec<usize> {
    let (left, right) = if from <= to { (from, to) } else { (to, from) };
    let notes = clip.notes.iter().enumerate();
    notes
        .filter(|(_, note)| {
            let bar = velocity_bar(viewport, clip, note);
            bar.x + bar.width >= left && bar.x <= right
        })
        .map(|(index, _)| index)
        .collect()
}

/// The note with its end moved by `delta`: at least `unit` long, or as short as it
/// was, and it ends at the clip end at the latest.
pub fn resized_note(clip_length: Length, origin: Note, delta: i64, unit: Ticks) -> Note {
    if delta == 0 {
        return origin;
    }
    let room = clip_length.ticks().0.saturating_sub(origin.start.0);
    let length = shifted(origin.length.ticks(), delta).0;
    let length = length.max(shortest(origin.length.ticks(), unit)).min(room);
    Note {
        length: Length::at_least_one(Ticks(length)),
        ..origin
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sound_notes::{Grid as PitchGrid, Row, View, piano_roll};

    use super::*;

    const BAR: u64 = 3840;
    /// A sixteenth in 4/4, the snap the window starts with.
    fn grid() -> Grid {
        super::super::snap::Snap::Sixteenth.grid(&sound_core::TimeSignatures::default())
    }

    const UNIT: Ticks = Ticks(240);

    fn pitch(number: u8) -> Pitch {
        Pitch::new(number).unwrap()
    }

    fn note(start: u64, length: u64, number: u8) -> Note {
        Note::new(
            Ticks(start),
            Length::new(Ticks(length)).unwrap(),
            pitch(number),
            Velocity::new(100).unwrap(),
        )
    }

    fn clip(start: u64, length: u64, notes: Vec<Note>) -> Clip {
        Clip::new(Ticks(start), Length::new(Ticks(length)).unwrap(), notes)
    }

    fn length(ticks: u64) -> Length {
        Length::new(Ticks(ticks)).unwrap()
    }

    fn piano() -> Rows {
        Rows::new(piano_roll().grid_for(&View::default()), [])
    }

    /// Kick, snare and hat, 20 pt, hits.
    fn drums() -> Rows {
        let grid = PitchGrid {
            rows: [36, 38, 42]
                .map(|number| Row {
                    pitch: pitch(number),
                    label: None,
                    shade: false,
                })
                .to_vec(),
            row_height: 20.0,
            notes: NoteShape::Hit,
        };
        Rows::new(Arc::new(grid), [])
    }

    #[test]
    fn a_note_is_drawn_at_its_project_position_and_ends_with_its_clip() {
        let viewport = Viewport::default();
        let clip = clip(BAR, BAR, vec![note(960, 960, 127), note(2880, 9600, 126)]);
        let first = note_rect(&viewport, &piano(), &clip, &clip.notes[0]).unwrap();
        assert_eq!((first.x, first.y), (LEAD_IN + 96.0 + 24.0, 0.0));
        assert_eq!((first.width, first.height), (23.0, 11.0));
        let long = note_rect(&viewport, &piano(), &clip, &clip.notes[1]).unwrap();
        assert_eq!(long.x + long.width, viewport.x_of(clip.end()) - 1.0);
        assert_eq!(long.y, 12.0);
    }

    #[test]
    fn a_hit_is_a_square_at_its_start_with_no_end_to_drag() {
        let viewport = Viewport {
            pixels_per_quarter: 96.0,
            ..Viewport::default()
        };
        let clip = clip(0, BAR, vec![note(0, 960, 38), note(0, 960, 40)]);
        let rect = note_rect(&viewport, &drums(), &clip, &clip.notes[0]).unwrap();
        assert_eq!(
            rect,
            Rect {
                x: LEAD_IN,
                y: 20.0,
                width: 19.0,
                height: 19.0
            }
        );
        // A pitch without a row is not drawn: the editor gives it an extra row first.
        assert_eq!(note_rect(&viewport, &drums(), &clip, &clip.notes[1]), None);
        let right_edge = LEAD_IN + 18.0;
        assert_eq!(
            note_at(&viewport, &drums(), &clip, right_edge, 25.0),
            Some((0, Zone::Body))
        );
        assert_eq!(
            note_at(&viewport, &drums(), &clip, LEAD_IN + 30.0, 25.0),
            None
        );
    }

    #[test]
    fn a_note_is_hit_on_its_body_or_its_right_edge_and_the_last_one_is_on_top() {
        let viewport = Viewport {
            pixels_per_quarter: 96.0,
            ..Viewport::default()
        };
        let clip = clip(0, BAR, vec![note(0, 960, 127), note(480, 960, 127)]);
        // The first note is 95 px wide from x 8, the second starts at 56.
        assert_eq!(
            note_at(&viewport, &piano(), &clip, 9.0, 5.0),
            Some((0, Zone::Body))
        );
        assert_eq!(
            note_at(&viewport, &piano(), &clip, 60.0, 5.0),
            Some((1, Zone::Body))
        );
        assert_eq!(
            note_at(&viewport, &piano(), &clip, 150.0, 5.0),
            Some((1, Zone::RightEdge))
        );
        assert_eq!(note_at(&viewport, &piano(), &clip, 60.0, 12.0), None);
        assert_eq!(note_at(&viewport, &piano(), &clip, 300.0, 5.0), None);
    }

    #[test]
    fn the_editor_opens_on_the_clip_and_the_middle_of_its_notes() {
        let clip = clip(4 * BAR, 4 * BAR, vec![note(0, 480, 48), note(480, 480, 72)]);
        let rows = piano();
        let viewport = opened(&clip, &rows, 1264.0, 320.0);
        // 16 quarters in 1264 - 8 - 96 px.
        assert_eq!(viewport.pixels_per_quarter, 72.5);
        assert_eq!(viewport.x_of(clip.start), LEAD_IN);
        // Pitch 60 is the middle of 48 and 72: the middle of its row is in the middle of the
        // 320 px of the area.
        let middle_of_60 = |viewport: &Viewport| rows.y_of(viewport, pitch(60)).unwrap() + 6.0;
        assert_eq!(middle_of_60(&viewport), 160.0);

        let empty = self::clip(0, BAR, vec![]);
        let viewport = opened(&empty, &rows, 1264.0, 320.0);
        assert_eq!(viewport.pixels_per_quarter, 192.0);
        assert_eq!(middle_of_60(&viewport), 160.0);
        let long = self::clip(0, 400 * BAR, vec![]);
        assert_eq!(opened(&long, &rows, 1264.0, 320.0).pixels_per_quarter, 24.0);

        // In a drum grid of three rows, a clip without notes opens on the row nearest to 60.
        let drums = drums();
        let viewport = opened(&empty, &drums, 1264.0, 320.0);
        assert_eq!(drums.y_of(&viewport, pitch(42)).unwrap() + 10.0, 160.0);
    }

    #[test]
    fn the_editor_scrolls_over_all_pitches_and_no_further() {
        let four_four = &TimeSignatures::default();
        let clip = clip(0, BAR, vec![]);
        let far = Viewport::default().scrolled(0.0, -100_000.0);
        let clamped_far = clamped(&far, &piano(), &clip, four_four, 1264.0, 320.0);
        // 128 rows, less the height.
        assert_eq!(clamped_far.scroll_y, 128.0 * 12.0 - 320.0);
        let clamped_drums = clamped(&far, &drums(), &clip, four_four, 1264.0, 40.0);
        assert_eq!(clamped_drums.scroll_y, 20.0);
        let before = Viewport::default().scrolled(500.0, 500.0);
        let clamped_before = clamped(&before, &piano(), &clip, four_four, 1264.0, 320.0);
        assert_eq!(
            (clamped_before.scroll_x, clamped_before.scroll_y),
            (0.0, 0.0)
        );
    }

    #[test]
    fn a_drawn_note_starts_in_the_cell_under_the_mouse_and_follows_the_pointer() {
        let clip = clip(BAR, BAR, vec![]);
        let down = Ticks(BAR + 500);
        // A click without a move is one snap step.
        assert_eq!(
            drawn_note(&clip, down, down, pitch(64), &grid(), NoteShape::Bar),
            Some(note(480, 240, 64))
        );
        let drawn = drawn_note(
            &clip,
            down,
            Ticks(BAR + 1450),
            pitch(64),
            &grid(),
            NoteShape::Bar,
        );
        assert_eq!(drawn, Some(note(480, 960, 64)));
        // Back past the start: still one step. Past the clip end: it ends with the clip.
        let back = drawn_note(&clip, down, Ticks(0), pitch(64), &grid(), NoteShape::Bar);
        assert_eq!(back, Some(note(480, 240, 64)));
        let past = drawn_note(
            &clip,
            down,
            Ticks(9 * BAR),
            pitch(64),
            &grid(),
            NoteShape::Bar,
        );
        assert_eq!(past, Some(note(480, BAR - 480, 64)));
        assert_eq!(drawn.map(|note| note.velocity.value()), Some(100));
        // A hit is one step, wherever the pointer goes.
        let hit = drawn_note(
            &clip,
            down,
            Ticks(BAR + 1450),
            pitch(38),
            &grid(),
            NoteShape::Hit,
        );
        assert_eq!(hit, Some(note(480, 240, 38)));
    }

    #[test]
    fn no_note_is_drawn_outside_the_clip() {
        let clip = clip(BAR, BAR, vec![]);
        assert_eq!(
            drawn_note(
                &clip,
                Ticks(BAR - 1),
                Ticks(BAR + 960),
                pitch(60),
                &grid(),
                NoteShape::Bar
            ),
            None
        );
        assert_eq!(
            drawn_note(
                &clip,
                Ticks(2 * BAR),
                Ticks(3 * BAR),
                pitch(60),
                &grid(),
                NoteShape::Bar
            ),
            None
        );
        let last_cell = drawn_note(
            &clip,
            Ticks(2 * BAR - 1),
            Ticks(3 * BAR),
            pitch(60),
            &grid(),
            NoteShape::Bar,
        );
        assert_eq!(last_cell, Some(note(BAR - 240, 240, 60)));

        // A clip that starts off the grid: the first note starts with the clip.
        let off_grid = self::clip(100, BAR, vec![]);
        let first = drawn_note(
            &off_grid,
            Ticks(150),
            Ticks(150),
            pitch(60),
            &grid(),
            NoteShape::Bar,
        );
        assert_eq!(first, Some(note(0, 240, 60)));
        // At tick 0 and at pitch 0 and 127.
        let at_zero = self::clip(0, BAR, vec![]);
        for number in [0, 127] {
            let drawn = drawn_note(
                &at_zero,
                Ticks(0),
                Ticks(0),
                pitch(number),
                &grid(),
                NoteShape::Bar,
            );
            assert_eq!(drawn, Some(note(0, 240, number)));
        }
    }

    #[test]
    fn a_moved_note_stays_whole_inside_its_clip() {
        let origin = note(960, 480, 60);
        assert_eq!(
            moved_note(&piano(), length(BAR), origin.clone(), 0, 0),
            origin
        );
        assert_eq!(
            moved_note(&piano(), length(BAR), origin.clone(), 480, 3),
            note(1440, 480, 63)
        );
        assert_eq!(
            moved_note(&piano(), length(BAR), origin.clone(), -5000, -100),
            note(0, 480, 0)
        );
        assert_eq!(
            moved_note(&piano(), length(BAR), origin.clone(), 50_000, 100),
            note(BAR - 480, 480, 127)
        );

        // Off the grid stays off the grid.
        assert_eq!(
            moved_note(&piano(), length(BAR), note(250, 480, 60), 240, 0),
            note(490, 480, 60)
        );
        // A note that reaches past the clip end moves left and never right.
        let long = note(960, 2 * BAR, 60);
        assert_eq!(
            moved_note(&piano(), length(BAR), long.clone(), 240, 1),
            note(960, 2 * BAR, 61)
        );
        assert_eq!(
            moved_note(&piano(), length(BAR), long.clone(), -240, 0),
            note(720, 2 * BAR, 60)
        );
        for delta in [-100_000, -240, 0, 240, 100_000] {
            assert!(moved_note(&piano(), length(BAR), origin.clone(), delta, 0).start < Ticks(BAR));
            assert!(moved_note(&piano(), length(BAR), long.clone(), delta, 0).start < Ticks(BAR));
        }
    }

    #[test]
    fn several_notes_move_as_a_whole_and_keep_their_distances() {
        let origins = [note(480, 480, 60), note(960, 960, 72)];
        assert_eq!(
            moved_notes(&piano(), length(BAR), &origins, 240, 2),
            [note(720, 480, 62), note(1200, 960, 74)]
        );
        // The first stops at the clip start, the second at the clip end, and both keep their
        // distance: 480 ticks and an octave.
        assert_eq!(
            moved_notes(&piano(), length(BAR), &origins, -5000, 0),
            [note(0, 480, 60), note(480, 960, 72)]
        );
        assert_eq!(
            moved_notes(&piano(), length(BAR), &origins, 5000, 0),
            [note(BAR - 1440, 480, 60), note(BAR - 960, 960, 72)]
        );
        assert_eq!(
            moved_notes(&piano(), length(BAR), &origins, 0, 100),
            [note(480, 480, 115), note(960, 960, 127)]
        );
        assert_eq!(
            moved_notes(&piano(), length(BAR), &origins, 0, -100),
            [note(480, 480, 0), note(960, 960, 12)]
        );
        assert!(moved_notes(&piano(), length(BAR), &[], 240, 1).is_empty());
        // In the drum grid a step is a row, and the notes keep their distance in rows.
        let hits = [note(0, 240, 36), note(240, 240, 38)];
        assert_eq!(
            moved_notes(&drums(), length(BAR), &hits, 0, 1),
            [note(0, 240, 38), note(240, 240, 42)]
        );
        assert_eq!(
            moved_notes(&drums(), length(BAR), &hits, 0, 5),
            [note(0, 240, 38), note(240, 240, 42)]
        );
        assert_eq!(moved_notes(&drums(), length(BAR), &hits, 0, -1), hits);
    }

    #[test]
    fn a_rectangle_touches_the_notes_it_overlaps() {
        let viewport = Viewport {
            pixels_per_quarter: 96.0,
            ..Viewport::default()
        };
        let clip = clip(0, BAR, vec![note(0, 960, 127), note(1920, 960, 126)]);
        let area = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        // The first note is from x 8 to 103 on the top row, the second from 200 on the next.
        assert_eq!(
            notes_in(&viewport, &piano(), &clip, area(50.0, 0.0, 10.0, 5.0)),
            [0]
        );
        assert_eq!(
            notes_in(&viewport, &piano(), &clip, area(50.0, 5.0, 200.0, 10.0)),
            [0, 1]
        );
        assert!(notes_in(&viewport, &piano(), &clip, area(110.0, 0.0, 50.0, 40.0)).is_empty());
    }

    #[test]
    fn a_velocity_bar_is_as_tall_as_its_velocity_and_a_drag_keeps_its_top_under_the_pointer() {
        let velocity = |value| Velocity::new(value).unwrap();
        assert_eq!(velocity_y(velocity(127)), VELOCITY_TOP);
        assert_eq!(velocity_at(VELOCITY_TOP), velocity(127));
        assert_eq!(velocity_at(-100.0), velocity(127));
        assert_eq!(velocity_at(VELOCITY_HEIGHT), velocity(1));
        for value in [1, 40, 64, 100, 127] {
            assert_eq!(velocity_at(velocity_y(velocity(value))), velocity(value));
        }
        assert_eq!(moved_velocity(velocity(100), 0.0), velocity(100));
        assert!(moved_velocity(velocity(100), -5.0) > velocity(100));
        assert!(moved_velocity(velocity(100), 5.0) < velocity(100));
        assert_eq!(moved_velocity(velocity(100), -500.0), velocity(127));
        assert_eq!(moved_velocity(velocity(100), 500.0), velocity(1));

        let viewport = Viewport {
            pixels_per_quarter: 96.0,
            ..Viewport::default()
        };
        // A chord at the start and one note a beat later, at x 8 and 104.
        let clip = clip(
            0,
            BAR,
            vec![note(0, 480, 60), note(0, 480, 64), note(960, 480, 67)],
        );
        let bar = velocity_bar(&viewport, &clip, &clip.notes[2]);
        assert_eq!((bar.x, bar.width), (104.0, VELOCITY_BAR_WIDTH));
        assert_eq!(bar.y + bar.height, VELOCITY_HEIGHT - VELOCITY_BOTTOM);
        assert_eq!(velocity_bars_at(&viewport, &clip, 9.0), [0, 1]);
        assert_eq!(velocity_bars_at(&viewport, &clip, 110.0), [2]);
        assert!(velocity_bars_at(&viewport, &clip, 60.0).is_empty());
        assert_eq!(
            velocity_bars_between(&viewport, &clip, 200.0, 0.0),
            [0, 1, 2]
        );
        assert_eq!(velocity_bars_between(&viewport, &clip, 50.0, 105.0), [2]);
    }

    #[test]
    fn a_resized_note_keeps_a_snap_step_and_ends_with_the_clip_at_the_latest() {
        let origin = note(960, 480, 60);
        assert_eq!(resized_note(length(BAR), origin.clone(), 0, UNIT), origin);
        assert_eq!(
            resized_note(length(BAR), origin.clone(), 480, UNIT),
            note(960, 960, 60)
        );
        assert_eq!(
            resized_note(length(BAR), origin.clone(), -10_000, UNIT),
            note(960, 240, 60)
        );
        assert_eq!(
            resized_note(length(BAR), origin, 100_000, UNIT),
            note(960, BAR - 960, 60)
        );
        let tiny = note(0, 100, 60);
        assert_eq!(resized_note(length(BAR), tiny.clone(), -240, UNIT), tiny);
        // Untouched, a note that reaches past the clip end stays as it is.
        let long = note(960, 2 * BAR, 60);
        assert_eq!(resized_note(length(BAR), long.clone(), 0, UNIT), long);
        assert_eq!(
            resized_note(length(BAR), long, -240, UNIT),
            note(960, BAR - 960, 60)
        );
    }

    #[test]
    fn a_moved_or_resized_note_keeps_its_view() {
        let mut origin = note(960, 480, 60);
        origin.view = serde_json::from_str(r#"{"accent":true}"#).unwrap();
        let moved = moved_note(&piano(), length(BAR), origin.clone(), 240, 2);
        assert_eq!(moved.view, origin.view);
        let several = moved_notes(&piano(), length(BAR), std::slice::from_ref(&origin), 240, 2);
        assert_eq!(several[0].view, origin.view);
        let resized = resized_note(length(BAR), origin.clone(), 240, UNIT);
        assert_eq!(resized.view, origin.view);
    }
}
