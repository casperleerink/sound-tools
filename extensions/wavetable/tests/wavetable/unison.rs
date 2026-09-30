//! Unison: more copies spread wider and are not much louder.

use wavetable::WavetableState;
use wavetable::state::{Oscillator, Unison};

use crate::sound::plain;
use crate::support::{Harness, frames, note, rms, ticks};

fn render(voices: u8, amount: f32) -> [Vec<f32>; 2] {
    let state = WavetableState {
        unison: Unison { voices, amount },
        ..plain(Oscillator::default())
    };
    let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 48, 127)], state);
    let [left, right] = harness.play(frames(1.5));
    [left[frames(0.5)..].to_vec(), right[frames(0.5)..].to_vec()]
}

/// How alike the two sides are, from 1 for the same sound to 0 for unrelated ones.
fn correlation(left: &[f32], right: &[f32]) -> f32 {
    let product: f32 = left
        .iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum();
    product / (rms(left) * rms(right) * left.len() as f32)
}

#[test]
fn more_copies_spread_wider_and_are_not_much_louder() {
    let [one_left, one_right] = render(1, 0.5);
    assert_eq!(one_left, one_right);
    let one = rms(&one_left);
    for voices in [2, 4, 8] {
        let [left, right] = render(voices, 0.5);
        assert!(correlation(&left, &right) < 0.9, "{voices}");
        let both = (rms(&left) + rms(&right)) / 2.0;
        let db = 20.0 * (both / one).log10();
        assert!(db.abs() < 3.0, "{voices}: {db} dB");
    }
    // At amount 0 the copies play the same pitch in the middle.
    let [left, right] = render(8, 0.0);
    assert_eq!(left, right);
}
