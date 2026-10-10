//! The transient shaper, with the gate left open (`range_db` 0): it moves the start of a hit
//! and its tail, and leaves a steady sound as it is.

use gate::GateState;

use crate::support::{Rig, SAMPLE_RATE, decibels, dry, hits, peak, sine};

const SECOND: usize = SAMPLE_RATE as usize;

fn shaper(transient_db: f32, sustain_db: f32) -> GateState {
    GateState {
        range_db: 0.0,
        transient_db,
        sustain_db,
        ..GateState::default()
    }
}

/// The gain on the part of the second hit (it starts at 500 ms) from `from_ms` to `to_ms` after
/// its start, in dB against the dry hit.
fn hit_gain_db(state: GateState, from_ms: usize, to_ms: usize) -> f32 {
    let period = SECOND / 2;
    let output = Rig::new(state, hits(period)).render(SECOND);
    let input = dry(hits(period), SECOND);
    let window = period + from_ms * SECOND / 1_000..period + to_ms * SECOND / 1_000;
    20.0 * (peak(&output[window.clone()]) / peak(&input[window])).log10()
}

/// A transient boost raises the peak of the first cycle of a hit, and a cut lowers it.
#[test]
fn transient_raises_or_lowers_the_start_of_each_hit() {
    let boosted = hit_gain_db(shaper(12.0, 0.0), 0, 5);
    let cut = hit_gain_db(shaper(-12.0, 0.0), 0, 5);
    assert!(boosted > 10.0, "{boosted}");
    assert!(cut < -9.0, "{cut}");
    // Over by the time the tail plays.
    let tail = hit_gain_db(shaper(12.0, 0.0), 200, 250);
    assert!(tail.abs() < 0.1, "{tail}");
}

/// A sustain change moves the tail of a hit by about as many dB, and not its start.
#[test]
fn sustain_raises_or_lowers_the_tail_of_each_hit() {
    for sustain_db in [-12.0, 12.0] {
        let tail = hit_gain_db(shaper(0.0, sustain_db), 200, 250);
        assert!((tail - sustain_db).abs() < 1.0, "{sustain_db}: {tail}");
        let start = hit_gain_db(shaper(0.0, sustain_db), 0, 5);
        assert!(start.abs() < 0.1, "{sustain_db}: {start}");
    }
}

/// A steady tone has no start and no tail: with both gains at their ends it comes out sample
/// for sample as it went in. At 1 kHz a sample falls on each peak, so its level does not move
/// at all; at other frequencies it moves by the ripple of the largest sample, under 0.03 dB.
#[test]
fn a_steady_sound_passes_the_shaper_untouched() {
    let amplitude = decibels(-12.0);
    let output = Rig::new(shaper(18.0, -18.0), sine(1_000.0, amplitude)).render(SECOND);
    let input = dry(sine(1_000.0, amplitude), SECOND);
    assert_eq!(output[SECOND / 2..], input[SECOND / 2..]);
}
