//! Where things are in the note editor, and what a drag or a key does to a note. Pure math,
//! no GPUI and no project, like [`super::layout`].
//!
//! The editor shows one clip as a piano roll. Time runs across on the project timeline, with
//! the same [`Viewport`] math as the arrangement, so bar numbers and the playhead mean the
//! same in both. Pitch runs up: `y` 0 is the top of the row of pitch 127 when nothing is
//! scrolled. The coordinates are those of the note area, right of the keys and below the ruler.

use std::ops::RangeInclusive;

use sound_core::{TICKS_PER_QUARTER, Ticks, TimeSignature};
use sound_notes::{Clip, Length, Note, Pitch, Velocity};

use super::gesture::{Zone, shortest, zone_at};
use super::layout::{LEAD_IN, Rect, Viewport, shifted};
use super::snap::{Grid, snap, snap_floor};

/// The height of the editor panel, ruler included.
pub const EDITOR_HEIGHT: f32 = 352.0;
/// The height of the row of one pitch.
pub const KEY_HEIGHT: f32 = 12.0;
/// The width of the key strip, at the right edge of the header column.
pub const KEYS_WIDTH: f32 = 32.0;
/// The velocity of a note that is drawn. There is no velocity lane yet.
pub const DRAWN_VELOCITY: i64 = 100;
/// The pitch in the middle of the editor of a clip without notes: middle C.
const MIDDLE_PITCH: u8 = 60;
const PITCHES: u8 = 128;

/// Zoom limits when the editor opens. It fits the clip, but a long clip stays readable and a
/// short one does not fill the screen with one bar.
const OPENING_PIXELS_PER_QUARTER: RangeInclusive<f64> = 24.0..=192.0;
/// Room right of the clip when the editor opens.
const OPENING_ROOM: f32 = 96.0;

/// The top of the row of a pitch.
pub fn y_of(viewport: &Viewport, pitch: Pitch) -> f32 {
    let row = f64::from(PITCHES - 1 - pitch.number());
    (row * f64::from(KEY_HEIGHT) - viewport.scroll_y) as f32
}

/// The pitch of the row at `y`. `None` above pitch 127 and below pitch 0.
pub fn pitch_at(viewport: &Viewport, y: f32) -> Option<Pitch> {
    let row = ((f64::from(y) + viewport.scroll_y) / f64::from(KEY_HEIGHT)).floor();
    let in_range = (0.0..f64::from(PITCHES)).contains(&row);
    let pitch = in_range.then(|| PITCHES - 1 - row as u8)?;
    Pitch::new(pitch).ok()
}

/// The pitch a drag is at: the nearest row when the pointer is above or below all of them.
pub fn nearest_pitch(viewport: &Viewport, y: f32) -> Pitch {
    let row = ((f64::from(y) + viewport.scroll_y) / f64::from(KEY_HEIGHT)).floor();
    Pitch::nearest(i64::from(PITCHES - 1) - row as i64)
}

/// The pitch moved by semitones. It stops at 0 and at 127.
pub fn transposed(pitch: Pitch, semitones: i32) -> Pitch {
    Pitch::nearest(i64::from(pitch.number()) + i64::from(semitones))
}

/// The pitches whose rows show, whole or in part, in a note area of this height.
pub fn visible_pitches(viewport: &Viewport, height: f32) -> RangeInclusive<u8> {
    let highest = nearest_pitch(viewport, 0.0).number();
    let lowest = nearest_pitch(viewport, height - 0.01).number();
    lowest..=highest
}

pub fn is_black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

/// Only the Cs are named: `C4` is pitch 60, `C-1` is pitch 0.
pub fn key_label(pitch: u8) -> Option<String> {
    pitch
        .is_multiple_of(12)
        .then(|| format!("C{}", i32::from(pitch / 12) - 1))
}

/// Where a note of the clip is drawn. A note that is longer than the rest of its clip ends
/// with the clip, as it sounds.
pub fn note_rect(viewport: &Viewport, clip: &Clip, note: &Note) -> Rect {
    let x = viewport.x_of(clip.start + note.start);
    let end = (clip.start + note.end()).min(clip.end());
    Rect {
        x,
        y: y_of(viewport, note.pitch),
        // A gap of a pixel to the next note and to the next row.
        width: (viewport.x_of(end) - x - 1.0).max(2.0),
        height: KEY_HEIGHT - 1.0,
    }
}

