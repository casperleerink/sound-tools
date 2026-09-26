//! What a drag or a key does to a clip. Pure math, no GPUI and no project: the view finds the
//! clip and the pointer, these functions say what the clip becomes.
//!
//! A drag is a delta in whole snap steps from where it began ([`super::snap::snapped_delta`]),
//! always applied to the clip as it was at mouse down. So dragging out and back again gives the
//! clip back as it was, with every note.

use sound_core::{Ticks, TimeSignature};
use sound_notes::{Clip, Length};

use super::layout::{Rect, shifted};
use super::snap::snap_floor;

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
pub fn new_clip(at: Ticks, time_signature: TimeSignature, step: Ticks) -> Clip {
    Clip::new(
        snap_floor(at, step),
        Length::at_least_one(Ticks(time_signature.ticks_per_bar())),
        Vec::new(),
    )
}

/// A shape does not get shorter than `unit` (see [`super::snap::Snap::unit`]), or than it
/// already was.
pub(super) fn shortest(length: Length, unit: Ticks) -> u64 {
    unit.0.min(length.ticks().0)
}

/// The clip with its right edge moved by `delta`. Notes that would start outside are dropped,
/// as `Clip::set_length` says. They come back when the same drag goes out again, because
/// every move starts from `origin`.
pub fn resized_right(origin: &Clip, delta: i64, unit: Ticks) -> Clip {
    let mut clip = origin.clone();
    if delta != 0 {
        let length = shifted(origin.length.ticks(), delta).0;
        clip.set_length(Length::at_least_one(Ticks(
            length.max(shortest(origin.length, unit)),
        )));
    }
    clip
}

/// The clip with its left edge moved by `delta`. The notes stay where they are in the project,
/// so their starts change by the same amount the other way. None is dropped: the edge stops at
/// the first note, and at tick 0, and one `unit` before the right edge.
pub fn resized_left(origin: &Clip, delta: i64, unit: Ticks) -> Clip {
    let first_note = origin.notes.iter().map(|note| note.start.0).min();
    let room = origin.length.ticks().0 - shortest(origin.length, unit);
    let latest = first_note.map_or(room, |first| first.min(room));
    let delta = delta.clamp(-(origin.start.0 as i64), latest as i64);
    let mut clip = origin.clone();
    clip.start = shifted(origin.start, delta);
    clip.length = Length::at_least_one(shifted(origin.length.ticks(), -delta));
    for note in &mut clip.notes {
        note.start = shifted(note.start, -delta);
    }
    // The pedal keeps its place in the project too, as the notes do. The edge stops at the
    // first note, not at the first pedal move, so a move can fall outside the clip: it goes,
    // as a note that a resize drops goes, and comes back when the drag goes out again,
    // because every move starts from the origin.
    let length = clip.length.ticks().0 as i64;
    clip.pedal.retain_mut(|change| {
        let start = (change.start.0 as i64) - delta;
        let inside = (0..length).contains(&start);
        if inside {
            change.start = Ticks(start as u64);
        }
        inside
    });
    clip
}

/// The track row an arrow key moves a clip to. It stops at the first and at the last.
pub fn nudged_track(current: usize, tracks: usize, step: i64) -> usize {
    let last = tracks.saturating_sub(1);
    current.saturating_add_signed(step as isize).min(last)
}

#[cfg(test)]
mod tests {
    use sound_notes::{Note, Pedal, PedalChange, Pitch, Velocity};

    use super::*;

