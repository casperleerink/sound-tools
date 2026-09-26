//! The built-in filter in the chain of a real track: an outside agent changes it by file while
//! the project plays and the change is heard, it comes back as it was after close and reopen,
//! and a render is the same every time, to the byte.

use filter::{FilterState, FilterType, Slope};
use sound_core::{Changes, InstanceId};

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const FILTER_FILE: &str = "state/arrangement/piano/tone.json";

/// A track that holds a chord for four bars and plays it through a filter named `tone`.
fn piano_through(filter: &str) -> Harness {
    let mut harness = Harness::new();
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    harness.write_track("piano", 1, 0.15, &[("chord", clip(0, 15360, &chord))]);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["tone"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(FILTER_FILE, filter),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "filter", "state": {state}}}"#)
}

/// The sound of the piano and nothing else: the default track of the project is silent.
fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

#[test]
fn an_outside_edit_of_the_filter_while_it_plays_is_heard_and_undone_in_one_step() {
    let bright = record(r#"{"cutoff_hz": 8000.0}"#);
    let dark = record(r#"{"cutoff_hz": 300.0, "resonance": 0.5, "slope": 24}"#);

    // What the track plays with each filter from the start: the references.
    let reference = |filter: &str| {
        let mut harness = piano_through(filter);
        harness.play_from_the_start(2 * BAR)
    };
    let (bright_all, dark_all) = (reference(&bright), reference(&dark));
    assert!(difference(&bright_all, &dark_all).is_some());

    // One session: the bright filter for half a bar, then an agent writes the dark one while
    // it plays. The edit applies at the next block of the engine.
    let mut harness = piano_through(&bright);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert_eq!(played, frames(&bright_all, 0, BAR / 2));
    assert_eq!(harness.write_and_apply(FILTER_FILE, &dark), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms and the filter settling, the track sounds
    // as if the dark filter had been there all along. 100 ms is plenty for both.
    let settled = BAR / 2 + 4_800;
    let (heard, expected) = (
        frames(&played, settled, BAR + BAR / 2),
        frames(&dark_all, settled, BAR + BAR / 2),
    );
    let largest = heard
        .iter()
        .zip(expected)
        .map(|(heard, expected)| (heard - expected).abs())
        .fold(0.0, f32::max);
    assert!(largest < 1e-5, "{largest}");
    // And it is not what the bright one plays.
    assert!(difference(heard, frames(&bright_all, settled, BAR + BAR / 2)).is_some());

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(FILTER_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["cutoff_hz"], 8000.0);
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte. A render is the same every time, also with the LFO moving.
#[test]
fn the_filter_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano_through(&record("{}"));
    let id = InstanceId::new("arrangement/piano/tone").unwrap();
    let filter = harness.project.resolve::<FilterState>(&id).unwrap();
    let sound = FilterState {
        kind: FilterType::BandPass,
        cutoff_hz: 900.0,
        resonance: 0.65,
        slope: Slope::TwentyFour,
        drive_db: 6.0,
        mix: 0.8,
        lfo_rate_hz: 3.0,
        lfo_depth_octaves: 1.5,
    };
    let mut changes = Changes::new();
    changes.set(&filter, sound);
    harness.project.commit("Change filter", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(FILTER_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows.
    assert_eq!(
        file,
        r#"{
  "tool": "filter",
  "state": {
    "type": "band_pass",
    "cutoff_hz": 900.0,
    "resonance": 0.65,
    "slope": 24,
    "drive_db": 6.0,
    "mix": 0.8,
    "lfo_rate_hz": 3.0,
    "lfo_depth_octaves": 1.5
  }
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let filter = harness.project.resolve::<FilterState>(&id).unwrap();
    assert_eq!(harness.project.state(&filter), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(FILTER_FILE)).unwrap(),
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

#[test]
fn a_filter_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano_through(&record(r#"{"cutoff_hz": 500.0}"#));
    let wrong = record(r#"{"cutoff_hz": 500.0, "slope": 18}"#);
    assert_eq!(harness.write_and_apply(FILTER_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0]
            .message
            .contains("slope must be 12 or 24, not 18"),
        "{}",
        problems[0].message
    );
    let wrong = record(r#"{"cutoff_hz": 50000.0}"#);
    harness.write_and_apply(FILTER_FILE, &wrong);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: cutoff_hz must be from 20 to 20000, not 50000"
    );
}
