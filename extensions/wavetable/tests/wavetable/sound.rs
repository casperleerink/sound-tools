//! Level, pitch, the tables and what folds back, with one plain oscillator.

use wavetable::state::{Effect, Filter, Oscillator};
use wavetable::{Destination, Route, Source, Table, WavetableState};

use crate::support::{
    Harness, Track, frames, inharmonic_db, largest_step, loudest_hz, note, peak, ticks,
};

/// One oscillator straight out: no second oscillator, no filter, no routes.
pub fn plain(osc_1: Oscillator) -> WavetableState {
    WavetableState {
        osc_1,
        osc_2: Oscillator {
            on: false,
            ..Oscillator::default()
        },
        filter_1: Filter {
            on: false,
            ..Filter::default()
        },
        matrix: vec![],
        ..WavetableState::default()
    }
}

/// Half a second of a held note, once its attack and decay are over.
pub fn steady(harness: &mut Harness) -> Vec<f32> {
    let left = harness.play_left(frames(1.0));
    left[frames(0.5)..frames(0.5) + 16_384].to_vec()
}

fn hz(pitch: f32) -> f32 {
    440.0 * ((pitch - 69.0) / 12.0).exp2()
}

#[test]
fn one_note_of_the_default_patch_peaks_at_about_its_gain() {
    let state = WavetableState::default();
    let mut harness = Harness::with_notes(vec![note(0, ticks(1.0), 60, 127)], state.clone());
    let [left, right] = harness.play(frames(1.5));
    let peak = peak(&left);
    assert!((0.1..0.2).contains(&peak), "{peak}");
    assert!((peak - state.gain).abs() < 0.03, "{peak}");
    // One voice with no unison and no pan is in the middle.
    assert_eq!(left, right);
}

#[test]
fn a_note_plays_at_its_pitch_and_the_tuning_of_the_oscillator_moves_it() {
    let cases = [
        (0, 0, 0.0, 69.0),
        (1, 0, 0.0, 81.0),
        (-2, 0, 0.0, 45.0),
        (0, 7, 0.0, 76.0),
        (0, -12, 0.0, 57.0),
        (0, 0, 50.0, 69.5),
        (1, -5, -25.0, 75.75),
    ];
    for (octave, semitone, detune_cents, expected) in cases {
        let osc = Oscillator {
            octave,
            semitone,
            detune_cents,
            ..Oscillator::default()
        };
        let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 69, 127)], plain(osc));
        let played = loudest_hz(&steady(&mut harness), 50.0, 4_000.0);
        let expected = hz(expected);
        assert!(
            (played / expected - 1.0).abs() < 0.001,
            "{octave} {semitone} {detune_cents}: {played} for {expected}"
        );
    }
}

#[test]
fn the_bend_wheel_moves_a_note_two_semitones_down_and_up() {
    for (bend, expected) in [(-8192, 67.0), (8191, 71.0)] {
        let track = Track {
            notes: vec![note(0, ticks(2.0), 69, 127)],
            bend: vec![(0, bend)],
            ..Track::default()
        };
        let mut harness = Harness::with(track, plain(Oscillator::default()));
        let played = loudest_hz(&steady(&mut harness), 50.0, 4_000.0);
        assert!(
            (played / hz(expected) - 1.0).abs() < 0.001,
            "{bend}: {played}"
        );
    }
}

/// The top note of a piano, bent up as far as the wheel goes, with a saw, the brightest frame
/// of Basic Shapes. What folds back stays 70 dB under the harmonics.
#[test]
fn a_saw_at_the_top_note_bent_up_does_not_alias() {
    for pitch in [96, 108] {
        let track = Track {
            notes: vec![note(0, ticks(2.0), pitch, 127)],
            bend: vec![(0, 8191)],
            ..Track::default()
        };
        let mut harness = Harness::with(track, plain(Oscillator::default()));
        let steady = steady(&mut harness);
        let played = hz(f32::from(pitch) + 2.0);
        let folded = inharmonic_db(&steady, played, 30.0);
        assert!(folded < -70.0, "{pitch}: {folded} dB");
    }
}

