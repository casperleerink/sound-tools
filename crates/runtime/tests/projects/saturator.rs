//! The built-in saturator in the chain of a real track: an outside agent changes it by file
//! while the project plays and the change is heard, and its oversampling is a latency the
//! project makes up for.

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
