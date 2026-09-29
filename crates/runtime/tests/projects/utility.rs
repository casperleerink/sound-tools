//! The built-in utility in the chain of a real track: at its defaults the track sounds as it
//! does without it, to the bit, an outside agent changes it by file while the project plays and
//! the change is heard and undone in one step, it comes back as it was after close and reopen,
//! and a record out of range is reported.

use sound_core::{Changes, InstanceId};
use utility::{Channels, UtilityState};

use crate::support::{BAR, Harness, clip};

const FOLDER: &str = "state/arrangement/piano";
const UTILITY_FILE: &str = "state/arrangement/piano/trim.json";

/// A track that plays short chords for four bars, through a utility named `trim` when there is
/// a record for it.
fn piano(utility: Option<&str>) -> Harness {
    let mut harness = Harness::new();
    let chords = [
        (0, 480, 48),
        (0, 480, 55),
        (1920, 480, 60),
        (3840, 480, 64),
        (5760, 480, 67),
    ];
    harness.write_track("piano", 1, 0.15, &[("chords", clip(0, 15360, &chords))]);
    if let Some(utility) = utility {
        let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["trim"]}}"#;
        let paths = [
            harness.write(&format!("{FOLDER}/instance.json"), track),
            harness.write(UTILITY_FILE, utility),
        ];
        assert_eq!(harness.apply(&paths), 2);
    }
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "utility", "state": {state}}}"#)
}

fn left(samples: &[f32]) -> Vec<f32> {
    samples.iter().step_by(2).copied().collect()
}

fn right(samples: &[f32]) -> Vec<f32> {
    samples.iter().skip(1).step_by(2).copied().collect()
}

#[test]
fn at_its_defaults_the_track_sounds_as_it_does_without_it() {
    let without = piano(None).play_from_the_start(2 * BAR);
    let with = piano(Some(&record("{}"))).play_from_the_start(2 * BAR);
    assert_eq!(with, without);
    assert!(with.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn an_outside_edit_of_the_utility_while_it_plays_is_heard_and_undone_in_one_step() {
    let dry = piano(None).play_from_the_start(2 * BAR);

    // One session: half a bar at the defaults, then an agent turns the left channel upside
    // down while it plays. The edit applies at the next block of the engine.
    let mut harness = piano(Some(&record("{}")));
    let mut played = harness.play_from_the_start(BAR / 2);
    assert_eq!(played, dry[..BAR]);
    let inverted = record(r#"{"invert_left": true}"#);
    assert_eq!(harness.write_and_apply(UTILITY_FILE, &inverted), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR / 2));

    // After its glide of 20 ms the left channel is the dry one upside down, to the bit, and
    // the right channel is as it was.
    let glided = BAR / 2 + 1_024;
    let heard = &played[2 * glided..];
    let expected = &dry[2 * glided..2 * BAR];
    let upside_down: Vec<f32> = left(expected).iter().map(|sample| -sample).collect();
    assert_eq!(left(heard), upside_down);
    assert_eq!(right(heard), right(expected));

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(UTILITY_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_ne!(state["state"]["invert_left"], true);
    let after_undo = harness.play(BAR / 2);
    assert_eq!(
        after_undo[2 * 1_024..],
        dry[2 * (BAR + 1_024)..2 * BAR + BAR]
    );
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte. A render is the same every time.
#[test]
fn the_utility_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano(Some(&record("{}")));
    let id = InstanceId::new("arrangement/piano/trim").unwrap();
    let utility = harness.project.resolve::<UtilityState>(&id).unwrap();
    let sound = UtilityState {
        gain_db: -4.5,
        pan: 0.25,
        width: 1.4,
        bass_mono: true,
        bass_mono_hz: 180.0,
        channels: Channels::Swap,
        invert_left: false,
        invert_right: true,
        mute: false,
    };
    let mut changes = Changes::new();
    changes.set(&utility, sound);
    harness.project.commit("Change utility", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(UTILITY_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows.
    assert_eq!(
        file,
        r#"{
  "tool": "utility",
  "state": {
    "gain_db": -4.5,
    "pan": 0.25,
    "width": 1.4,
    "bass_mono": true,
    "bass_mono_hz": 180.0,
    "channels": "swap",
    "invert_left": false,
    "invert_right": true,
    "mute": false
  }
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let utility = harness.project.resolve::<UtilityState>(&id).unwrap();
    assert_eq!(harness.project.state(&utility), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(UTILITY_FILE)).unwrap(),
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
fn a_utility_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano(Some(&record(r#"{"width": 0.5}"#)));
    let wrong = record(r#"{"width": 3.0}"#);
    assert_eq!(harness.write_and_apply(UTILITY_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: width must be from 0 to 2, not 3"
    );
    let wrong = record(r#"{"channels": "mono"}"#);
    harness.write_and_apply(UTILITY_FILE, &wrong);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].message.contains("channels"),
        "{}",
        problems[0].message
    );
    let id = InstanceId::new("arrangement/piano/trim").unwrap();
    let utility = harness.project.resolve::<UtilityState>(&id).unwrap();
    assert_eq!(harness.project.state(&utility).unwrap().width, 0.5);
}
