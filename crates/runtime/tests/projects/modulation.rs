//! The built-in modulation in the chain of a real track: an outside agent changes it by file
//! while the project plays and the change is heard.

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/organ";
const MODULATION_FILE: &str = "state/arrangement/organ/swirl.json";

/// A track that plays long chords for four bars through a modulation named `swirl`.
fn organ_through(modulation: &str) -> Harness {
    let mut harness = Harness::new();
    let chords = [
        (0, 3840, 48),
        (0, 3840, 55),
        (3840, 3840, 60),
        (3840, 3840, 64),
    ];
    harness.write_track("organ", 1, 0.15, &[("chords", clip(0, 15360, &chords))]);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "organ", "order": 1, "effects": ["swirl"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(MODULATION_FILE, modulation),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "modulation", "state": {state}}}"#)
}

fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

#[test]
fn an_outside_edit_of_the_modulation_while_it_plays_is_heard_and_undone_in_one_step() {
    let wet = record(r#"{"mode": "flanger", "feedback": 0.7, "mix": 0.8}"#);
    let dry = record(r#"{"mix": 0.0}"#);

    // What the track plays with no modulation in the mix, from the start: the reference.
    let dry_all = organ_through(&dry).play_from_the_start(2 * BAR);

    // One session: a flanger for half a bar, then an agent writes mix 0 while it plays. The
    // edit applies at the next block of the engine.
    let mut harness = organ_through(&wet);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert!(difference(&played, frames(&dry_all, 0, BAR / 2)).is_some());
    assert_eq!(harness.write_and_apply(MODULATION_FILE, &dry), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms the output is the dry sound to the bit,
    // although the flanger goes on inside.
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
    let file = std::fs::read_to_string(harness.path(MODULATION_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["mode"], "flanger");
    let after_undo = harness.play(BAR / 4);
    assert!(
        difference(
            &after_undo,
            frames(&dry_all, BAR + BAR / 2, BAR + BAR / 2 + BAR / 4)
        )
        .is_some()
    );
}
