//! Where the voice goes, how fast, and what is left alone.

use pitch_correction::{Key, PitchCorrection, PitchCorrectionState, Scale};
use sound_core::Processor;

use crate::support::{
    Rig, SAMPLE_RATE, between, chord, farthest_cents, heard, held, noise, onsets, voice,
};

const SECOND: usize = SAMPLE_RATE as usize;

fn retune(retune_ms: f32) -> PitchCorrectionState {
    PitchCorrectionState {
        retune_ms,
        ..PitchCorrectionState::default()
    }
}

fn latency() -> u32 {
    PitchCorrection::new(PitchCorrectionState::default(), Default::default()).latency()
}

#[test]
fn a_note_sung_off_reaches_its_note_in_about_the_retune_time() {
    // A3 30 cents sharp, from a quarter of a second on.
    let sharp = voice(|time| (time >= 0.25).then_some(57.3));
    let output = Rig::new(retune(100.0), sharp).render(SECOND);
    let heard = heard(&output, latency());
    let early = between(&heard, 0.27, 0.29);
    assert!(farthest_cents(&early, 57.0) > 10.0, "{early:?}");
    let late = between(&heard, 0.38, 0.95);
    assert!(farthest_cents(&late, 57.0) < 3.0, "{late:?}");
}

#[test]
fn a_retune_of_zero_takes_the_note_at_once() {
    let sharp = voice(|time| (time >= 0.25).then_some(57.3));
    let output = Rig::new(retune(0.0), sharp).render(SECOND);
    let notes = between(&heard(&output, latency()), 0.27, 0.95);
    assert!(farthest_cents(&notes, 57.0) < 3.0, "{notes:?}");
}

#[test]
fn hard_tuning_flattens_a_vibrato_and_a_slow_retune_keeps_it() {
    // 30 cents each way six times a second, around A3 10 cents sharp.
    let vibrato = |time: f64| Some(57.1 + 0.3 * (std::f64::consts::TAU * 6.0 * time).sin());
    let width = |retune_ms| {
        let output = Rig::new(retune(retune_ms), voice(vibrato)).render(2 * SECOND);
        let notes = between(&heard(&output, latency()), 0.5, 1.9);
        let (low, high) = (notes.iter()).fold((f64::MAX, f64::MIN), |(low, high), note| {
            (low.min(*note), high.max(*note))
        });
        (
            100.0 * (high - low),
            notes.iter().sum::<f64>() / notes.len() as f64,
        )
    };
    let (hard, hard_middle) = width(0.0);
    assert!(hard < 15.0, "hard tuning leaves {hard} cents of vibrato");
    assert!((hard_middle - 57.0).abs() < 0.03, "{hard_middle}");
    let (slow, slow_middle) = width(300.0);
    assert!(
        slow > 40.0,
        "a slow retune leaves only {slow} cents of vibrato"
    );
    assert!((slow_middle - 57.0).abs() < 0.05, "{slow_middle}");
}

#[test]
fn a_scale_moves_the_voice_to_its_nearest_note() {
    // C#4 a little low, in C major: D4 is nearer than C4.
    let state = PitchCorrectionState {
        key: Key::C,
        scale: Scale::Major,
        retune_ms: 0.0,
        ..PitchCorrectionState::default()
    };
    let output = Rig::new(state, voice(|_| Some(61.4))).render(SECOND);
    let notes = between(&heard(&output, latency()), 0.1, 0.95);
    assert!(farthest_cents(&notes, 62.0) < 3.0, "{notes:?}");
}

#[test]
fn held_notes_take_the_place_of_the_scale_while_they_are_held() {
    // A3 sharp the whole time, and C held from 0.6 s to 1.2 s: C4 is the nearest C.
    let sharp = voice(|_| Some(57.3));
    let notes = held(60, 0.6, 1.2).to_vec();
    let output = Rig::with(retune(0.0), sharp, false, notes, true).render(2 * SECOND);
    let heard = heard(&output, latency());
    let before = between(&heard, 0.2, 0.58);
    assert!(farthest_cents(&before, 57.0) < 3.0, "{before:?}");
    let during = between(&heard, 0.63, 1.18);
    assert!(farthest_cents(&during, 60.0) < 3.0, "{during:?}");
    let after = between(&heard, 1.25, 1.9);
    assert!(farthest_cents(&after, 57.0) < 3.0, "{after:?}");
}

#[test]
fn notes_keep_their_place_in_time_and_the_sound_its_length() {
    // Notes of 300 ms with 300 ms between them, each off its note.
    let phrase = |time: f64| (time % 0.6 < 0.3).then_some(57.3 + (time / 0.6).floor());
    let latency = latency() as usize;
    let input: Vec<f32> = {
        let mut signal = voice(phrase);
        (0..3 * SECOND)
            .map(|frame| signal(frame as u64)[0])
            .collect()
    };
    let output = Rig::new(retune(20.0), voice(phrase)).render(3 * SECOND);
    let (before, after) = (onsets(&input), onsets(&output));
    assert_eq!(before.len(), 5);
    assert_eq!(before.len(), after.len(), "{after:?}");
    for (before, after) in before.iter().zip(&after) {
        let late = *after as i64 - (*before + latency) as i64;
        assert!(late.abs() <= 24, "a note {late} frames off");
    }
}

#[test]
fn what_has_no_pitch_passes_as_it_came_as_late_as_reported() {
    let latency = latency() as usize;
    for (name, signal, again) in [
        ("noise", noise(), noise()),
        (
            "a chord",
            chord(&[57.0, 61.0, 64.0]),
            chord(&[57.0, 61.0, 64.0]),
        ),
    ] {
        let mut dry = again;
        let input: Vec<f32> = (0..SECOND).map(|frame| dry(frame as u64)[0]).collect();
        let output = Rig::new(retune(0.0), signal).render(SECOND);
        // How alike the output is to the input `lag` frames before, from 1 for the same to 0.
        let likeness = |lag: usize| -> f64 {
            let pairs = || output.iter().skip(lag).zip(&input).skip(SECOND / 4);
            let product: f64 = pairs().map(|(out, input)| f64::from(out * input)).sum();
            let power = |side: fn(&(&f32, &f32)) -> f32| {
                pairs()
                    .map(|pair| f64::from(side(&pair)).powi(2))
                    .sum::<f64>()
            };
            product / (power(|pair| *pair.0) * power(|pair| *pair.1)).sqrt()
        };
        let lag = (latency - 50..latency + 50)
            .max_by(|a, b| likeness(*a).total_cmp(&likeness(*b)))
            .unwrap();
        assert!(lag.abs_diff(latency) <= 1, "{name}: {lag} for {latency}");
        let alike = likeness(lag);
        assert!(alike > 0.999, "{name}: alike by {alike}");
    }
}
