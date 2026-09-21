//! What a drag or a key does to a clip. Pure math, no GPUI and no project: the view finds the
//! clip and the pointer, these functions say what the clip becomes.
//!
//! A drag is a delta in whole snap steps from where it began ([`super::layout::snapped_delta`]),
//! always applied to the clip as it was at mouse down. So dragging out and back again gives the
//! clip back as it was, with every note.

use sound_core::{Ticks, TimeSignature};
use sound_notes::{Clip, Length};

use super::layout::{Rect, SNAP, shifted, snap_floor};

/// How far into a clip or a note its edges reach, in pixels.
pub const EDGE_ZONE: f32 = 6.0;

/// The part of a clip or a note under the pointer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Zone {
    Body,
    LeftEdge,
    RightEdge,
}

/// The zone at `x`, for a pointer inside `rect`. The edges of a narrow shape shrink, so half
/// of it is always body and it can still be moved.
pub fn zone_at(rect: Rect, x: f32) -> Zone {
    let edge = EDGE_ZONE.min(rect.width / 4.0);
    if x < rect.x + edge {
        Zone::LeftEdge
    } else if x >= rect.x + rect.width - edge {
        Zone::RightEdge
    } else {
        Zone::Body
    }
}

/// The empty clip of one bar that a double click makes, in the grid cell under the pointer.
pub fn new_clip(at: Ticks, time_signature: TimeSignature) -> Clip {
    Clip::new(
        snap_floor(at),
        Length::at_least_one(Ticks(time_signature.ticks_per_bar())),
        Vec::new(),
    )
}

/// A shape does not get shorter than one snap step, or than it already was.
pub(super) fn shortest(length: Length) -> u64 {
    SNAP.0.min(length.ticks().0)
}

/// The clip with its right edge moved by `delta`. Notes that would start outside are dropped,
/// as `Clip::set_length` says. They come back when the same drag goes out again, because
/// every move starts from `origin`.
pub fn resized_right(origin: &Clip, delta: i64) -> Clip {
    let mut clip = origin.clone();
    if delta != 0 {
        let length = shifted(origin.length.ticks(), delta).0;
        clip.set_length(Length::at_least_one(Ticks(
            length.max(shortest(origin.length)),
        )));
    }
    clip
}

/// The clip with its left edge moved by `delta`. The notes stay where they are in the project,
/// so their starts change by the same amount the other way. None is dropped: the edge stops at
/// the first note, and at tick 0, and one snap step before the right edge.
pub fn resized_left(origin: &Clip, delta: i64) -> Clip {
    let first_note = origin.notes.iter().map(|note| note.start.0).min();
    let room = origin.length.ticks().0 - shortest(origin.length);
    let latest = first_note.map_or(room, |first| first.min(room));
    let delta = delta.clamp(-(origin.start.0 as i64), latest as i64);
    let mut clip = origin.clone();
    clip.start = shifted(origin.start, delta);
    clip.length = Length::at_least_one(shifted(origin.length.ticks(), -delta));
    for note in &mut clip.notes {
        note.start = shifted(note.start, -delta);
    }
    clip
}

/// The track row an arrow key moves a clip to. It stops at the first and at the last.
pub fn nudged_track(current: usize, tracks: usize, step: i64) -> usize {
    let last = tracks.saturating_sub(1);
    current.saturating_add_signed(step as isize).min(last)
}

#[cfg(test)]
mod tests {
    use sound_notes::{Note, Pitch, Velocity};

    use super::*;

    const BAR: u64 = 3840;

    fn clip(start: u64, length: u64, notes: &[(u64, u64)]) -> Clip {
        Clip::new(
            Ticks(start),
            Length::new(Ticks(length)).unwrap(),
            notes
                .iter()
                .map(|&(start, length)| Note {
                    start: Ticks(start),
                    length: Length::new(Ticks(length)).unwrap(),
                    pitch: Pitch::new(60).unwrap(),
                    velocity: Velocity::new(100).unwrap(),
                })
                .collect(),
        )
    }

    fn rect(x: f32, width: f32) -> Rect {
        Rect {
            x,
            y: 0.0,
            width,
            height: 56.0,
        }
    }