/// The note on top at a position, as its index in the clip, with the zone that was hit. A
/// note has no left edge: only its end is dragged.
pub fn note_at(viewport: &Viewport, clip: &Clip, x: f32, y: f32) -> Option<(usize, Zone)> {
    let notes = clip.notes.iter().enumerate().rev();
    let hit = notes
        .map(|(index, note)| (index, note_rect(viewport, clip, note)))
        .find(|(_, rect)| rect.contains(x, y));
    hit.map(|(index, rect)| match zone_at(rect, x) {
        Zone::RightEdge => (index, Zone::RightEdge),
        Zone::Body | Zone::LeftEdge => (index, Zone::Body),
    })
}

/// The viewport of an editor that opens for a clip in a note area of this size: the clip
/// starts at the left and fits the width, and the middle of its notes is in the middle of
/// the area.
pub fn opened(clip: &Clip, width: f32, height: f32) -> Viewport {
    let quarters = clip.length.ticks().0 as f64 / TICKS_PER_QUARTER as f64;
    let fitted = f64::from((width - LEAD_IN - OPENING_ROOM).max(1.0)) / quarters;
    let pixels_per_quarter = fitted.clamp(
        *OPENING_PIXELS_PER_QUARTER.start(),
        *OPENING_PIXELS_PER_QUARTER.end(),
    );
    let pitches = clip.notes.iter().map(|note| note.pitch.number());
    let middle = match (pitches.clone().min(), pitches.max()) {
        (Some(lowest), Some(highest)) => (f64::from(lowest) + f64::from(highest)) / 2.0,
        _ => f64::from(MIDDLE_PITCH),
    };
    let middle_y = (f64::from(PITCHES - 1) - middle + 0.5) * f64::from(KEY_HEIGHT);
    Viewport {
        pixels_per_quarter,
        scroll_x: clip.start.0 as f64 * pixels_per_quarter / TICKS_PER_QUARTER as f64,
        scroll_y: (middle_y - f64::from(height) / 2.0).round(),
    }
}

/// Keeps the scroll inside the pitch range and the time up to the end of the clip.
pub fn clamped(
    viewport: &Viewport,
    clip: &Clip,
    time_signature: TimeSignature,
    width: f32,
    height: f32,
) -> Viewport {
    let content_height = f64::from(PITCHES) * f64::from(KEY_HEIGHT);
    viewport.clamped_to(clip.end(), content_height, time_signature, width, height)
}

/// The note that a drag draws: it starts in the grid cell where the mouse went down, with the
/// pitch of that row, and ends at the pointer, at least one unit of the grid later and at most where
/// the clip ends. `None` when the mouse went down outside the clip: a note must start inside.
/// Both ticks are project positions.
pub fn drawn_note(
    clip: &Clip,
    down: Ticks,
    pointer: Ticks,
    pitch: Pitch,
    grid: Grid,
) -> Option<Note> {
    if down < clip.start || down >= clip.end() {
        return None;
    }
    // The grid is that of the project. In a clip that starts off the grid, the first cell
    // starts with the clip.
    let start = snap_floor(down, grid.step).max(clip.start);
    let end = snap(pointer, grid.step)
        .max(start + grid.unit)
        .min(clip.end());
    Some(Note {
        start: Ticks(start.0 - clip.start.0),
        length: Length::at_least_one(Ticks(end.0 - start.0)),
        pitch,
        velocity: Velocity::nearest(DRAWN_VELOCITY),
    })
}

/// The note moved by a delta in time and in pitch. It stays whole inside the clip: it stops
/// at the clip start, where its end meets the clip end, and at pitch 0 and 127.
pub fn moved_note(clip_length: Length, origin: Note, delta: i64, semitones: i32) -> Note {
    let room = clip_length
        .ticks()
        .0
        .saturating_sub(origin.length.ticks().0);
    // A note that already reaches past the clip end can go left and does not have to.
    let latest = Ticks(room.max(origin.start.0));
    Note {
        start: shifted(origin.start, delta).min(latest),
        pitch: transposed(origin.pitch, semitones),
        ..origin
    }
}

