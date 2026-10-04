//! The built-in filter in the chain of a real track: an outside agent changes it by file while
//! the project plays and the change is heard.

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

/// Frames `from` up to `to` of a stereo render: the piano, the one track.
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
