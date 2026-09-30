//! The modulation matrix: each source moves its destination the way the docs say, every
//! destination can be moved, the random value of a note is the same on every render, and a
//! synced LFO follows the tempo.

use sound_core::LfoShape;
use sound_notes::{Division, Feel};
use wavetable::state::{Adsr, Effect, Filter, LfoSettings, Oscillator, Unison};
use wavetable::{Destination, Route, Source, WavetableState};

use crate::sound::plain;
use crate::support::{
    Harness, Track, brightness_db, frames, level_at, loudest_hz, note, peak, rms, ticks,
};

fn route(source: Source, destination: Destination, amount: f32) -> Route {
    Route {
        source,
        destination,
        amount,
    }
}

fn with_routes(state: WavetableState, routes: &[Route]) -> WavetableState {
    WavetableState {
        matrix: routes.to_vec(),
        ..state
    }
}

/// A saw through a low pass at 1 kHz, the oscillator alone.
fn filtered() -> WavetableState {
    WavetableState {
        filter_1: Filter {
            cutoff_hz: 1_000.0,
            resonance: 0.0,
            ..Filter::default()
        },
        ..plain(Oscillator::default())
    }
}

const C4: f32 = 261.625_58;

#[test]
fn env_2_opens_the_filter_and_it_closes_again_as_the_envelope_falls() {
    let state = WavetableState {
        env_2: Adsr {
            decay_seconds: 0.3,
            sustain: 0.0,
            ..Adsr::default()
        },
        ..filtered()
    };
    let bright = |state: WavetableState, from: f32| {
        let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 60, 127)], state);
        let left = harness.play_left(frames(1.0));
        brightness_db(&left[frames(from)..frames(from) + 2_048], 2_000.0)
    };
    let routed = with_routes(
        state.clone(),
        &[route(Source::Env2, Destination::Filter1Cutoff, 0.5)],
    );
    let (early, late) = (bright(routed.clone(), 0.02), bright(routed, 0.8));
    let (plain_early, plain_late) = (bright(state.clone(), 0.02), bright(state, 0.8));
    assert!(early > late + 10.0, "{early} {late}");
    assert!(
        (plain_early - plain_late).abs() < 2.0,
        "{plain_early} {plain_late}"
    );
    assert!((late - plain_late).abs() < 1.0, "{late} {plain_late}");
}

#[test]
fn an_lfo_on_the_pitch_moves_it_up_and_down_by_its_amount() {
    let state = WavetableState {
        lfo_1: LfoSettings {
            rate_hz: 1.0,
            ..LfoSettings::default()
        },
        ..plain(Oscillator::default())
    };
    // Half a semitone either way: 1/48 of the 24 semitones of amount 1.
    let state = with_routes(
        state,
        &[route(Source::Lfo1, Destination::Osc1Pitch, 1.0 / 48.0)],
    );
    let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), 60, 127)], state);
    let left = harness.play_left(frames(1.0));
    // The sine LFO is at its top a quarter of a second in, and at its bottom at three quarters.
    let at = |seconds: f32| {
        let window = &left[frames(seconds) - 2_048..frames(seconds) + 2_048];
        12.0 * (loudest_hz(window, 200.0, 350.0) / C4).log2()
    };
    let (top, bottom) = (at(0.25), at(0.75));
    assert!((top - 0.5).abs() < 0.05, "{top}");
    assert!((bottom + 0.5).abs() < 0.05, "{bottom}");
}

#[test]
fn velocity_on_the_amp_level_at_amount_one_makes_a_note_as_loud_as_it_was_played() {
    let state = with_routes(
        plain(Oscillator::default()),
        &[route(Source::Velocity, Destination::AmpLevel, 1.0)],
    );
    let level = |velocity| {
        let notes = vec![note(0, ticks(2.0), 60, velocity)];
        let mut harness = Harness::with_notes(notes, state.clone());
        rms(&harness.play_left(frames(1.0))[frames(0.5)..])
    };
    let ratio = level(64) / level(127);
    assert!((ratio - 64.0 / 127.0).abs() < 0.005, "{ratio}");
    // Turned the other way, the harder note is the quieter one.
    let inverted = with_routes(
        plain(Oscillator::default()),
        &[route(Source::Velocity, Destination::AmpLevel, -1.0)],
    );
    let mut soft = Harness::with_notes(vec![note(0, ticks(2.0), 60, 32)], inverted.clone());
    let mut hard = Harness::with_notes(vec![note(0, ticks(2.0), 60, 127)], inverted);
    assert!(peak(&soft.play_left(frames(1.0))) > 10.0 * peak(&hard.play_left(frames(1.0))));
}

