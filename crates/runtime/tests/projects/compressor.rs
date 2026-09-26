//! The built-in compressor in the chain of a real track: an outside agent changes it by file
//! while the project plays and the change is heard, it comes back as it was after close and
//! reopen, a render is the same every time to the byte, and its lookahead is a latency the
//! project makes up for.

use compressor::{CompressorState, Lookahead};
use sound_core::{Changes, InstanceId};

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const COMPRESSOR_FILE: &str = "state/arrangement/piano/glue.json";

/// A track that plays a chord for four bars through a compressor named `glue`.
fn piano_through(compressor: &str) -> Harness {
    let mut harness = Harness::new();
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    harness.write_track("piano", 1, 0.15, &[("chord", clip(0, 15360, &chord))]);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["glue"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(COMPRESSOR_FILE, compressor),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "compressor", "state": {state}}}"#)
}

/// The sound of the piano and nothing else: the default track of the project is silent.
fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

#[test]
fn an_outside_edit_of_the_compressor_while_it_plays_is_heard_and_undone_in_one_step() {
    let gentle = record(r#"{"threshold_db": -6.0, "attack_ms": 1.0, "release_ms": 10.0}"#);
    let heavy =
        record(r#"{"threshold_db": -40.0, "ratio": 8.0, "attack_ms": 1.0, "release_ms": 10.0}"#);

    // What the track plays with each compressor from the start: the references.
    let reference = |compressor: &str| {
        let mut harness = piano_through(compressor);
        harness.play_from_the_start(2 * BAR)
    };
    let (gentle_all, heavy_all) = (reference(&gentle), reference(&heavy));
    assert!(difference(&gentle_all, &heavy_all).is_some());

    // One session: the gentle one for half a bar, then an agent writes the heavy one while it
    // plays. The edit applies at the next block of the engine.
    let mut harness = piano_through(&gentle);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert_eq!(played, frames(&gentle_all, 0, BAR / 2));
    assert_eq!(harness.write_and_apply(COMPRESSOR_FILE, &heavy), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms, the hold of 10 ms and many release times,
    // the track sounds as if the heavy compressor had been there all along. 250 ms is plenty.
    let settled = BAR / 2 + 12_000;
    let (heard, expected) = (
        frames(&played, settled, BAR + BAR / 2),
        frames(&heavy_all, settled, BAR + BAR / 2),
    );
    let largest = heard
        .iter()
        .zip(expected)
        .map(|(heard, expected)| (heard - expected).abs())
        .fold(0.0, f32::max);
    assert!(largest < 1e-5, "{largest}");
    // And it is not what the gentle one plays.
    assert!(difference(heard, frames(&gentle_all, settled, BAR + BAR / 2)).is_some());

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(COMPRESSOR_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["threshold_db"], -6.0);
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte.
#[test]
fn the_compressor_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano_through(&record("{}"));
    let id = InstanceId::new("arrangement/piano/glue").unwrap();
    let compressor = harness.project.resolve::<CompressorState>(&id).unwrap();
    let sound = CompressorState {
        threshold_db: -32.5,
        ratio: 6.0,
        attack_ms: 3.5,
        release_ms: 250.0,
        knee_db: 9.0,
        makeup_db: 7.5,
        mix: 0.8,
        lookahead: Lookahead::One,
    };
    let mut changes = Changes::new();
    changes.set(&compressor, sound);
    harness
        .project
        .commit("Change compressor", changes)
        .unwrap();
    let file = std::fs::read_to_string(harness.path(COMPRESSOR_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows.
    assert_eq!(
        file,
        r#"{
  "tool": "compressor",
  "state": {
    "threshold_db": -32.5,
    "ratio": 6.0,
    "attack_ms": 3.5,
    "release_ms": 250.0,
    "knee_db": 9.0,
    "makeup_db": 7.5,
    "mix": 0.8,
    "lookahead_ms": 1
  }
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let compressor = harness.project.resolve::<CompressorState>(&id).unwrap();
    assert_eq!(harness.project.state(&compressor), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(COMPRESSOR_FILE)).unwrap(),
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
    assert!(first.iter().any(|sample| sample.abs() > 0.01));
}

/// A compressor that does nothing but look ahead delays its track by 10 ms, and the project
/// makes up for it: the render is the render without the lookahead, sample for sample, and the
/// project reports the latency.
#[test]
fn the_lookahead_is_a_latency_the_project_makes_up_for() {
    let untouched = |lookahead: u8| {
        record(&format!(
            r#"{{"ratio": 1.0, "threshold_db": 0.0, "lookahead_ms": {lookahead}}}"#
        ))
    };
    let mut ahead = piano_through(&untouched(10));
    let mut plain = piano_through(&untouched(0));
    let heard = ahead.play(BAR);
    assert_eq!(ahead.project.engine().poll().unwrap().latency, 480);
    assert_eq!(heard, plain.play(BAR));
    assert!(heard.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn a_compressor_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano_through(&record(r#"{"ratio": 2.0}"#));
    let wrong = record(r#"{"ratio": 2.0, "lookahead_ms": 5}"#);
    assert_eq!(harness.write_and_apply(COMPRESSOR_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0]
            .message
            .contains("lookahead_ms must be 0, 1 or 10, not 5"),
        "{}",
        problems[0].message
    );
    let wrong = record(r#"{"threshold_db": 6.0}"#);
    harness.write_and_apply(COMPRESSOR_FILE, &wrong);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: threshold_db must be from -60 to 0, not 6"
    );
}
