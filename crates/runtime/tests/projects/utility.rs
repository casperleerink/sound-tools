//! The built-in utility in a rack after the synth: at its defaults the rack sounds as it
//! does without it, to the bit, and an outside agent changes it by file while the project plays
//! and the change is heard and undone in one step.

use crate::support::{BAR, Harness, clip, synth};

const UTILITY_FILE: &str = "state/rack/trim.json";

/// A rack that plays short chords for four bars, through a utility named `trim` when there is
/// a record for it.
fn piano(utility: Option<&str>) -> Harness {
    let chords = [
        (0, 480, 48),
        (0, 480, 55),
        (1920, 480, 60),
        (3840, 480, 64),
        (5760, 480, 67),
    ];
    let (synth, chords) = (synth(0.15), clip(0, 15360, &chords));
    let mut children = vec![("instrument", synth.as_str()), ("chords", chords.as_str())];
    children.extend(utility.map(|record| ("trim", record)));
    Harness::rack(&children)
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
fn at_its_defaults_the_rack_sounds_as_it_does_without_it() {
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
