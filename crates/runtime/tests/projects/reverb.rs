//! The built-in reverb in the chain of a real track: an outside agent changes it by file while
//! the project plays and the change is heard, it comes back as it was after close and reopen,
//! and a render is the same every time, to the byte.

use reverb::ReverbState;
use sound_core::{Changes, InstanceId};

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const REVERB_FILE: &str = "state/arrangement/piano/room.json";

/// A track that plays short chords for four bars through a reverb named `room`.
fn piano_through(reverb: &str) -> Harness {
    let mut harness = Harness::new();
    let chords = [
        (0, 480, 48),
        (0, 480, 55),
        (1920, 480, 60),
        (3840, 480, 64),
        (5760, 480, 67),
    ];
    harness.write_track("piano", 1, 0.15, &[("chords", clip(0, 15360, &chords))]);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["room"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(REVERB_FILE, reverb),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "reverb", "state": {state}}}"#)
}

fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

#[test]
fn an_outside_edit_of_the_reverb_while_it_plays_is_heard_and_undone_in_one_step() {
    let wet = record(r#"{"decay_seconds": 4.0, "mix": 0.8}"#);
    let dry = record(r#"{"mix": 0.0}"#);

    // What the track plays with no reverb in the mix, from the start: the reference.
    let dry_all = piano_through(&dry).play_from_the_start(2 * BAR);

    // One session: a long wet reverb for half a bar, then an agent writes mix 0 while it
    // plays. The edit applies at the next block of the engine.
    let mut harness = piano_through(&wet);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert!(difference(&played, frames(&dry_all, 0, BAR / 2)).is_some());
    assert_eq!(harness.write_and_apply(REVERB_FILE, &dry), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms the tail is gone from the output, which
    // is the dry sound to the bit, although the tail goes on inside the reverb.
    let glided = BAR / 2 + 1_024;
    let (heard, expected) = (
        frames(&played, glided, BAR + BAR / 2),
        frames(&dry_all, glided, BAR + BAR / 2),
    );
    assert_eq!(heard, expected);
    // And during the glide it was neither.
    let gliding = BAR / 2 + 480;
    assert!(
        difference(
            &played[2 * gliding..2 * gliding + 2],
            &dry_all[2 * gliding..2 * gliding + 2]
        )
        .is_some()
    );

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(REVERB_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["mix"], 0.8);
    let after_undo = harness.play(BAR / 4);
    assert!(
        difference(
            &after_undo,
            frames(&dry_all, BAR + BAR / 2, BAR + BAR / 2 + BAR / 4)
        )
        .is_some()
    );
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte. A render is the same every time.
#[test]
fn the_reverb_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano_through(&record("{}"));
    let id = InstanceId::new("arrangement/piano/room").unwrap();
    let reverb = harness.project.resolve::<ReverbState>(&id).unwrap();
    let sound = ReverbState {
        pre_delay_ms: 35.0,
        decay_seconds: 3.5,
        size: 0.8,
        damping: 0.25,
        diffusion: 0.9,
        low_cut_hz: 150.0,
        high_cut_hz: 6_000.0,
        width: 0.7,
        mix: 0.45,
        freeze: false,
    };
    let mut changes = Changes::new();
    changes.set(&reverb, sound);
    harness.project.commit("Change reverb", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(REVERB_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows.
    assert_eq!(
        file,
        r#"{
  "tool": "reverb",
  "state": {
    "pre_delay_ms": 35.0,
    "decay_seconds": 3.5,
    "size": 0.8,
    "damping": 0.25,
    "diffusion": 0.9,
    "low_cut_hz": 150.0,
    "high_cut_hz": 6000.0,
    "width": 0.7,
    "mix": 0.45,
    "freeze": false
  }
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let reverb = harness.project.resolve::<ReverbState>(&id).unwrap();
    assert_eq!(harness.project.state(&reverb), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(REVERB_FILE)).unwrap(),
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
fn a_reverb_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano_through(&record(r#"{"decay_seconds": 3.0}"#));
    let wrong = record(r#"{"decay_seconds": 100.0}"#);
    assert_eq!(harness.write_and_apply(REVERB_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: decay_seconds must be from 0.2 to 60, not 100"
    );
    let wrong = record(r#"{"freeze": "yes"}"#);
    harness.write_and_apply(REVERB_FILE, &wrong);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].message.contains("freeze"),
        "{}",
        problems[0].message
    );
    let id = InstanceId::new("arrangement/piano/room").unwrap();
    let reverb = harness.project.resolve::<ReverbState>(&id).unwrap();
    assert_eq!(harness.project.state(&reverb).unwrap().decay_seconds, 3.0);
}