/// Key at amount 1 makes the cutoff follow the keys one octave per octave: an octave up, every
/// harmonic comes through the filter as it did an octave down.
#[test]
fn key_on_the_cutoff_at_amount_one_tracks_the_keys() {
    let tracking = with_routes(
        filtered(),
        &[route(Source::Key, Destination::Filter1Cutoff, 1.0)],
    );
    let harmonics = |state: WavetableState, pitch: u8| -> Vec<f32> {
        let mut harness = Harness::with_notes(vec![note(0, ticks(2.0), pitch, 127)], state);
        let left = harness.play_left(frames(1.0));
        let steady = &left[frames(0.5)..];
        let hz = 440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2();
        let fundamental = level_at(steady, hz);
        (2..12)
            .map(|harmonic| 20.0 * (level_at(steady, hz * harmonic as f32) / fundamental).log10())
            .collect()
    };
    let middle = harmonics(tracking.clone(), 60);
    let up = harmonics(tracking, 72);
    let untracked_up = harmonics(filtered(), 72);
    // Deep in the slope the filter bends a little differently an octave up, closer to half
    // the sample rate.
    for (middle, up) in middle.iter().zip(&up) {
        assert!(
            (middle - up).abs() < 0.2 + 0.03 * middle.abs(),
            "{middle} {up}"
        );
    }
    // Without the route, the same filter takes more of the harmonics of the higher note.
    let sum = |levels: &[f32]| levels.iter().sum::<f32>();
    assert!(sum(&untracked_up) < sum(&up) - 10.0);
}

#[test]
fn the_mod_wheel_on_the_position_morphs_the_saw_into_a_square() {
    let state = with_routes(
        plain(Oscillator::default()),
        &[route(Source::ModWheel, Destination::Osc1Position, 0.5)],
    );
    let second_harmonic = |wheel: u8| {
        let track = Track {
            notes: vec![note(0, ticks(2.0), 60, 127)],
            mod_wheel: vec![(0, wheel)],
            ..Track::default()
        };
        let mut harness = Harness::with(track, state.clone());
        let steady = &harness.play_left(frames(1.0))[frames(0.5)..];
        level_at(steady, 2.0 * C4) / level_at(steady, C4)
    };
    // A saw has half its fundamental at the second harmonic; a square has none.
    assert!((second_harmonic(0) - 0.5).abs() < 0.02);
    assert!(second_harmonic(127) < 0.01);
}

#[test]
fn pressure_on_the_sub_gain_brings_the_sub_in() {
    let state = with_routes(
        plain(Oscillator::default()),
        &[route(Source::Pressure, Destination::SubGain, 1.0)],
    );
    let sub = |pressure: u8| {
        let track = Track {
            notes: vec![note(0, ticks(2.0), 60, 127)],
            pressure: vec![(0, pressure)],
            ..Track::default()
        };
        let mut harness = Harness::with(track, state.clone());
        let steady = &harness.play_left(frames(1.0))[frames(0.5)..];
        level_at(steady, C4 / 2.0)
    };
    assert!(sub(0) < 1e-4, "{}", sub(0));
    assert!(sub(127) > 0.05, "{}", sub(127));
}

/// Random on the pan: each note sits somewhere else, and the same note sits in the same place
/// on every render.
#[test]
fn the_random_value_of_a_note_is_the_same_on_every_render() {
    let state = with_routes(
        plain(Oscillator::default()),
        &[route(Source::Random, Destination::Pan, 1.0)],
    );
    let notes = (0..4)
        .map(|index| note(ticks(0.5) * index, ticks(0.4), 60, 127))
        .collect::<Vec<_>>();
    let render = || {
        let mut harness = Harness::with_notes(notes.clone(), state.clone());
        harness.play(frames(2.0))
    };
    let first = render();
    assert_eq!(first, render());
    let [left, right] = &first;
    let balances: Vec<f32> = (0..4)
        .map(|index| {
            let window = frames(0.5 * index as f32 + 0.1)..frames(0.5 * index as f32 + 0.3);
            rms(&left[window.clone()]) / rms(&right[window])
        })
        .collect();
    for pair in balances.windows(2) {
        assert!((pair[0] - pair[1]).abs() > 0.01, "{balances:?}");
    }
}

