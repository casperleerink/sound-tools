//! The built-in limiter in a rack after the synth: an outside agent changes it by file while the
//! project plays and the change is heard, and its lookahead is a latency the project makes up
//! for.

use crate::support::{BAR, Harness, clip, synth};

const LIMITER_FILE: &str = "state/rack/peaks.json";

/// A rack that plays a chord for four bars, loud enough to go over -6 dBFS, through a limiter
/// named `peaks`.
fn piano_through(limiter: &str) -> Harness {
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    let (synth, chord) = (synth(0.5), clip(0, 15360, &chord));
    Harness::rack(&[
        ("instrument", &synth),
        ("chord", &chord),
        ("peaks", limiter),
    ])
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "limiter", "state": {state}}}"#)
}

fn amplitude(db: f32) -> f32 {
    10_f64.powf(f64::from(db) / 20.0) as f32
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

#[test]
fn an_outside_edit_of_the_limiter_while_it_plays_is_heard_and_undone_in_one_step() {
    let mut harness = piano_through(&record("{}"));
    let played = harness.play_from_the_start(BAR / 2);
    // Over -24 dBFS, and under the ceiling of the default.
    assert!(peak(&played) > amplitude(-20.0), "{}", peak(&played));
    assert!(peak(&played) <= amplitude(-1.0));

    // An agent lowers the ceiling while it plays. From the next block on, nothing is over it.
    let low = record(r#"{"ceiling_db": -24.0}"#);
    let file_before = std::fs::read(harness.path(LIMITER_FILE)).unwrap();
    assert_eq!(harness.write_and_apply(LIMITER_FILE, &low), 1);
    assert_eq!(harness.project.problems(), []);
    let after = harness.play(BAR / 2);
    assert!(peak(&after) <= amplitude(-24.0), "{}", peak(&after));

    // One undo takes the agent's edit back, file and sound: the file byte for byte.
    assert!(harness.project.undo().unwrap().is_some());
    assert_eq!(
        std::fs::read(harness.path(LIMITER_FILE)).unwrap(),
        file_before
    );
    let undone = harness.play(BAR / 2);
    assert!(peak(&undone[BAR / 4..]) > amplitude(-20.0));
}

/// A limiter whose ceiling the rack never reaches only looks ahead: it delays the rack by
/// 5 ms, and the project makes up for it. The render is the render without the lookahead,
/// sample for sample, and the project reports the latency.
#[test]
fn the_lookahead_is_a_latency_the_project_makes_up_for() {
    let untouched = |lookahead: u8| {
        record(&format!(
            r#"{{"ceiling_db": 0.0, "lookahead_ms": {lookahead}}}"#
        ))
    };
    let mut ahead = piano_through(&untouched(5));
    let mut plain = piano_through(&untouched(0));
    let heard = ahead.play(BAR);
    assert_eq!(ahead.project.engine().poll().unwrap().latency, 240);
    assert_eq!(plain.project.engine().poll().unwrap().latency, 0);
    assert_eq!(heard, plain.play(BAR));
    assert!(peak(&heard) > 0.1);
}
