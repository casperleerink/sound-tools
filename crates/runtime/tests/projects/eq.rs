//! The built-in EQ in the chain of a real track: an outside agent changes it by file while the
//! project plays and the change is heard, it comes back as it was after close and reopen, and a
//! render is the same every time, to the byte.

use eq::{Band, EqState, Shape};
use sound_core::{Changes, InstanceId};

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const EQ_FILE: &str = "state/arrangement/piano/tone.json";

/// A track that holds a chord for four bars and plays it through an EQ named `tone`.
fn piano_through(eq: &str) -> Harness {
    let mut harness = Harness::new();
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    harness.write_track("piano", 1, 0.15, &[("chord", clip(0, 15360, &chord))]);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["tone"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(EQ_FILE, eq),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "eq", "state": {state}}}"#)
}

fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

/// An agent writes bands 1 and 2 only: a cut of the lows and a bell. Bands 3 and 4, left out,
/// are reset to their defaults, as the doc says. The edit is heard after its glide, and one
/// undo takes it back.
#[test]
fn an_outside_edit_of_the_eq_while_it_plays_is_heard_and_undone_in_one_step() {
    let flat = record("{}");
    let shaped = record(
        r#"{"bands": [{"shape": "low_cut", "frequency_hz": 300.0}, {"gain_db": -12.0, "q": 2.0, "frequency_hz": 260.0}]}"#,
    );

    // What the track plays with each EQ from the start: the references.
    let reference = |eq: &str| {
        let mut harness = piano_through(eq);
        harness.play_from_the_start(2 * BAR)
    };
    let (flat_all, shaped_all) = (reference(&flat), reference(&shaped));
    assert!(difference(&flat_all, &shaped_all).is_some());

    // One session: the flat EQ for half a bar, then an agent writes the other while it plays.
    let mut harness = piano_through(&flat);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert_eq!(played, frames(&flat_all, 0, BAR / 2));
    assert_eq!(harness.write_and_apply(EQ_FILE, &shaped), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms and the bands settling, the track sounds
    // as if the new EQ had been there all along. 100 ms is plenty for both.
    let settled = BAR / 2 + 4_800;
    let (heard, expected) = (
        frames(&played, settled, BAR + BAR / 2),
        frames(&shaped_all, settled, BAR + BAR / 2),
    );
    let largest = heard
        .iter()
        .zip(expected)
        .map(|(heard, expected)| (heard - expected).abs())
        .fold(0.0, f32::max);
    assert!(largest < 1e-5, "{largest}");
    assert!(difference(heard, frames(&flat_all, settled, BAR + BAR / 2)).is_some());

    // What the agent left out is the default of its band, not what the file had before.
    let id = InstanceId::new("arrangement/piano/tone").unwrap();
    let eq = harness.project.resolve::<EqState>(&id).unwrap();
    let state = *harness.project.state(&eq).unwrap();
    assert_eq!(state.bands[0].shape, Shape::LowCut);
    assert_eq!(state.bands[1].shape, Shape::Bell);
    assert_eq!(state.bands[1].gain_db, -12.0);
    assert_eq!(state.bands[2], Band::default_at(2));

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(EQ_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["bands"][0]["shape"], "low_shelf");
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte.
#[test]
fn the_eq_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano_through(&record("{}"));
    let id = InstanceId::new("arrangement/piano/tone").unwrap();
    let eq = harness.project.resolve::<EqState>(&id).unwrap();
    let band = |on, shape, frequency_hz, gain_db, q| Band {
        on,
        shape,
        frequency_hz,
        gain_db,
        q,
    };
    let sound = EqState {
        bands: [
            band(true, Shape::LowCut, 120.0, 0.0, 1.2),
            band(true, Shape::Bell, 450.0, -4.5, 2.0),
            band(false, Shape::Notch, 3_000.0, 0.0, 8.0),
            band(true, Shape::HighCut, 9_000.0, 0.0, 0.71),
        ],
        output_gain_db: 1.5,
    };
    let mut changes = Changes::new();
    changes.set(&eq, sound);
    harness.project.commit("Change EQ", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(EQ_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows.
    assert_eq!(
        file,
        r#"{
  "tool": "eq",
  "state": {
    "bands": [
      {"on": true, "shape": "low_cut", "frequency_hz": 120.0, "gain_db": 0.0, "q": 1.2},
      {"on": true, "shape": "bell", "frequency_hz": 450.0, "gain_db": -4.5, "q": 2.0},
      {"on": false, "shape": "notch", "frequency_hz": 3000.0, "gain_db": 0.0, "q": 8.0},
      {"on": true, "shape": "high_cut", "frequency_hz": 9000.0, "gain_db": 0.0, "q": 0.71}
    ],
    "output_gain_db": 1.5
  }
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let eq = harness.project.resolve::<EqState>(&id).unwrap();
    assert_eq!(harness.project.state(&eq), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(EQ_FILE)).unwrap(),
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
fn an_eq_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano_through(&record(r#"{"output_gain_db": -3.0}"#));
    let wrong = record(r#"{"bands": [{}, {"frequency_hz": 50000.0}]}"#);
    assert_eq!(harness.write_and_apply(EQ_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert_eq!(
        problems[0].message,
        "state: band 2: frequency_hz must be from 20 to 20000, not 50000"
    );
    let wrong = record(r#"{"bands": [{"shape": "peak"}]}"#);
    harness.write_and_apply(EQ_FILE, &wrong);
    let problems = harness.project.problems();
    assert!(
        problems[0].message.contains("unknown variant `peak`"),
        "{}",
        problems[0].message
    );
    // What it had is still what plays.
    let id = InstanceId::new("arrangement/piano/tone").unwrap();
    let eq = harness.project.resolve::<EqState>(&id).unwrap();
    assert_eq!(harness.project.state(&eq).unwrap().output_gain_db, -3.0);
}
