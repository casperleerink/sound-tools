//! The effects of an oscillator: each bends the sound, and a change of effect or table fades
//! instead of clicking.

use wavetable::Table;
use wavetable::state::{Effect, Oscillator};

use crate::sound::{plain, steady};
use crate::support::{Harness, frames, largest_step, level_at, note, peak, ticks};

/// The level of the first 16 harmonics of middle C.
fn harmonics(samples: &[f32]) -> Vec<f32> {
    let hz = 261.625_58;
    (1..=16)
        .map(|harmonic| level_at(samples, hz * harmonic as f32))
        .collect()
}

/// On a sine, the first frame of Basic Shapes. FM bends the phase with a sine of the same
/// pitch, and a saw is a straight ramp of the phase: on a saw, FM mostly adds that sine.
#[test]
fn every_effect_changes_the_harmonics_and_stays_bounded() {
    let render = |effect, effect_amount| {
        let osc = Oscillator {
            effect,
            effect_amount,
            position: 0.0,
            ..Oscillator::default()
        };
        let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 60, 127)], plain(osc));
        steady(&mut harness)
    };
    let sine = render(Effect::None, 0.6);
    let plain_harmonics = harmonics(&sine);
    for effect in [Effect::Fm, Effect::Sync, Effect::Warp, Effect::Fold] {
        for amount in [0.2, 0.6, 1.0] {
            let bent = render(effect, amount);
            assert!(bent.iter().all(|sample| sample.is_finite()));
            // An oscillator at gain 0.7 and sustain 0.6, times the gain of 0.15: 0.063 for a
            // wave that peaks at 1. A jump, as sync makes, rings a little past it on its way
            // down from four times the rate.
            assert!(peak(&bent) < 0.075, "{effect:?} {amount}: {}", peak(&bent));
            let moved = harmonics(&bent)
                .iter()
                .zip(&plain_harmonics)
                .map(|(bent, plain)| (20.0 * (bent.max(1e-6) / plain.max(1e-6)).log10()).abs())
                .fold(0.0, f32::max);
            assert!(moved > 3.0, "{effect:?} {amount}: {moved} dB");
        }
    }
}

/// A sine, whose largest step from one sample to the next is small, so a click would stand
/// out. The switches run at four times the rate after the change, and come out later than the
/// plain oscillator: without the fade that would be a jump.
#[test]
fn a_new_effect_or_table_fades_the_oscillator_out_and_in() {
    let sine = Oscillator {
        position: 0.0,
        effect_amount: 0.0,
        ..Oscillator::default()
    };
    let changes = [
        Oscillator {
            effect: Effect::Sync,
            ..sine
        },
        Oscillator {
            effect: Effect::Fold,
            ..sine
        },
        Oscillator {
            table: Table::Harmonics,
            ..sine
        },
    ];
    for change in changes {
        let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 60, 127)], plain(sine));
        let before = harness.play_left(frames(0.6));
        let steady = largest_step(&before[frames(0.5)..]);
        harness.edit(plain(change));
        let after = harness.render(frames(0.2))[0].clone();
        let step = largest_step(&after);
        assert!(step < 1.5 * steady, "{change:?}: {step} against {steady}");
        // It came back.
        let level = peak(&after[frames(0.1)..]);
        assert!(
            level > 0.9 * peak(&before[frames(0.5)..]),
            "{change:?}: {level}"
        );
    }
}