/// Every destination moves the sound: the mod wheel at full on a route to it at half amount
/// changes the render. A route to the amp level turns a voice down where its source is low, so
/// that one is turned the other way round.
#[test]
fn every_destination_moves_the_sound() {
    let base = WavetableState {
        osc_1: Oscillator {
            effect: Effect::Fm,
            ..Oscillator::default()
        },
        osc_2: Oscillator {
            effect: Effect::Warp,
            octave: 1,
            ..Oscillator::default()
        },
        sub: wavetable::state::Sub {
            gain: 0.3,
            ..wavetable::state::Sub::default()
        },
        unison: Unison {
            voices: 3,
            amount: 0.3,
        },
        filter_2: Filter {
            cutoff_hz: 3_000.0,
            ..Filter::default()
        },
        matrix: vec![
            route(Source::Lfo1, Destination::Osc1Position, 0.3),
            route(Source::Lfo2, Destination::Osc2Position, 0.3),
        ],
        ..WavetableState::default()
    };
    let render = |state: WavetableState| {
        let track = Track {
            notes: vec![note(0, ticks(1.0), 60, 127)],
            mod_wheel: vec![(0, 127)],
            ..Track::default()
        };
        let mut harness = Harness::with(track, state);
        harness.play(frames(0.8))
    };
    let unmoved = render(base.clone());
    for destination in Destination::ALL {
        let mut state = base.clone();
        let amount = if destination == Destination::AmpLevel {
            -0.5
        } else {
            0.5
        };
        state
            .matrix
            .push(route(Source::ModWheel, destination, amount));
        let moved = render(state);
        let difference = moved
            .iter()
            .zip(&unmoved)
            .flat_map(|(moved, unmoved)| moved.iter().zip(unmoved))
            .fold(0.0_f32, |largest, (a, b)| largest.max((a - b).abs()));
        assert!(difference > 0.005, "{destination:?}: {difference}");
    }
}

/// An LFO on the amp level at amount 1 is a full tremolo: its quietest moments are its
/// cycles. The frames of the quietest moment in each of `cycles` stretches of `seconds`.
fn quietest(left: &[f32], seconds: f32, cycles: usize) -> Vec<f32> {
    let window = frames(0.005);
    (0..cycles)
        .map(|cycle| {
            let start = frames(seconds * cycle as f32);
            let levels = (0..frames(seconds) / window).map(|index| {
                let from = start + index * window;
                (index, rms(&left[from..from + window]))
            });
            let (index, _) = levels.min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            (start + index * window + window / 2) as f32 / 48_000.0
        })
        .collect()
}

#[test]
fn a_synced_lfo_follows_the_tempo_and_a_free_one_the_beat() {
    let tremolo = |retrigger| {
        let state = WavetableState {
            lfo_1: LfoSettings {
                shape: LfoShape::Sine,
                sync: true,
                division: Division::Quarter,
                feel: Feel::Straight,
                retrigger,
                ..LfoSettings::default()
            },
            ..plain(Oscillator::default())
        };
        with_routes(state, &[route(Source::Lfo1, Destination::AmpLevel, 1.0)])
    };
    for (bpm, beat) in [(120.0, 0.5), (90.0, 2.0 / 3.0)] {
        let mut harness = Harness::with_notes(vec![note(0, 4 * 960, 60, 127)], tremolo(true));
        harness.set_tempo(bpm);
        let left = harness.play_left(frames(2.0));
        // A sine that starts with the note is at its bottom three quarters into each cycle.
        let bottoms = quietest(&left, beat, 3);
        for (cycle, bottom) in bottoms.iter().enumerate() {
            let expected = (cycle as f32 + 0.75) * beat;
            assert!((bottom - expected).abs() < 0.01, "{bpm}: {bottoms:?}");
        }
    }
    // Free-running, a note that starts off the beat picks the LFO up where the beat has it.
    let mut harness = Harness::with_notes(vec![note(300, 4 * 960, 60, 127)], tremolo(false));
    let left = harness.play_left(frames(2.0));
    let bottoms = quietest(&left, 0.5, 4);
    for bottom in &bottoms[1..] {
        let beats = bottom / 0.5;
        assert!((beats - beats.floor() - 0.75).abs() < 0.03, "{bottoms:?}");
    }
}