    const BAR: u64 = 3840;
    /// A sixteenth, the snap the window starts with.
    const UNIT: Ticks = Ticks(240);

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
            new_clip(Ticks(BAR + 479), four_four, UNIT),
            clip(BAR + 240, BAR, &[])
        );
        assert_eq!(new_clip(Ticks(0), four_four, UNIT), clip(0, BAR, &[]));
        let waltz = TimeSignature::new(3, 4).unwrap();
        assert_eq!(new_clip(Ticks(100), waltz, UNIT), clip(0, 2880, &[]));
    }

    #[test]
    fn the_right_edge_changes_the_length_and_going_back_restores_the_notes() {
        let origin = clip(BAR, BAR, &[(0, 480), (2880, 480)]);
        assert_eq!(resized_right(&origin, 0, UNIT), origin);
        assert_eq!(resized_right(&origin, 960, UNIT).length.ticks(), Ticks(BAR + 960));

        // In past the second note, then out again: every move starts from the origin.
        let shorter = resized_right(&origin, -1920, UNIT);
        assert_eq!(shorter, clip(BAR, 1920, &[(0, 480)]));
        assert_eq!(resized_right(&origin, -240, UNIT).notes.len(), 2);

        // Never shorter than one snap step, however far the pointer goes.
        assert_eq!(resized_right(&origin, -100_000, UNIT).length.ticks(), UNIT);
        // A clip that was shorter than a step keeps its length and can still grow.
        let tiny = clip(0, 100, &[]);
        assert_eq!(resized_right(&tiny, -240, UNIT), tiny);
        assert_eq!(resized_right(&tiny, 240, UNIT).length.ticks(), Ticks(340));
    }

    #[test]
    fn the_left_edge_keeps_notes_where_they_are_and_stops_at_the_first_note() {
        let origin = clip(BAR, BAR, &[(960, 480), (2880, 480)]);
        assert_eq!(resized_left(&origin, 0, UNIT), origin);

        // Out to the left: the clip grows and the note starts grow with it.
        let grown = resized_left(&origin, -480, UNIT);
        assert_eq!(
            grown,
            clip(BAR - 480, BAR + 480, &[(1440, 480), (3360, 480)])
        );
        // In to the right: the first note ends up at the start and the edge stops there.
        let at_note = clip(BAR + 960, BAR - 960, &[(0, 480), (1920, 480)]);
        assert_eq!(resized_left(&origin, 960, UNIT), at_note);
        assert_eq!(resized_left(&origin, 2400, UNIT), at_note);
        for delta in [-480, 480, 960, 100_000] {
            let resized = resized_left(&origin, delta, UNIT);
            assert_eq!(resized.end(), origin.end());
            let placed: Vec<_> = resized.placed_notes().collect();
            assert_eq!(placed, origin.placed_notes().collect::<Vec<_>>());
        }
    }

    /// The pedal keeps its place in the project when the left edge moves, as the notes do.
    /// Without this a recorded clip's pedal would slide against its own notes.
    #[test]
    fn the_left_edge_keeps_the_pedal_where_it_is_in_the_project() {
        let mut origin = clip(BAR, BAR, &[(960, 480)]);
        origin.pedal = vec![
            // Before the first note, which is where the edge stops.
            PedalChange {
                start: Ticks(480),
                value: Pedal::new(127).unwrap(),
            },
            PedalChange {
                start: Ticks(2880),
                value: Pedal::UP,
            },
        ];
        // Out to the left by a beat: the clip starts a beat earlier and the moves with it.
        let grown = resized_left(&origin, -960, UNIT);
        let placed = |clip: &Clip| -> Vec<(u64, u8)> {
            clip.placed_pedal()
                .map(|change| (change.start.0, change.value.value()))
                .collect()
        };
        assert_eq!(placed(&grown), placed(&origin));
        assert_eq!(
            grown
                .pedal
                .iter()
                .map(|change| change.start.0)
                .collect::<Vec<_>>(),
            [1440, 3840]
        );

        // In to the first note: the move that is then before the clip goes, as a note a
        // resize drops goes, and every move starts from the origin, so it comes back.
        let at_note = resized_left(&origin, 960, UNIT);
        assert_eq!(placed(&at_note), [(BAR + 2880, 0)]);
        assert_eq!(placed(&resized_left(&origin, 0, UNIT)), placed(&origin));
        assert_eq!(resized_left(&origin, 0, UNIT), origin);
    }

    #[test]
    fn the_left_edge_stops_at_tick_zero_and_before_the_right_edge() {
        let origin = clip(480, BAR, &[]);
        assert_eq!(resized_left(&origin, -960, UNIT), clip(0, BAR + 480, &[]));
        assert_eq!(
            resized_left(&origin, 100_000, UNIT),
            clip(480 + BAR - 240, 240, &[])
        );
        let at_zero = clip(0, BAR, &[(0, 480)]);
        assert_eq!(resized_left(&at_zero, -240, UNIT), at_zero);
        assert_eq!(resized_left(&at_zero, 240, UNIT), at_zero);
    }

    #[test]
    fn arrow_keys_stop_at_the_first_and_the_last_track() {
        assert_eq!(nudged_track(0, 3, -1), 0);
        assert_eq!(nudged_track(0, 3, 1), 1);
        assert_eq!(nudged_track(2, 3, 1), 2);
        assert_eq!(nudged_track(0, 1, 1), 0);
    }
}