/// Every table plays at every position: the sound stays finite and under full scale, and a
/// sweep of its position with an LFO is never steeper than the table is at the positions it
/// passes. A morph is a mix of two frames, so it cannot be steeper than both.
#[test]
fn every_table_plays_and_its_position_sweeps_without_a_step() {
    let render = |state: WavetableState| {
        let mut harness = Harness::with_notes(vec![note(0, ticks(1.0), 48, 127)], state);
        harness.play_left(frames(1.0))
    };
    for table in Table::ALL {
        let at = |position| Oscillator {
            table,
            position,
            ..Oscillator::default()
        };
        let steepest = [0.0, 0.25, 0.5, 0.75, 1.0]
            .map(|position| largest_step(&render(plain(at(position)))))
            .into_iter()
            .fold(0.0, f32::max);
        let sweep = WavetableState {
            matrix: vec![Route {
                source: Source::Lfo1,
                destination: Destination::Osc1Position,
                amount: 1.0,
            }],
            ..plain(at(0.0))
        };
        let swept = render(sweep);
        assert!(swept.iter().all(|sample| sample.is_finite()), "{table:?}");
        assert!(peak(&swept) < 0.2, "{table:?}: {}", peak(&swept));
        assert!(peak(&swept) > 0.01, "{table:?}: {}", peak(&swept));
        let step = largest_step(&swept);
        assert!(
            step < 1.2 * steepest,
            "{table:?}: {step} ({steepest} still)"
        );
    }
}

/// Up to C6 and at full amount, what every effect folds back stays 50 dB under its
/// harmonics in the audible band. Every effect runs at four times the sample rate, and
/// sync rounds off its jump.
#[test]
fn no_effect_aliases_up_to_c6() {
    for effect in Effect::ALL {
        for pitch in [60, 84] {
            for effect_amount in [0.3, 1.0] {
                let osc = Oscillator {
                    effect,
                    effect_amount,
                    ..Oscillator::default()
                };
                let mut harness =
                    Harness::with_notes(vec![note(0, ticks(2.0), pitch, 127)], plain(osc));
                let folded = inharmonic_db(&steady(&mut harness), hz(f32::from(pitch)), 30.0);
                assert!(
                    folded < -50.0,
                    "{effect:?} {effect_amount} at {pitch}: {folded} dB"
                );
            }
        }
    }
}

/// Unison, a sample and hold LFO, the random value of each note, sync and drive: everything
/// that could differ from one render to the next, and none of it does.
#[test]
fn the_same_project_renders_the_same_bytes_twice() {
    use sound_core::LfoShape;
    use wavetable::state::{LfoSettings, Unison};

    let state = WavetableState {
        osc_2: Oscillator {
            effect: Effect::Sync,
            ..Oscillator::default()
        },
        unison: Unison {
            voices: 5,
            amount: 0.6,
        },
        lfo_1: LfoSettings {
            shape: LfoShape::SampleAndHold,
            rate_hz: 7.0,
            ..LfoSettings::default()
        },
        filter_1: Filter {
            drive_db: 12.0,
            ..Filter::default()
        },
        matrix: vec![
            Route {
                source: Source::Lfo1,
                destination: Destination::Filter1Cutoff,
                amount: 0.3,
            },
            Route {
                source: Source::Random,
                destination: Destination::Osc2Position,
                amount: 0.5,
            },
        ],
        ..WavetableState::default()
    };
    let notes: Vec<_> = (0..8)
        .map(|index| note(ticks(0.2) * index, ticks(0.5), 48 + 5 * index as u8, 100))
        .collect();
    let render = || {
        let mut harness = Harness::with_notes(notes.clone(), state.clone());
        let [left, right] = harness.play(frames(3.0));
        let bits = |samples: Vec<f32>| samples.into_iter().map(f32::to_bits).collect::<Vec<_>>();
        (bits(left), bits(right))
    };
    let first = render();
    assert!(first.0.iter().any(|bits| *bits != 0));
    assert_eq!(first, render());
}
