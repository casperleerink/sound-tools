//! The built-in delay in the chain of a real track: an outside agent changes it by file while
//! the project plays and the change is heard, a synced delay follows the tempo of the project,
//! it comes back as it was after close and reopen, and a render is the same every time, to the
//! byte.

use delay::{DelayState, Division, Feel};
use sound_core::{Changes, InstanceId, Tempo, TempoMap, TimeSignature};

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const DELAY_FILE: &str = "state/arrangement/piano/echo.json";

/// A track that plays short chords for four bars through a delay named `echo`.
fn piano_through(delay: &str) -> Harness {
    let mut harness = Harness::new();
    let chords = [
        (0, 240, 48),
        (0, 240, 55),
        (1920, 240, 60),
        (3840, 240, 64),
        (5760, 240, 67),
    ];
    harness.write_track("piano", 1, 0.15, &[("chords", clip(0, 15360, &chords))]);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["echo"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(DELAY_FILE, delay),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "delay", "state": {state}}}"#)
}

fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

#[test]
fn an_outside_edit_of_the_delay_while_it_plays_is_heard_and_undone_in_one_step() {
    let wet = record(r#"{"feedback": 0.8, "mix": 0.8}"#);
    let dry = record(r#"{"mix": 0.0}"#);

    // What the track plays with no repeats in the mix, from the start: the reference.
    let dry_all = piano_through(&dry).play_from_the_start(2 * BAR);

    // One session: loud repeats for half a bar, then an agent writes mix 0 while it plays.
    // The edit applies at the next block of the engine.
    let mut harness = piano_through(&wet);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert!(difference(&played, frames(&dry_all, 0, BAR / 2)).is_some());
    assert_eq!(harness.write_and_apply(DELAY_FILE, &dry), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms the repeats are gone from the output,
    // which is the dry sound to the bit, although they go on inside the delay.
    let glided = BAR / 2 + 1_024;
    let (heard, expected) = (
        frames(&played, glided, BAR + BAR / 2),
        frames(&dry_all, glided, BAR + BAR / 2),
    );
    assert_eq!(heard, expected);

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(DELAY_FILE)).unwrap();
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

/// The first frame that is not silent, in the left channel.
fn first_sound(samples: &[f32]) -> usize {
    let first = samples.iter().step_by(2).position(|sample| *sample != 0.0);
    first.unwrap()
}

/// A quarter note of delay, with only the repeat in the mix: at 120 bpm the chord comes back
/// after half a second, and after the project goes to 60 bpm, after a whole one.
#[test]
fn a_synced_delay_follows_the_tempo_of_the_project() {
    let quarter = record(
        r#"{"division": "1/4", "feedback": 0.0, "mix": 1.0, "low_cut_hz": 20.0, "high_cut_hz": 20000.0}"#,
    );
    let at_120 = first_sound(&piano_through(&quarter).play_from_the_start(BAR));

    let mut harness = piano_through(&quarter);
    let mut changes = Changes::new();
    let four_four = TimeSignature::new(4, 4).unwrap();
    let slower = TempoMap::constant(four_four, Tempo::from_bpm(60.0).unwrap());
    changes.set_tempo_map(slower);
    harness.project.commit("Change tempo", changes).unwrap();
    let at_60 = first_sound(&harness.play_from_the_start(BAR));

    // The chord starts at frame 0 at both tempos; its repeat is a quarter later.
    assert!((24_000..24_100).contains(&at_120), "{at_120}");
    assert_eq!(at_60 - at_120, 24_000);
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte. A render is the same every time.
#[test]
fn the_delay_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = piano_through(&record("{}"));
    let id = InstanceId::new("arrangement/piano/echo").unwrap();
    let delay = harness.project.resolve::<DelayState>(&id).unwrap();
    let sound = DelayState {
        sync: true,
        division: Division::Sixteenth,
        feel: Feel::Dotted,
        time_ms: 180.0,
        feedback: 0.55,
        ping_pong: true,
        low_cut_hz: 150.0,
        high_cut_hz: 6_000.0,
        mix: 0.45,
    };
    let mut changes = Changes::new();
    changes.set(&delay, sound);
    harness.project.commit("Change delay", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(DELAY_FILE)).unwrap();
    // The whole record, in the bytes the agent doc shows.
    assert_eq!(
        file,
        r#"{
  "tool": "delay",
  "state": {
    "sync": true,
    "division": "1/16",
    "feel": "dotted",
    "time_ms": 180.0,
    "feedback": 0.55,
    "ping_pong": true,
    "low_cut_hz": 150.0,
    "high_cut_hz": 6000.0,
    "mix": 0.45
  }
}
"#
    );

    // Reopened twice: a render of each first session is the same, to the byte.
    let mut harness = harness.reopen();
    let first = harness.play(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let delay = harness.project.resolve::<DelayState>(&id).unwrap();
    assert_eq!(harness.project.state(&delay), Some(&sound));
    assert_eq!(
        std::fs::read_to_string(harness.path(DELAY_FILE)).unwrap(),
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
fn a_delay_record_out_of_range_is_reported_and_the_track_keeps_what_it_had() {
    let mut harness = piano_through(&record(r#"{"feedback": 0.6}"#));
    let wrong = record(r#"{"feedback": 1.5}"#);
    assert_eq!(harness.write_and_apply(DELAY_FILE, &wrong), 0);
    let problems = harness.project.problems();
    assert_eq!(
        problems[0].message,
        "state: feedback must be from 0 to 0.95, not 1.5"
    );
    let wrong = record(r#"{"division": "1/3"}"#);
    harness.write_and_apply(DELAY_FILE, &wrong);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1);
    assert!(
        problems[0].message.contains("1/3"),
        "{}",
        problems[0].message
    );
    let id = InstanceId::new("arrangement/piano/echo").unwrap();
    let delay = harness.project.resolve::<DelayState>(&id).unwrap();
    assert_eq!(harness.project.state(&delay).unwrap().feedback, 0.6);
}
