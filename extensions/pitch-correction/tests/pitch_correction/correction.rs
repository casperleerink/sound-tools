//! Where the voice goes, how fast, and what is left alone.

use pitch_correction::{Key, PitchCorrection, PitchCorrectionState, Scale};
use sound_core::Processor;

use crate::support::{
    Rig, SAMPLE_RATE, Signal, between, chord, farthest_cents, heard, held, hz, noise, onsets, voice,
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
fn a_note_sung_off_is_half_way_to_its_note_after_the_retune_time() {
    // A3 30 cents sharp, from a quarter of a second on, a retune of 100 ms.
    let sharp = voice(|time| (time >= 0.25).then_some(57.3));
    let output = Rig::new(retune(100.0), sharp).render(SECOND);
    let heard = heard(&output, latency());
    let off = |from: f64, to: f64| {
        let notes = between(&heard, from, to);
        100.0 * (notes.iter().sum::<f64>() / notes.len() as f64 - 57.0)
    };
    // Half the 30 cents after one retune time, a quarter after two.
    let one = off(0.34, 0.36);
    assert!((10.0..20.0).contains(&one), "{one} cents off after 100 ms");
    let two = off(0.44, 0.46);
    assert!((4.0..11.0).contains(&two), "{two} cents off after 200 ms");
    let late = between(&heard, 0.9, 0.95);
    assert!(farthest_cents(&late, 57.0) < 2.0, "{late:?}");
}

#[test]
fn a_retune_of_zero_takes_the_note_at_once() {
    let sharp = voice(|time| (time >= 0.25).then_some(57.3));
    let output = Rig::new(retune(0.0), sharp).render(SECOND);
    // From when the tracker has found the note: it needs two or three of its periods.
    let notes = between(&heard(&output, latency()), 0.29, 0.95);
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
    // The head stays within 0.6 of a period of the latency, also through a rest, where
    // nothing moves it back: 0.6 of the period of the lowest note.
    let window = (0.6 * f64::from(SAMPLE_RATE) / hz(57.3)).ceil() as i64;
    for (before, after) in before.iter().zip(&after) {
        let late = *after as i64 - (*before + latency) as i64;
        assert!(late.abs() <= window, "a note {late} frames off");
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
        // The tracker can follow a chord for a few milliseconds before it lets go, and a jump
        // of the head then leaves it a few frames off, within its window.
        assert!(lag.abs_diff(latency) <= 5, "{name}: {lag} for {latency}");
        let alike = likeness(lag);
        assert!(alike > 0.99, "{name}: alike by {alike}");
    }
}

#[test]
fn each_side_of_a_sudden_note_change_gets_its_own_correction() {
    // A pure tone, so each cycle out can be timed: A3 30 cents sharp, then from 0.5 s on C4
    // 40 cents sharp. A reading lined up with the wrong frames would move the end of the first
    // note by the second's correction, or the other way round.
    let mut phase = 0.0;
    let change = 0.5 * f64::from(SAMPLE_RATE);
    let sine: Signal = Box::new(move |frame| {
        let note = if (frame as f64) < change { 57.3 } else { 60.4 };
        phase += std::f64::consts::TAU * hz(note) / f64::from(SAMPLE_RATE);
        [(0.4 * phase.sin()) as f32; 2]
    });
    let output = Rig::new(retune(0.0), sine).render(SECOND);
    let latency = f64::from(latency());
    let mut crossings = Vec::new();
    for (frame, pair) in output.windows(2).enumerate() {
        if pair[0] < 0.0 && pair[1] >= 0.0 {
            let between = f64::from(pair[0] / (pair[0] - pair[1]));
            crossings.push(frame as f64 + between);
        }
    }
    let cycles = crossings.windows(2).map(|pair| {
        let hz = f64::from(SAMPLE_RATE) / (pair[1] - pair[0]);
        let input = ((pair[0] + pair[1]) / 2.0 - latency) / f64::from(SAMPLE_RATE);
        (input, 69.0 + 12.0 * (hz / 440.0).log2())
    });
    // Each cycle from 0.2 s on, but those within 10 ms of the change, about two cycles.
    let (mut first, mut second) = (0, 0);
    for (input, note) in cycles.filter(|(input, _)| *input > 0.2) {
        let expected = match input {
            input if input < 0.49 => {
                first += 1;
                57.0
            }
            input if input > 0.51 => {
                second += 1;
                60.0
            }
            _ => continue,
        };
        assert!((note - expected).abs() < 0.03, "{note} at {input} s");
    }
    // Every cycle of both notes was there to check: 0.29 s of A3, and the rest of C4.
    assert!(first >= 60, "{first} cycles of the first note");
    assert!(second >= 100, "{second} cycles of the second note");
}

#[test]
fn every_voice_range_reads_every_note_from_its_lowest_to_its_highest() {
    use pitch_correction::Voice;
    use sound_media::analysis::{LOWEST_HZ, Pitch, PitchTracker};
    let rate = f64::from(SAMPLE_RATE);
    let note_of = |hz: f64| 69.0 + 12.0 * (hz / 440.0).log2();
    for range in Voice::ALL {
        let (lowest, highest) = range.range_hz();
        let (from, to) = (
            note_of(lowest.max(LOWEST_HZ)).ceil() as i32,
            note_of(highest).floor() as i32,
        );
        for note in from..=to {
            let mut tracker = PitchTracker::new(rate, range.tracker_settings());
            let mut sing = voice(move |_| Some(f64::from(note)));
            let frames: Vec<f32> = (0..(0.4 * rate) as u64)
                .map(|frame| sing(frame)[0])
                .collect();
            tracker.push(frames);
            let read = tracker.pitch();
            assert!(
                matches!(read, Pitch::Note(read) if (read - f64::from(note)).abs() < 0.05),
                "{range:?}: {note} read as {read:?}"
            );
        }
    }
}
