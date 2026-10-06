//! The built-in compressor in the chain of a real track: an outside agent changes it by file
//! while the project plays and the change is heard, and its lookahead is a latency the project
//! makes up for.

use crate::support::{BAR, Harness, clip, difference};

const FOLDER: &str = "state/arrangement/piano";
const COMPRESSOR_FILE: &str = "state/arrangement/piano/glue.json";

/// A track that plays a chord for four bars through a compressor named `glue`.
fn piano_through(compressor: &str) -> Harness {
    let mut harness = Harness::new();
    let chord = [(0, 15360, 48), (0, 15360, 55), (0, 15360, 64)];
    harness.write_track("piano", 1, 0.15, &[("chord", clip(0, 15360, &chord))]);
    let track = r#"{"tool": "arrangement.track", "state": {"name": "piano", "order": 1, "effects": ["glue"]}}"#;
    let paths = [
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(COMPRESSOR_FILE, compressor),
    ];
    assert_eq!(harness.apply(&paths), 2);
    assert_eq!(harness.project.problems(), []);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "compressor", "state": {state}}}"#)
}

/// Frames `from` up to `to` of a stereo render: the piano, the one track.
fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

#[test]
fn an_outside_edit_of_the_compressor_while_it_plays_is_heard_and_undone_in_one_step() {
    let gentle = record(r#"{"threshold_db": -6.0, "attack_ms": 1.0, "release_ms": 10.0}"#);
    let heavy =
        record(r#"{"threshold_db": -40.0, "ratio": 8.0, "attack_ms": 1.0, "release_ms": 10.0}"#);

    // What the track plays with each compressor from the start: the references.
    let reference = |compressor: &str| {
        let mut harness = piano_through(compressor);
        harness.play_from_the_start(2 * BAR)
    };
    let (gentle_all, heavy_all) = (reference(&gentle), reference(&heavy));
    assert!(difference(&gentle_all, &heavy_all).is_some());

    // One session: the gentle one for half a bar, then an agent writes the heavy one while it
    // plays. The edit applies at the next block of the engine.
    let mut harness = piano_through(&gentle);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert_eq!(played, frames(&gentle_all, 0, BAR / 2));
    assert_eq!(harness.write_and_apply(COMPRESSOR_FILE, &heavy), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // The change is heard: after its glide of 20 ms, the hold of 10 ms and many release times,
    // the track sounds as if the heavy compressor had been there all along. 250 ms is plenty.
    let settled = BAR / 2 + 12_000;
    let (heard, expected) = (
        frames(&played, settled, BAR + BAR / 2),
        frames(&heavy_all, settled, BAR + BAR / 2),
    );
    let largest = heard
        .iter()
        .zip(expected)
        .map(|(heard, expected)| (heard - expected).abs())
        .fold(0.0, f32::max);
    assert!(largest < 1e-5, "{largest}");
    // And it is not what the gentle one plays.
    assert!(difference(heard, frames(&gentle_all, settled, BAR + BAR / 2)).is_some());

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(COMPRESSOR_FILE)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(state["state"]["threshold_db"], -6.0);
}

/// A compressor that does nothing but look ahead delays its track by 10 ms, and the project
/// makes up for it: the render is the render without the lookahead, sample for sample, and the
/// project reports the latency.
#[test]
fn the_lookahead_is_a_latency_the_project_makes_up_for() {
    let untouched = |lookahead: u8| {
        record(&format!(
            r#"{{"ratio": 1.0, "threshold_db": 0.0, "lookahead_ms": {lookahead}}}"#
        ))
    };
    let mut ahead = piano_through(&untouched(10));
    let mut plain = piano_through(&untouched(0));
    let heard = ahead.play(BAR);
    assert_eq!(ahead.project.engine().poll().unwrap().latency, 480);
    assert_eq!(heard, plain.play(BAR));
    assert!(heard.iter().any(|sample| sample.abs() > 0.01));
}

/// An agent turns the lookahead on while the track plays: the project learns the new latency
/// at once, the sound fades to the delayed one without a jump, and from the next note on the
/// track plays as if the lookahead had been there from the start.
#[test]
fn a_lookahead_turned_on_while_it_plays_is_in_time_without_a_jump() {
    let with = |lookahead: u8| {
        record(&format!(
            r#"{{"threshold_db": -40.0, "attack_ms": 1.0, "release_ms": 10.0, "lookahead_ms": {lookahead}}}"#
        ))
    };
    // Short notes on every beat, so there is a new one after the change.
    let notes: Vec<(u64, u64, u8)> = (0..16).map(|beat| (beat * 960, 240, 60)).collect();
    let track = |compressor: &str| {
        let mut harness = piano_through(compressor);
        harness.write_and_apply(&format!("{FOLDER}/chord.json"), &clip(0, 15360, &notes));
        harness
    };
    let reference = track(&with(10)).play_from_the_start(2 * BAR);

    // The change comes between two beats, while the last note still sounds.
    let switch = 3 * BAR / 8;
    let mut harness = track(&with(0));
    let mut played = harness.play_from_the_start(switch);
    assert_eq!(harness.project.engine().poll().unwrap().latency, 0);
    assert_eq!(harness.write_and_apply(COMPRESSOR_FILE, &with(10)), 1);
    played.extend(harness.play(BAR));
    assert_eq!(harness.project.engine().poll().unwrap().latency, 480);

    // No jump: around the change no step from one sample to the next is larger than the
    // largest of the beat before it.
    let steps = |from: usize, to: usize| {
        frames(&played, from, to)
            .chunks(2)
            .map(|frame| frame[0])
            .collect::<Vec<f32>>()
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max)
    };
    let (before, around) = (
        steps(switch - BAR / 4, switch),
        steps(switch - 1, switch + 2_400),
    );
    println!("largest step around the change {around}, in the beat before {before}");
    assert!(around <= before, "{around} over {before}");

    // From 50 ms after the next beat on, the render of 10 ms from the start.
    let next_beat = BAR / 2;
    let settled = next_beat + 2_400;
    let (heard, expected) = (
        frames(&played, settled, switch + BAR),
        frames(&reference, settled, switch + BAR),
    );
    let largest = heard
        .iter()
        .zip(expected)
        .map(|(heard, expected)| (heard - expected).abs())
        .fold(0.0, f32::max);
    assert!(largest < 1e-5, "{largest}");
    assert!(heard.iter().any(|sample| sample.abs() > 0.01));
}

/// A kick on another track keys the compressor of the bass: the bass is turned down while the
/// kick plays and comes back after it. The kick is muted, so the render is the bass alone: a
/// muted track still keys from `post_fx`.
#[test]
fn a_kick_on_another_track_ducks_the_bass_while_it_plays() {
    let mut harness = Harness::new();
    let beat = BAR / 4;
    harness.write_track("kick", 1, 0.3, &[("hit", clip(960, 960, &[(0, 960, 36)]))]);
    harness.write_track(
        "bass",
        2,
        0.15,
        &[("line", clip(0, 3840, &[(0, 3840, 40)]))],
    );
    let duck = r#"{"threshold_db": -40.0, "ratio": 8.0, "attack_ms": 1.0, "release_ms": 20.0}"#;
    let paths = [
        harness.write(
            "state/arrangement/kick/instance.json",
            r#"{"tool": "arrangement.track", "state": {"name": "kick", "order": 1, "mute": true}}"#,
        ),
        harness.write("state/arrangement/bass/duck.json", &record(duck)),
        harness.write(
            "state/arrangement/bass/instance.json",
            r#"{"tool": "arrangement.track", "state": {"name": "bass", "order": 2, "effects": [{"name": "duck", "sidechain": {"track": "kick", "tap": "post_fx"}}]}}"#,
        ),
    ];
    assert_eq!(harness.apply(&paths), 3);
    assert_eq!(harness.project.problems(), []);
    let played = harness.play_from_the_start(4 * beat);
    // The second half of each beat, after the attack and the release.
    let level = |index: usize| {
        let beat = frames(&played, index * beat + beat / 2, (index + 1) * beat);
        beat.iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
    };
    let (before, during, after) = (level(0), level(1), level(3));
    assert!(during < before / 4.0, "{during} against {before}");
    assert!(after > before * 0.9, "{after} against {before}");
}
