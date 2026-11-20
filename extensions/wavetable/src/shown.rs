//! What its feeds and facts show: the frames of its tables and the response of its filters.

use schemars::JsonSchema;
use serde::Serialize;
use sound_core::feeds::{AUDIBLE_OCTAVES, FeedContext, db, hz_at};
use sound_dsp::svf_response;

use crate::state::{Filter, Oscillator};
use crate::{FRAME_LENGTH, Table, Wavetable, wavetable};

/// Samples of a frame a feed gives unless the binding asks for another number.
pub(crate) const POINTS: usize = 96;
/// The most frames [`frames`] gives: a table of more shows as many of them, evenly apart.
const SHOWN_FRAMES: usize = 12;

/// `points` samples of one cycle of `sample`, a level at each sample of a frame.
fn cycle(points: usize, sample: impl Fn(usize) -> f32) -> impl Iterator<Item = f32> {
    let points = points.max(2);
    (0..points).map(move |point| sample(point * FRAME_LENGTH / points))
}

/// See the feed `frames`.
pub(crate) fn frames(table: Table, points: usize) -> Vec<f32> {
    let Ok(table) = wavetable(table) else {
        return Vec::new();
    };
    let count = table.frames();
    let shown = count.min(SHOWN_FRAMES);
    let last = count.saturating_sub(1);
    (0..shown)
        .flat_map(|place| {
            let index = match shown {
                1 => 0,
                _ => (place * last + (shown - 1) / 2) / (shown - 1),
            };
            let frame = table.frame(index);
            cycle(points, move |sample| frame[sample]).collect::<Vec<_>>()
        })
        .collect()
}

/// See the feed `frame`.
pub(crate) fn playing(oscillator: &Oscillator, points: usize) -> Vec<f32> {
    let Ok(table) = wavetable(oscillator.table) else {
        return Vec::new();
    };
    mixed(&table, oscillator.position, points)
}

/// The frame at `position`, 0 to 1: the mix of the two frames it is between, as a voice plays
/// it.
fn mixed(table: &Wavetable, position: f32, points: usize) -> Vec<f32> {
    let at = position.clamp(0.0, 1.0) * table.frames().saturating_sub(1) as f32;
    let (first, mix) = (at.floor() as usize, at.fract());
    let (from, to) = (table.frame(first), table.frame(first + 1));
    cycle(points, |sample| {
        from[sample] + (to[sample] - from[sample]) * mix
    })
    .collect()
}

/// See the feed `response_1`.
pub(crate) fn response(filter: &Filter, context: &FeedContext<'_>) -> Vec<f32> {
    let rate = context.sample_rate as f32;
    context.curve(AUDIBLE_OCTAVES, 157, |octaves| match filter.on {
        false => 0.0,
        true => {
            let (real, imaginary) = svf_response(
                filter.kind,
                filter.slope,
                filter.cutoff_hz,
                filter.resonance,
                hz_at(octaves),
                rate,
            );
            db(real.hypot(imaginary) as f32)
        }
    })
}

/// What its card knows that is no record: the tables a picker offers.
#[derive(Serialize, JsonSchema)]
pub(crate) struct Facts {
    tables: Vec<TableFact>,
}

#[derive(Serialize, JsonSchema)]
struct TableFact {
    value: Table,
    name: &'static str,
    /// The group a picker sorts it into.
    category: &'static str,
}

impl Facts {
    pub(crate) fn new() -> Self {
        let tables = (Table::ALL.into_iter())
            .map(|value| TableFact {
                value,
                name: value.name(),
                category: value.category().name(),
            })
            .collect();
        Self { tables }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame at a position of a frame is that frame, and halfway between two it is their
    /// mix: what a voice plays there.
    #[test]
    fn the_playing_frame_is_the_frame_at_the_position() {
        let table = wavetable(Table::BasicShapes).unwrap();
        let last = (table.frames() - 1) as f32;
        let sampled =
            |index| cycle(POINTS, |sample| table.frame(index)[sample]).collect::<Vec<_>>();
        assert_eq!(mixed(&table, 0.0, POINTS), sampled(0));
        assert_eq!(mixed(&table, 1.0 / last, POINTS), sampled(1));
        let between = mixed(&table, 0.5 / last, POINTS);
        let (first, second) = (sampled(0), sampled(1));
        for ((between, first), second) in between.iter().zip(&first).zip(&second) {
            assert!((between - (first + second) / 2.0).abs() < 1e-5);
        }
    }

    #[test]
    fn a_table_shows_at_most_twelve_frames_from_its_first_to_its_last() {
        for table in Table::ALL {
            let built = wavetable(table).unwrap();
            let frames = frames(table, POINTS);
            let shown = built.frames().min(SHOWN_FRAMES);
            assert_eq!(frames.len(), shown * POINTS, "{table:?}");
            let last = cycle(POINTS, |sample| built.frame(built.frames() - 1)[sample]);
            assert!(
                frames[(shown - 1) * POINTS..].iter().copied().eq(last),
                "{table:?}"
            );
        }
    }
}
