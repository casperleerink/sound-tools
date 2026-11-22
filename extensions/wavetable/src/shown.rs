//! What its feeds and facts show: the frames of its tables and the response of its filters.

use schemars::JsonSchema;
use serde::Serialize;
use sound_core::feeds::{AUDIBLE_OCTAVES, FeedContext, db, hz_at};
use sound_dsp::svf_response;

use sound_dsp::{EnvelopeCurves, Lfo, LfoShape};

use crate::state::{Adsr, ENV_ATTACK, ENV_DECAY, ENV_RELEASE, Filter, Oscillator};
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

/// Where the drawing of a table puts its frames, 0 to 1 across and up: the first starts at
/// `LEFT`, each is `FRAME_WIDTH` wide, and the last sits `DEPTH_ACROSS` right of the first; the
/// middle line of the first is at `FRONT` and of the last `DEPTH_UP` above it; a frame at full
/// level swings `SWING` up and down. A card's drag up the drawing moves the position by these.
const LEFT: f32 = 0.04;
const FRAME_WIDTH: f32 = 0.62;
const DEPTH_ACROSS: f32 = 0.3;
const FRONT: f32 = 0.2;
const DEPTH_UP: f32 = 0.4;
const SWING: f32 = 0.12;
/// How thick a line of the drawing is, up the drawing.
const THICK: f32 = 0.008;

/// The frame at `depth`, 0 at the front to 1 at the back, as `rects`: each step from one sample
/// to the next a thin rect that spans both, so they join into a line. `points` across the drawing.
fn drawn(depth: f32, frame: &[f32], into: &mut Vec<f32>) {
    let (left, middle) = (LEFT + DEPTH_ACROSS * depth, FRONT + DEPTH_UP * depth);
    let step = FRAME_WIDTH / frame.len().saturating_sub(1).max(1) as f32;
    for (index, pair) in frame.windows(2).enumerate() {
        let [from, to] = [pair[0], pair[1]].map(|level| middle + SWING * level);
        let x = left + step * index as f32;
        into.extend([
            x,
            from.min(to) - THICK / 2.0,
            step,
            (to - from).abs() + THICK,
        ]);
    }
}

/// The samples of a frame across the drawing of `points`.
fn samples(points: usize) -> usize {
    (points as f32 * FRAME_WIDTH).round().max(2.0) as usize
}

/// See the feed `frames`.
pub(crate) fn frames(table: Table, points: usize) -> Vec<f32> {
    let Ok(table) = wavetable(table) else {
        return Vec::new();
    };
    let count = table.frames();
    let shown = count.min(SHOWN_FRAMES);
    let last = count.saturating_sub(1);
    let mut rects = Vec::new();
    for place in 0..shown {
        let index = match shown {
            1 => 0,
            _ => (place * last + (shown - 1) / 2) / (shown - 1),
        };
        let frame = table.frame(index);
        let line: Vec<f32> = cycle(samples(points), |sample| frame[sample]).collect();
        drawn(index as f32 / last.max(1) as f32, &line, &mut rects);
    }
    rects
}

