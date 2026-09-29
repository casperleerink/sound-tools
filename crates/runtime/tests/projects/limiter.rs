//! The built-in limiter in the chain of a real track: an outside agent changes it by file while
//! the project plays and the change is heard, it comes back as it was after close and reopen, a
//! render is the same every time to the byte, its lookahead is a latency the project makes up
//! for, and it sounds as the limiter of the master does with the same settings.

use limiter::{LimiterState, Lookahead};
use sound_core::{Changes, InstanceId};

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

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte.
#[test]
fn the_limiter_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano_through(&record("{}"));
    let id = InstanceId::new("arrangement/piano/peaks").unwrap();
    let limiter = harness.project.resolve::<LimiterState>(&id).unwrap();
    let sound = LimiterState {
        gain_db: 6.5,
        ceiling_db: -3.5,
        release_ms: 250.0,
        lookahead: Lookahead::Five,
    };
    let mut changes = Changes::new();
    changes.set(&limiter, sound);
    harness.project.commit("Change limiter", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(LIMITER_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows.
    assert_eq!(
        file,
        r#"{
  "tool": "limiter",
  "state": {"gain_db": 6.5, "ceiling_db": -3.5, "release_ms": 250.0, "lookahead_ms": 5}
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let limiter = harness.project.resolve::<LimiterState>(&id).unwrap();
    assert_eq!(harness.project.state(&limiter), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(LIMITER_FILE)).unwrap(),
        file
    );
    let second = harness.play(2 * BAR);
    let bytes = |samples: &[f32]| -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    };
    assert_eq!(bytes(&first), bytes(&second));
    assert!(peak(&first) > amplitude(-6.0));
    assert!(peak(&first) <= amplitude(-3.5));
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

#[test]
fn a_limiter_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano_through(&record(r#"{"ceiling_db": -3.0}"#));
    let wrong = record(r#"{"lookahead_ms": 10}"#);
    assert_eq!(harness.write_and_apply(LIMITER_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0]
            .message
            .contains("lookahead_ms must be 0, 1 or 5, not 10"),
        "{}",
        problems[0].message
    );
    let wrong = record(r#"{"ceiling_db": 3.0}"#);
    harness.write_and_apply(LIMITER_FILE, &wrong);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: ceiling_db must be from -24 to 0, not 3"
    );
    // What loaded last still plays.
    let id = InstanceId::new("arrangement/piano/peaks").unwrap();
    let limiter = harness.project.resolve::<LimiterState>(&id).unwrap();
    assert_eq!(harness.project.state(&limiter).unwrap().ceiling_db, -3.0);
}