/// The note with its end moved by `delta`: at least `unit` long, or as short as it
/// was, and it ends at the clip end at the latest.
pub fn resized_note(clip_length: Length, origin: Note, delta: i64, unit: Ticks) -> Note {
    if delta == 0 {
        return origin;
    }
    let room = clip_length.ticks().0.saturating_sub(origin.start.0);
    let length = shifted(origin.length.ticks(), delta).0;
    let length = length.max(shortest(origin.length, unit)).min(room);
    Note {
        length: Length::at_least_one(Ticks(length)),
        ..origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: u64 = 3840;
    /// A sixteenth, the snap the window starts with.
    const GRID: Grid = Grid {
        step: Ticks(240),
        unit: Ticks(240),
    };

    fn pitch(number: u8) -> Pitch {
        Pitch::new(number).unwrap()
    }

    fn note(start: u64, length: u64, number: u8) -> Note {
        Note {
            start: Ticks(start),
            length: Length::new(Ticks(length)).unwrap(),
            pitch: pitch(number),
            velocity: Velocity::new(100).unwrap(),
        }
    }

    fn clip(start: u64, length: u64, notes: Vec<Note>) -> Clip {
        Clip::new(Ticks(start), Length::new(Ticks(length)).unwrap(), notes)
    }

    fn length(ticks: u64) -> Length {
        Length::new(Ticks(ticks)).unwrap()
    }

    #[test]
    fn pitches_and_rows_convert_both_ways() {
        let viewport = Viewport {
            scroll_y: 120.0,
            ..Viewport::default()
        };
        assert_eq!(y_of(&Viewport::default(), pitch(127)), 0.0);
        assert_eq!(y_of(&Viewport::default(), pitch(0)), 127.0 * KEY_HEIGHT);
        assert_eq!(y_of(&viewport, pitch(117)), 0.0);
        for number in [0, 1, 59, 60, 126, 127] {
            let y = y_of(&viewport, pitch(number));
            assert_eq!(pitch_at(&viewport, y), Some(pitch(number)));
            assert_eq!(
                pitch_at(&viewport, y + KEY_HEIGHT - 0.1),
                Some(pitch(number))
            );
        }
    }

    #[test]
    fn there_is_no_pitch_above_127_or_below_0_and_a_drag_stays_on_the_nearest() {
        let top = Viewport::default();
        assert_eq!(pitch_at(&top, -0.1), None);
        assert_eq!(pitch_at(&top, 0.0), Some(pitch(127)));
        assert_eq!(pitch_at(&top, 128.0 * KEY_HEIGHT - 0.1), Some(pitch(0)));
        assert_eq!(pitch_at(&top, 128.0 * KEY_HEIGHT), None);
        assert_eq!(nearest_pitch(&top, -500.0), pitch(127));
        assert_eq!(nearest_pitch(&top, 100_000.0), pitch(0));
        assert_eq!(nearest_pitch(&top, 13.0), pitch(126));

        assert_eq!(transposed(pitch(0), -1), pitch(0));
        assert_eq!(transposed(pitch(5), -12), pitch(0));
        assert_eq!(transposed(pitch(127), 1), pitch(127));
        assert_eq!(transposed(pitch(120), 12), pitch(127));
        assert_eq!(transposed(pitch(60), -12), pitch(48));
    }

    #[test]
    fn the_visible_pitches_are_what_overlaps_the_area() {
        let viewport = Viewport {
            scroll_y: 6.0,
            ..Viewport::default()
        };
        // Half of 127, all of 126 and half of 125 in 24 px.
        assert_eq!(visible_pitches(&viewport, 24.0), 125..=127);
        assert_eq!(visible_pitches(&Viewport::default(), 24.0), 126..=127);
        let bottom = Viewport {
            scroll_y: 127.0 * 12.0,
            ..Viewport::default()
        };
        assert_eq!(visible_pitches(&bottom, 300.0), 0..=0);
    }

    #[test]
    fn keys_are_black_or_white_and_only_the_cs_are_named() {
        let black: Vec<u8> = (60..72).filter(|pitch| is_black_key(*pitch)).collect();
        assert_eq!(black, [61, 63, 66, 68, 70]);
        assert_eq!(key_label(60).as_deref(), Some("C4"));
        assert_eq!(key_label(0).as_deref(), Some("C-1"));
        assert_eq!(key_label(120).as_deref(), Some("C9"));
        assert_eq!(key_label(61), None);
    }

    #[test]
    fn a_note_is_drawn_at_its_project_position_and_ends_with_its_clip() {
        let viewport = Viewport::default();
        let clip = clip(BAR, BAR, vec![note(960, 960, 127), note(2880, 9600, 126)]);
        let first = note_rect(&viewport, &clip, &clip.notes[0]);
        assert_eq!((first.x, first.y), (LEAD_IN + 96.0 + 24.0, 0.0));
        assert_eq!((first.width, first.height), (23.0, KEY_HEIGHT - 1.0));
        let long = note_rect(&viewport, &clip, &clip.notes[1]);
        assert_eq!(long.x + long.width, viewport.x_of(clip.end()) - 1.0);
        assert_eq!(long.y, KEY_HEIGHT);
    }

    #[test]
    fn a_note_is_hit_on_its_body_or_its_right_edge_and_the_last_one_is_on_top() {
        let viewport = Viewport {
            pixels_per_quarter: 96.0,
            ..Viewport::default()
        };
        let clip = clip(0, BAR, vec![note(0, 960, 127), note(480, 960, 127)]);
        // The first note is 95 px wide from x 8, the second starts at 56.
        assert_eq!(note_at(&viewport, &clip, 9.0, 5.0), Some((0, Zone::Body)));
        assert_eq!(note_at(&viewport, &clip, 60.0, 5.0), Some((1, Zone::Body)));
        assert_eq!(
            note_at(&viewport, &clip, 150.0, 5.0),
            Some((1, Zone::RightEdge))
        );
        assert_eq!(note_at(&viewport, &clip, 60.0, 12.0), None);
        assert_eq!(note_at(&viewport, &clip, 300.0, 5.0), None);
    }

    #[test]
    fn the_editor_opens_on_the_clip_and_the_middle_of_its_notes() {
        let clip = clip(4 * BAR, 4 * BAR, vec![note(0, 480, 48), note(480, 480, 72)]);
        let viewport = opened(&clip, 1264.0, 320.0);
        // 16 quarters in 1264 - 8 - 96 px.
        assert_eq!(viewport.pixels_per_quarter, 72.5);
        assert_eq!(viewport.x_of(clip.start), LEAD_IN);
        // Pitch 60 is the middle of 48 and 72: the middle of its row is in the middle of the
        // 320 px of the area.
        assert_eq!(y_of(&viewport, pitch(60)) + KEY_HEIGHT / 2.0, 160.0);

        let empty = self::clip(0, BAR, vec![]);
        let viewport = opened(&empty, 1264.0, 320.0);
        assert_eq!(viewport.pixels_per_quarter, 192.0);
        assert_eq!(y_of(&viewport, pitch(60)) + KEY_HEIGHT / 2.0, 160.0);
        let long = self::clip(0, 400 * BAR, vec![]);
        assert_eq!(opened(&long, 1264.0, 320.0).pixels_per_quarter, 24.0);
    }

    #[test]
    fn the_editor_scrolls_over_all_pitches_and_no_further() {
        let four_four = TimeSignature::new(4, 4).unwrap();
        let clip = clip(0, BAR, vec![]);
        let far = Viewport::default().scrolled(0.0, -100_000.0);
        let clamped_far = clamped(&far, &clip, four_four, 1264.0, 320.0);
        // 128 rows, less the height.
        assert_eq!(clamped_far.scroll_y, 128.0 * 12.0 - 320.0);
        let before = Viewport::default().scrolled(500.0, 500.0);
        let clamped_before = clamped(&before, &clip, four_four, 1264.0, 320.0);
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
            drawn_note(&clip, down, down, pitch(64), GRID),
            Some(note(480, 240, 64))
        );
        let drawn = drawn_note(&clip, down, Ticks(BAR + 1450), pitch(64), GRID);
        assert_eq!(drawn, Some(note(480, 960, 64)));
        // Back past the start: still one step. Past the clip end: it ends with the clip.
        let back = drawn_note(&clip, down, Ticks(0), pitch(64), GRID);
        assert_eq!(back, Some(note(480, 240, 64)));
        let past = drawn_note(&clip, down, Ticks(9 * BAR), pitch(64), GRID);
        assert_eq!(past, Some(note(480, BAR - 480, 64)));
        assert_eq!(drawn.map(|note| note.velocity.value()), Some(100));
    }

    #[test]
    fn no_note_is_drawn_outside_the_clip() {
        let clip = clip(BAR, BAR, vec![]);
        assert_eq!(
            drawn_note(&clip, Ticks(BAR - 1), Ticks(BAR + 960), pitch(60), GRID),
            None
        );
        assert_eq!(
            drawn_note(&clip, Ticks(2 * BAR), Ticks(3 * BAR), pitch(60), GRID),
            None
        );
        let last_cell = drawn_note(&clip, Ticks(2 * BAR - 1), Ticks(3 * BAR), pitch(60), GRID);
        assert_eq!(last_cell, Some(note(BAR - 240, 240, 60)));

        // A clip that starts off the grid: the first note starts with the clip.
        let off_grid = self::clip(100, BAR, vec![]);
        let first = drawn_note(&off_grid, Ticks(150), Ticks(150), pitch(60), GRID);
        assert_eq!(first, Some(note(0, 240, 60)));
        // At tick 0 and at pitch 0 and 127.
        let at_zero = self::clip(0, BAR, vec![]);
        for number in [0, 127] {
            let drawn = drawn_note(&at_zero, Ticks(0), Ticks(0), pitch(number), GRID);
            assert_eq!(drawn, Some(note(0, 240, number)));
        }
    }

    #[test]
    fn a_moved_note_stays_whole_inside_its_clip() {
        let origin = note(960, 480, 60);
        assert_eq!(moved_note(length(BAR), origin, 0, 0), origin);
        assert_eq!(moved_note(length(BAR), origin, 480, 3), note(1440, 480, 63));
        assert_eq!(
            moved_note(length(BAR), origin, -5000, -100),
            note(0, 480, 0)
        );
        assert_eq!(
            moved_note(length(BAR), origin, 50_000, 100),
            note(BAR - 480, 480, 127)
        );

        // Off the grid stays off the grid.
        assert_eq!(
            moved_note(length(BAR), note(250, 480, 60), 240, 0),
            note(490, 480, 60)
        );
        // A note that reaches past the clip end moves left and never right.
        let long = note(960, 2 * BAR, 60);
        assert_eq!(
            moved_note(length(BAR), long, 240, 1),
            note(960, 2 * BAR, 61)
        );
        assert_eq!(
            moved_note(length(BAR), long, -240, 0),
            note(720, 2 * BAR, 60)
        );
        for delta in [-100_000, -240, 0, 240, 100_000] {
            assert!(moved_note(length(BAR), origin, delta, 0).start < Ticks(BAR));
            assert!(moved_note(length(BAR), long, delta, 0).start < Ticks(BAR));
        }
    }

    #[test]
    fn a_resized_note_keeps_a_snap_step_and_ends_with_the_clip_at_the_latest() {
        let origin = note(960, 480, 60);
        assert_eq!(resized_note(length(BAR), origin, 0, GRID.unit), origin);
        assert_eq!(resized_note(length(BAR), origin, 480, GRID.unit), note(960, 960, 60));
        assert_eq!(
            resized_note(length(BAR), origin, -10_000, GRID.unit),
            note(960, 240, 60)
        );
        assert_eq!(
            resized_note(length(BAR), origin, 100_000, GRID.unit),
            note(960, BAR - 960, 60)
        );
        let tiny = note(0, 100, 60);
        assert_eq!(resized_note(length(BAR), tiny, -240, GRID.unit), tiny);
        // Untouched, a note that reaches past the clip end stays as it is.
        let long = note(960, 2 * BAR, 60);
        assert_eq!(resized_note(length(BAR), long, 0, GRID.unit), long);
        assert_eq!(
            resized_note(length(BAR), long, -240, GRID.unit),
            note(960, BAR - 960, 60)
        );
    }
}