    #[test]
    fn the_edges_of_a_shape_are_six_pixels_and_shrink_for_a_narrow_one() {
        let wide = rect(100.0, 96.0);
        assert_eq!(zone_at(wide, 100.0), Zone::LeftEdge);
        assert_eq!(zone_at(wide, 105.9), Zone::LeftEdge);
        assert_eq!(zone_at(wide, 106.0), Zone::Body);
        assert_eq!(zone_at(wide, 189.9), Zone::Body);
        assert_eq!(zone_at(wide, 190.0), Zone::RightEdge);

        // 8 px wide: 2 px edges, 4 px of body.
        let narrow = rect(100.0, 8.0);
        assert_eq!(zone_at(narrow, 101.9), Zone::LeftEdge);
        assert_eq!(zone_at(narrow, 102.0), Zone::Body);
        assert_eq!(zone_at(narrow, 105.9), Zone::Body);
        assert_eq!(zone_at(narrow, 106.0), Zone::RightEdge);
    }

    #[test]
    fn a_new_clip_is_one_bar_in_the_cell_under_the_pointer() {
        let four_four = TimeSignature::new(4, 4).unwrap();
        assert_eq!(
            new_clip(Ticks(BAR + 479), four_four),
            clip(BAR + 240, BAR, &[])
        );
        assert_eq!(new_clip(Ticks(0), four_four), clip(0, BAR, &[]));
        let waltz = TimeSignature::new(3, 4).unwrap();
        assert_eq!(new_clip(Ticks(100), waltz), clip(0, 2880, &[]));
    }

    #[test]
    fn the_right_edge_changes_the_length_and_going_back_restores_the_notes() {
        let origin = clip(BAR, BAR, &[(0, 480), (2880, 480)]);
        assert_eq!(resized_right(&origin, 0), origin);
        assert_eq!(resized_right(&origin, 960).length.ticks(), Ticks(BAR + 960));

        // In past the second note, then out again: every move starts from the origin.
        let shorter = resized_right(&origin, -1920);
        assert_eq!(shorter, clip(BAR, 1920, &[(0, 480)]));
        assert_eq!(resized_right(&origin, -240).notes.len(), 2);

        // Never shorter than one snap step, however far the pointer goes.
        assert_eq!(resized_right(&origin, -100_000).length.ticks(), SNAP);
        // A clip that was shorter than a step keeps its length and can still grow.
        let tiny = clip(0, 100, &[]);
        assert_eq!(resized_right(&tiny, -240), tiny);
        assert_eq!(resized_right(&tiny, 240).length.ticks(), Ticks(340));
    }

    #[test]
    fn the_left_edge_keeps_notes_where_they_are_and_stops_at_the_first_note() {
        let origin = clip(BAR, BAR, &[(960, 480), (2880, 480)]);
        assert_eq!(resized_left(&origin, 0), origin);

        // Out to the left: the clip grows and the note starts grow with it.
        let grown = resized_left(&origin, -480);
        assert_eq!(
            grown,
            clip(BAR - 480, BAR + 480, &[(1440, 480), (3360, 480)])
        );
        // In to the right: the first note ends up at the start and the edge stops there.
        let at_note = clip(BAR + 960, BAR - 960, &[(0, 480), (1920, 480)]);
        assert_eq!(resized_left(&origin, 960), at_note);
        assert_eq!(resized_left(&origin, 2400), at_note);
        for delta in [-480, 480, 960, 100_000] {
            let resized = resized_left(&origin, delta);
            assert_eq!(resized.end(), origin.end());
            let placed: Vec<_> = resized.placed_notes().collect();
            assert_eq!(placed, origin.placed_notes().collect::<Vec<_>>());
        }
    }

    #[test]
    fn the_left_edge_stops_at_tick_zero_and_before_the_right_edge() {
        let origin = clip(480, BAR, &[]);
        assert_eq!(resized_left(&origin, -960), clip(0, BAR + 480, &[]));
        assert_eq!(
            resized_left(&origin, 100_000),
            clip(480 + BAR - 240, 240, &[])
        );
        let at_zero = clip(0, BAR, &[(0, 480)]);
        assert_eq!(resized_left(&at_zero, -240), at_zero);
        assert_eq!(resized_left(&at_zero, 240), at_zero);
    }

    #[test]
    fn arrow_keys_stop_at_the_first_and_the_last_track() {
        assert_eq!(nudged_track(0, 3, -1), 0);
        assert_eq!(nudged_track(0, 3, 1), 1);
        assert_eq!(nudged_track(2, 3, 1), 2);
        assert_eq!(nudged_track(0, 1, 1), 0);
    }
}
