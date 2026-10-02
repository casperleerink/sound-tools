//! Our drawing of a wavetable: its frames one behind the other, each one cycle wide, the first
//! at the front and low on the left and the last at the back, higher and to the right. They
//! are quiet lines; the frame that plays at the position of the oscillator is the one bright
//! line, a mix of the two frames it is between, as the oscillator plays it. Places are from 0
//! to 1 on the display, `y` up.

use gpui::{Point, point};
use sound_ui::components::knob::KnobRange;

use crate::{FRAME_LENGTH, Wavetable};

/// Where the first frame starts, how wide a frame is, and how far right the last one sits
/// from the first.
const LEFT: f32 = 0.04;
const FRAME_WIDTH: f32 = 0.62;
const DEPTH_ACROSS: f32 = 0.3;
/// Where the middle line of the first frame is, and how far up the last one sits from it.
const FRONT: f32 = 0.2;
const DEPTH_UP: f32 = 0.4;
/// How far a frame at full level reaches up and down from its middle line.
const SWING: f32 = 0.12;
/// The most quiet lines: a table of more frames shows as many of them, evenly apart.
const SHOWN_FRAMES: usize = 12;
/// Points of one frame across.
const POINTS: usize = 96;

/// The middle line of the frame at a position, from the front at 0 to the back at 1: where
/// its line is, and where a drag up and down on the display moves the position.
pub(super) const POSITION_TRAVEL: KnobRange =
    KnobRange::linear(-FRONT / DEPTH_UP, (1. - FRONT) / DEPTH_UP);

/// A frame at a position as a line: `sample(index)` is its level at a sample of one cycle.
fn line(position: f32, sample: impl Fn(usize) -> f32) -> Vec<Point<f32>> {
    let left = LEFT + DEPTH_ACROSS * position;
    let middle = FRONT + DEPTH_UP * position;
    (0..=POINTS)
        .map(|step| {
            let part = step as f32 / POINTS as f32;
            let index = ((part * FRAME_LENGTH as f32) as usize).min(FRAME_LENGTH - 1);
            point(left + FRAME_WIDTH * part, middle + SWING * sample(index))
        })
        .collect()
}

/// The quiet lines of the frames, from the back to the front, so a front one draws over the
/// ones behind it.
pub(super) fn frames(table: &Wavetable) -> Vec<Vec<Point<f32>>> {
    let count = table.frames();
    let last = count.saturating_sub(1).max(1) as f32;
    let shown = count.min(SHOWN_FRAMES);
    (0..shown)
        .rev()
        .map(|place| {
            let index = match shown {
                1 => 0,
                _ => (place as f32 * last / (shown - 1) as f32).round() as usize,
            };
            let frame = table.frame(index);
            line(index as f32 / last, |sample| frame[sample])
        })
        .collect()
}

/// The frame that plays at `position`, from 0 to 1: the mix of the two frames it is between.
pub(super) fn playing(table: &Wavetable, position: f32) -> Vec<Point<f32>> {
    let position = position.clamp(0., 1.);
    let at = position * table.frames().saturating_sub(1) as f32;
    let (first, mix) = (at.floor() as usize, at.fract());
    let (from, to) = (table.frame(first), table.frame(first + 1));
    line(position, |sample| {
        from[sample] + (to[sample] - from[sample]) * mix
    })
}

/// The frame at `position`, counted from 1, and how many frames the table has: what the line
/// under the display says.
pub(super) fn frame_number(table: &Wavetable, position: f32) -> (usize, usize) {
    let count = table.frames();
    let at = position.clamp(0., 1.) * count.saturating_sub(1) as f32;
    (at.round() as usize + 1, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Table, wavetable};

    /// The bright line is where a drag on the display says the position is, and it is one of
    /// the quiet lines at the position of a frame.
    #[test]
    fn the_playing_frame_is_on_the_travel_of_the_position() {
        let table = wavetable(Table::BasicShapes).unwrap();
        for position in [0., 0.25, 0.5, 1.] {
            let playing = playing(&table, position);
            let middle = POSITION_TRAVEL.position(position);
            let lowest = playing.iter().map(|place| place.y).fold(1., f32::min);
            let highest = playing.iter().map(|place| place.y).fold(0., f32::max);
            assert!(
                ((lowest + highest) / 2. - middle).abs() < SWING,
                "{position}"
            );
        }
        let first = playing(&table, 0.);
        assert_eq!(frames(&table).last(), Some(&first));
    }

    /// Every line stays inside the display.
    #[test]
    fn every_frame_of_every_table_fits_the_display() {
        for table in Table::ALL {
            let table = wavetable(table).unwrap();
            let lines = frames(&table);
            assert!(lines.len() <= SHOWN_FRAMES);
            for place in lines.iter().flatten().chain(&playing(&table, 1.)) {
                assert!((0. ..=1.).contains(&place.x) && (0. ..=1.).contains(&place.y));
            }
        }
    }

    #[test]
    fn the_frame_number_counts_from_one() {
        let table = wavetable(Table::PulseWidth).unwrap();
        let count = table.frames();
        assert_eq!(frame_number(&table, 0.), (1, count));
        assert_eq!(frame_number(&table, 1.), (count, count));
    }
}
