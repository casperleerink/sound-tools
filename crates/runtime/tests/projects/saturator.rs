//! The built-in saturator in the chain of a real track: an outside agent changes it by file
//! while the project plays and the change is heard, it comes back as it was after close and
//! reopen, a render is the same every time to the byte, and its oversampling is a latency the
//! project makes up for.

use saturator::{Curve, SaturatorState};
use sound_core::{Changes, InstanceId};

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const SATURATOR_FILE: &str = "state/arrangement/piano/heat.json";

/// A track that plays short chords for four bars, through a saturator named `heat` when there
/// is a record for it.
fn piano_through(saturator: Option<&str>) -> Harness {
    let mut harness = Harness::new();
    let chords = [
        (0, 480, 48),
        (0, 480, 55),
        (1920, 480, 60),
        (3840, 480, 64),
        (5760, 480, 67),
    ];
    harness.write_track("piano", 1, 0.15, &[("chords", clip(0, 15360, &chords))]);
    let Some(saturator) = saturator else {
        return harness;
    };
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["heat"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(SATURATOR_FILE, saturator),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "saturator", "state": {state}}}"#)
}

fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

#[test]
fn an_outside_edit_of_the_saturator_while_it_plays_is_heard_and_undone_in_one_step() {
    let hot = record(r#"{"curve": "tube", "drive_db": 24.0}"#);
    let dry = record(r#"{"curve": "tube", "drive_db": 24.0, "mix": 0.0}"#);

    // What the track plays with none of the saturated sound in the mix, from the start: the
    // reference.
    let dry_all = piano_through(Some(&dry)).play_from_the_start(2 * BAR);

    // One session: saturated for half a bar, then an agent writes mix 0 while it plays. The
    // edit applies at the next block of the engine.
    let mut harness = piano_through(Some(&hot));
    let mut played = harness.play_from_the_start(BAR / 2);
    assert!(difference(&played, frames(&dry_all, 0, BAR / 2)).is_some());
    assert_eq!(harness.write_and_apply(SATURATOR_FILE, &dry), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms the output is the dry sound to the bit.
    let glided = BAR / 2 + 1_024;
    let (heard, expected) = (
        frames(&played, glided, BAR + BAR / 2),
        frames(&dry_all, glided, BAR + BAR / 2),
    );
    assert_eq!(heard, expected);

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(SATURATOR_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["mix"], 1.0);
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
fn the_saturator_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano_through(Some(&record("{}")));
    let id = InstanceId::new("arrangement/piano/heat").unwrap();
    let saturator = harness.project.resolve::<SaturatorState>(&id).unwrap();
    let sound = SaturatorState {
        curve: Curve::Tape,
        drive_db: 14.5,
        tone_db: -3.0,
        output_db: 1.5,
        mix: 0.75,
    };
    let mut changes = Changes::new();
    changes.set(&saturator, sound);
    harness.project.commit("Change saturator", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(SATURATOR_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows: the state fits on one line.
    assert_eq!(
        file,
        r#"{
  "tool": "saturator",
  "state": {"curve": "tape", "drive_db": 14.5, "tone_db": -3.0, "output_db": 1.5, "mix": 0.75}
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let saturator = harness.project.resolve::<SaturatorState>(&id).unwrap();
    assert_eq!(harness.project.state(&saturator), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(SATURATOR_FILE)).unwrap(),
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

/// The oversampling delays the sound by 64 frames, and the saturator says so: the project
/// plays everything before it that much earlier. With none of the saturated sound in the mix
/// the track is the track without the saturator, sample for sample.
#[test]
fn the_oversampling_is_a_latency_the_project_makes_up_for() {
    let mut through = piano_through(Some(&record(r#"{"drive_db": 30.0, "mix": 0.0}"#)));
    let mut plain = piano_through(None);
    let heard = through.play(BAR);
    assert_eq!(through.project.engine().poll().unwrap().latency, 64);
    assert_eq!(heard, plain.play(BAR));
    assert!(heard.iter().any(|sample| sample.abs() > 0.01));
}

#[test]
fn a_saturator_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano_through(Some(&record(r#"{"drive_db": 12.0}"#)));
    let wrong = record(r#"{"drive_db": 40.0}"#);
    assert_eq!(harness.write_and_apply(SATURATOR_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: drive_db must be from 0 to 36, not 40"
    );
    let wrong = record(r#"{"curve": "fuzz"}"#);
    harness.write_and_apply(SATURATOR_FILE, &wrong);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].message.contains("curve"),
        "{}",
        problems[0].message
    );
    let id = InstanceId::new("arrangement/piano/heat").unwrap();
    let saturator = harness.project.resolve::<SaturatorState>(&id).unwrap();
    assert_eq!(harness.project.state(&saturator).unwrap().drive_db, 12.0);
}
