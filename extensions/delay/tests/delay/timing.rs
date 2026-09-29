//! When the repeats come: after the time in ms to the frame, after the note at the tempo, after
//! the new note when the tempo changes while it plays, and on which side in ping-pong.

use delay::{DelayState, Division, Feel, response};
use sound_core::{Tempo, TempoChange, TempoMap, Ticks, TimeSignature};

use crate::support::{
    Rig, SAMPLE_RATE, SECOND, clicks, db, impulse, loudest_frame, peak, rms, sine_for, tempo, wet,
};

/// The first three repeats of a click at frame 0 start exactly one time apart, in the left
/// channel. A repeat starts at its first frame over a tenth of its peak: the cuts spread a click
/// over a few frames, and the loudest of them moves with the sample rate and the passes.
fn assert_repeats_every(rig: &mut Rig, time_frames: usize, label: &str) {
    let [left, _] = rig.render(time_frames * 4);
    let starts: Vec<usize> = (1..=3)
        .map(|number| {
            let from = number * time_frames - time_frames / 2;
            let window = &left[from..from + time_frames];
            let loudest = peak(window);
            let start = window
                .iter()
                .position(|sample| sample.abs() > loudest / 10.0);
            from + start.unwrap_or_default()
        })
        .collect();
    let expected: Vec<usize> = (1..=3).map(|number| number * time_frames).collect();
    assert_eq!(starts, expected, "{label}");
}

#[test]
fn a_free_time_repeats_a_click_after_exactly_that_time() {
    for sample_rate in [44_100, 48_000, 96_000] {
        for time_ms in [1.0, 100.0, 333.0, 4_000.0] {
            let state = DelayState {
                sync: false,
                time_ms,
                feedback: 0.5,
                ..wet()
            };
            let frames = (f64::from(time_ms) * f64::from(sample_rate) / 1_000.0).round() as usize;
            let mut rig = Rig::at_rate(state, impulse(), sample_rate);
            let label = format!("{time_ms} ms at {sample_rate} Hz");
            assert_repeats_every(&mut rig, frames, &label);
        }
    }
}

#[test]
fn a_synced_time_is_its_note_at_the_tempo_of_the_project() {
    let cases = [
        (Division::Eighth, Feel::Straight, 120.0, 12_000),
        (Division::Eighth, Feel::Dotted, 120.0, 18_000),
        (Division::Eighth, Feel::Triplet, 120.0, 8_000),
        (Division::Quarter, Feel::Straight, 90.0, 32_000),
        (Division::Sixteenth, Feel::Straight, 150.0, 4_800),
        (Division::ThirtySecond, Feel::Triplet, 200.0, 1_200),
        (Division::Half, Feel::Dotted, 144.0, 60_000),
    ];
    for (division, feel, bpm, frames) in cases {
        let state = DelayState {
            division,
            feel,
            feedback: 0.5,
            ..wet()
        };
        let mut rig = Rig::new(state, impulse());
        rig.control.set_tempo_map(tempo(bpm));
        let label = format!("{division:?} {feel:?} at {bpm} bpm");
        assert_repeats_every(&mut rig, frames, &label);
    }
}

/// The tempo halves at bar 2 while the project plays. A click before it repeats after a
/// quarter at 120 bpm, a click after it after a quarter at 60.
#[test]
fn a_tempo_change_moves_the_repeats_while_it_plays() {
    let four_four = TimeSignature::new(4, 4).unwrap();
    let change = |tick, bpm| TempoChange {
        tick: Ticks(tick),
        bpm: Tempo::from_bpm(bpm).unwrap(),
    };
    let map = TempoMap::new(four_four, vec![change(0, 120.0), change(3_840, 60.0)]).unwrap();
    let state = DelayState {
        division: Division::Quarter,
        feedback: 0.0,
        ..wet()
    };
    // Bar 2 starts at 2 s.
    let (before, after) = (SECOND / 2, 5 * SECOND / 2);
    let mut rig = Rig::new(state, clicks(vec![before, after]));
    rig.control.set_tempo_map(map);
    rig.control.play();
    let [left, _] = rig.render(4 * SECOND);
    assert_eq!(
        loudest_frame(&left, before + 1, 2 * SECOND),
        before + SECOND / 2
    );
    assert_eq!(loudest_frame(&left, after + 1, 4 * SECOND), after + SECOND);
    // Nothing where the old time would have put it.
    assert!(peak(&left[after + SECOND / 2 - 10..after + SECOND / 2 + 10]) < 1e-6);
}

/// In ping-pong a tone comes back left, then right, then left, each repeat as loud as the
/// response and the feedback say. Side by side a click on the left stays on the left.
#[test]
fn ping_pong_sends_the_repeats_from_side_to_side() {
    let time = SECOND / 10;
    let state = DelayState {
        sync: false,
        time_ms: 100.0,
        feedback: 0.5,
        ping_pong: true,
        ..wet()
    };
    let tone = sine_for(1_000.0, 0.5, SECOND / 20, SAMPLE_RATE);
    let [left, right] = Rig::new(state, tone).render(5 * time);
    let middle = |samples: &[f32], number: usize| {
        rms(&samples[number * time + SECOND / 100..][..SECOND * 3 / 100])
    };
    let pass = f64::from(response(&state, 1_000.0, SAMPLE_RATE as f32));
    let tone_level = 0.5 / 2.0_f64.sqrt();
    for number in 1..=4 {
        let expected = tone_level * pass.powi(number as i32) * 0.5_f64.powi(number as i32 - 1);
        let (heard, other) = match number % 2 {
            1 => (&left, &right),
            _ => (&right, &left),
        };
        let error = db(middle(heard, number)) - db(expected);
        assert!(error.abs() < 0.1, "repeat {number}: {error:+.3} dB");
        let apart = db(middle(other, number)) - db(middle(heard, number));
        assert!(
            apart < -60.0,
            "repeat {number}: the other side {apart:.1} dB"
        );
    }

    let side_by_side = DelayState {
        ping_pong: false,
        ..state
    };
    let [left, right] = Rig::new(side_by_side, impulse()).render(5 * time);
    assert!(right.iter().all(|sample| *sample == 0.0));
    assert!(peak(&left[2 * time - 10..2 * time + 10]) > 0.1);
}
