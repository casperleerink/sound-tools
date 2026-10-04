//! The built-in limiter in the chain of a real track: an outside agent changes it by file while
//! the project plays and the change is heard, its lookahead is a latency the project makes up
//! for, and it sounds as the limiter of the master does with the same settings.

use crate::support::{BAR, Harness, clip};

const FOLDER: &str = "state/arrangement/piano";
const LIMITER_FILE: &str = "state/arrangement/piano/peaks.json";

/// A chord for four bars, loud enough to go over -6 dBFS.
fn loud_piano(harness: &mut Harness) {
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    harness.write_track("piano", 1, 0.5, &[("chord", clip(0, 15360, &chord))]);
}

/// A track that plays the chord through a limiter named `peaks`.
fn piano_through(limiter: &str) -> Harness {
    let mut harness = Harness::new();
    loud_piano(&mut harness);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["peaks"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(LIMITER_FILE, limiter),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
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
    assert_eq!(harness.write_and_apply(LIMITER_FILE, &low), 1);
    assert_eq!(harness.project.problems(), []);
    let after = harness.play(BAR / 2);
    assert!(peak(&after) <= amplitude(-24.0), "{}", peak(&after));

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(LIMITER_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["ceiling_db"], -1.0);
    let undone = harness.play(BAR / 2);
    assert!(peak(&undone[BAR / 4..]) > amplitude(-20.0));
}

/// A limiter whose ceiling the track never reaches only looks ahead: it delays its track by
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

/// The limiter on the track and the limiter of the master are one limiter: with the same
/// settings, the track through the device into a master that is not reached sounds exactly as
/// the track straight into a master set that way.
#[test]
fn the_device_sounds_as_the_limiter_of_the_master() {
    let mut device = piano_through(&record(
        r#"{"ceiling_db": -6.0, "release_ms": 50.0, "lookahead_ms": 5}"#,
    ));
    let mut master = Harness::new();
    loud_piano(&mut master);
    let arrangement = r#"{"tool": "arrangement", "state": {"master": {"limiter": {"ceiling_db": -6.0, "release_ms": 50.0, "lookahead_ms": 5.0}}}}"#;
    assert_eq!(
        master.write_and_apply("state/arrangement/instance.json", arrangement),
        1
    );
    let heard = device.play_from_the_start(2 * BAR);
    assert!(peak(&heard) <= amplitude(-6.0));
    assert_eq!(heard, master.play_from_the_start(2 * BAR));
    assert!(peak(&heard) > amplitude(-6.5));
}