/// See the feed `frame`.
pub(crate) fn playing(oscillator: &Oscillator, points: usize) -> Vec<f32> {
    let Ok(table) = wavetable(oscillator.table) else {
        return Vec::new();
    };
    let mut rects = Vec::new();
    let position = oscillator.position.clamp(0.0, 1.0);
    drawn(
        position,
        &mixed(&table, position, samples(points)),
        &mut rects,
    );
    rects
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

/// How long a held note is drawn at the sustain level, in the travel of a time knob.
pub(crate) const ENVELOPE_HOLD: f32 = 0.36;

/// See the feed `envelope_1`: the level of a held note at `x` across the drawing, where each
/// time takes the travel of its knob, so a handle at a corner moves as its knob turns. Each
/// stage bends by its curve, as the voices play it.
pub(crate) fn envelope_at(envelope: &Adsr, x: f32) -> f32 {
    let travel = |time: &sound_core::Parameter<Adsr>, seconds| {
        time.scale.position(time.min, time.max, seconds)
    };
    let peak = travel(&ENV_ATTACK, envelope.attack_seconds);
    let decayed = peak + travel(&ENV_DECAY, envelope.decay_seconds);
    let held = decayed + ENVELOPE_HOLD;
    let released = held + travel(&ENV_RELEASE, envelope.release_seconds);
    let part = |from: f32, to: f32| match to > from {
        true => (x - from) / (to - from),
        false => 1.0,
    };
    let sustain = envelope.sustain;
    match x {
        x if x < 0.0 => 0.0,
        x if x < peak => EnvelopeCurves::progress(envelope.attack_curve, part(0.0, peak)),
        x if x < decayed => {
            let gone = EnvelopeCurves::progress(envelope.decay_curve, part(peak, decayed));
            1.0 + (sustain - 1.0) * gone
        }
        x if x < held => sustain,
        x if x < released => {
            let gone = EnvelopeCurves::progress(envelope.release_curve, part(held, released));
            sustain * (1.0 - gone)
        }
        _ => 0.0,
    }
}

/// See the feed `lfo_1`: two cycles of `shape`, -1 to 1.
pub(crate) fn lfo_line(shape: LfoShape, context: &FeedContext<'_>) -> Vec<f32> {
    let lfo = Lfo::default();
    context.curve([0.0, 2.0], POINTS, |cycles| lfo.value(shape, -cycles))
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
    fn a_table_shows_at_most_twelve_frames_from_its_first_at_the_front_to_its_last_at_the_back() {
        let points = 100;
        let across = samples(points) - 1;
        for table in Table::ALL {
            let shown = wavetable(table).unwrap().frames().min(SHOWN_FRAMES);
            let rects = frames(table, points);
            assert_eq!(rects.len(), shown * across * 4, "{table:?}");
            let lefts = [rects[0], rects[(shown - 1) * across * 4]];
            assert_eq!(lefts, [LEFT, LEFT + DEPTH_ACROSS], "{table:?}");
        }
    }

    /// The curve of a stage bends it and nothing else: each corner stays where the knobs put it.
    #[test]
    fn an_envelope_reaches_each_corner_whatever_its_curves() {
        let straight = Adsr::default();
        let bent = Adsr {
            attack_curve: 1.0,
            decay_curve: 0.0,
            ..straight
        };
        let travel = |seconds| {
            ENV_ATTACK
                .scale
                .position(ENV_ATTACK.min, ENV_ATTACK.max, seconds)
        };
        let peak = travel(straight.attack_seconds);
        let decayed = peak + travel(straight.decay_seconds);
        for envelope in [straight, bent] {
            assert!((envelope_at(&envelope, peak - 1e-4) - 1.0).abs() < 1e-2);
            assert!((envelope_at(&envelope, decayed + 0.1) - envelope.sustain).abs() < 1e-6);
        }
        let middle = peak / 2.0;
        assert!(envelope_at(&bent, middle) > envelope_at(&straight, middle));
    }

    /// A drag up the drawing moves the position as far as the bright frame moves up it.
    #[test]
    fn the_playing_frame_sits_at_the_depth_of_its_position() {
        let oscillator = Oscillator {
            position: 0.5,
            ..Oscillator::default()
        };
        let rects = playing(&oscillator, 100);
        assert_eq!(rects[0], LEFT + DEPTH_ACROSS / 2.0);
        let (lowest, highest) = (rects.chunks_exact(4))
            .fold((f32::MAX, f32::MIN), |(low, high), rect| {
                (low.min(rect[1]), high.max(rect[1] + rect[3]))
            });
        let middle = FRONT + DEPTH_UP / 2.0;
        assert!(lowest >= middle - SWING - THICK && highest <= middle + SWING + THICK);
    }
}
