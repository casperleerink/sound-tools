//! The Drum pad on a real track of a new project: the default kit sounds with no sample file,
//! an outside agent changes a pad by file while the project plays and the change is heard, it
//! comes back as it was after close and reopen, and a missing sample is a problem and nothing
//! else.

use drum_pad::{DrumPadState, Source};
use sound_core::{Changes, InstanceId};
use sound_media::AudioAsset;

use crate::support::{BAR, Harness, difference};

const FOLDER: &str = "state/arrangement/beat";
const DRUMS_FILE: &str = "state/arrangement/beat/instrument.json";

/// One bar at 120 bpm: kick on 1 and 3, snare on 2 and 4, a closed hat on every eighth.
fn groove() -> String {
    let mut notes = Vec::new();
    for eighth in 0..8_u64 {
        notes.push((eighth * 480, 42, if eighth % 2 == 0 { 100 } else { 72 }));
    }
    notes.extend([
        (0, 36, 120),
        (1920, 36, 110),
        (960, 38, 112),
        (2880, 38, 112),
    ]);
    notes.sort();
    let notes: Vec<String> = notes
        .iter()
        .map(|(start, pitch, velocity)| {
            format!(
                r#"{{"start": {start}, "length": 240, "pitch": {pitch}, "velocity": {velocity}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"tool": "arrangement.clip", "state": {{"start": 0, "length": 3840, "notes": [{}]}}}}"#,
        notes.join(", ")
    )
}

/// The default project with a track `beat` that plays the groove, bar after bar, on a Drum
/// pad written as `drums`.
fn beat(drums: &str) -> Harness {
    let mut harness = Harness::new();
    let track = r#"{"tool": "arrangement.track", "state": {"name": "Beat", "order": 1}}"#;
    let mut paths = vec![
        harness.write(&format!("{FOLDER}/instance.json"), track),
        harness.write(DRUMS_FILE, drums),
    ];
    for bar in 0..4 {
        let clip = groove().replace(
            r#""start": 0, "length": 3840"#,
            &format!(r#""start": {}, "length": 3840"#, bar * 3840),
        );
        paths.push(harness.write(&format!("{FOLDER}/bar-{}.json", bar + 1), &clip));
    }
    assert_eq!(harness.apply(&paths), 6);
    harness
}

fn record(state: &str) -> String {
    format!(r#"{{"tool": "drum-pad", "state": {state}}}"#)
}

fn frames(samples: &[f32], from: usize, to: usize) -> &[f32] {
    &samples[2 * from..2 * to]
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

#[test]
fn the_default_kit_sounds_in_a_new_project_with_no_sample_files() {
    let mut harness = beat(&record("{}"));
    assert_eq!(harness.project.problems(), []);
    assert!(!harness.path("assets/audio").exists());
    let played = harness.play_from_the_start(BAR);
    // Every eighth has a hit, and every hit is heard where it is: each eighth of the bar is
    // louder in its first 10 ms than just before it.
    let eighth = BAR / 8;
    for step in 0..8 {
        let at = step * eighth;
        let hit = peak(frames(&played, at, at + 480));
        println!("eighth {}: peak {hit:.3}", step + 1);
        assert!(hit > 0.02, "eighth {}: {hit}", step + 1);
    }
    let whole = peak(&played);
    println!("the bar peaks at {whole:.3}");
    assert!(whole > 0.2 && whole < 0.9, "{whole}");
}

#[test]
fn an_outside_edit_of_a_pad_while_it_plays_is_heard_and_undone_in_one_step() {
    let loud = record("{}");
    let quiet = record(r#"{"pads": {"42": {"volume_db": -12.0}}}"#);
    let reference = |drums: &str| beat(drums).play_from_the_start(2 * BAR);
    let (loud_all, quiet_all) = (reference(&loud), reference(&quiet));
    assert!(difference(&loud_all, &quiet_all).is_some());

    // One session: the loud hat for half a bar, then an agent turns the hat down while it
    // plays. The edit applies at the next block of the engine.
    let mut harness = beat(&loud);
    let mut played = harness.play_from_the_start(BAR / 2);
    assert_eq!(played, frames(&loud_all, 0, BAR / 2));
    assert_eq!(harness.write_and_apply(DRUMS_FILE, &quiet), 1);
    assert_eq!(harness.project.problems(), []);
    played.extend(harness.play(BAR));

    // Heard: once the hat that sounded at the edit has rung out, the beat is the quiet one to
    // the sample.
    let settled = BAR / 2 + 12_000;
    let heard = frames(&played, settled, BAR + BAR / 2);
    let expected = frames(&quiet_all, settled, BAR + BAR / 2);
    let largest = heard
        .iter()
        .zip(expected)
        .map(|(heard, expected)| (heard - expected).abs())
        .fold(0.0, f32::max);
    assert!(largest < 1e-6, "{largest}");
    assert!(difference(heard, frames(&loud_all, settled, BAR + BAR / 2)).is_some());
    // And no step at the edit: the hat that sounded glides down over 20 ms.
    let edit = frames(&played, BAR / 2 - 480, BAR / 2 + 1_440);
    let steps = edit.chunks(2).collect::<Vec<_>>();
    let largest_step = steps
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).abs())
        .fold(0.0, f32::max);
    let before = frames(&loud_all, BAR / 2 - 480, BAR / 2 + 1_440)
        .chunks(2)
        .collect::<Vec<_>>();
    let largest_before = before
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).abs())
        .fold(0.0, f32::max);
    assert!(
        largest_step <= largest_before + 1e-6,
        "{largest_step} {largest_before}"
    );

    // One undo takes the agent's edit back, file and sound.
    assert!(harness.project.undo().unwrap().is_some());
    let file = std::fs::read_to_string(harness.path(DRUMS_FILE)).unwrap();
    assert_eq!(
        file,
        "{\n  \"tool\": \"drum-pad\",\n  \"state\": {\"pads\": {}}\n}\n"
    );
}

/// Everything of the record survives close and reopen, and the render after it is the render
/// before it, to the byte.
#[test]
fn the_drum_pad_comes_back_after_close_and_reopen_and_renders_the_same() {
    let mut harness = beat(&record("{}"));
    let id = InstanceId::new("arrangement/beat/instrument").unwrap();
    let drums = harness.project.resolve::<DrumPadState>(&id).unwrap();
    let mut kit = DrumPadState::default();
    kit.pads[0].pitch_semitones = -3.0;
    kit.pads[0].decay_ms = 900.0;
    kit.pads[2].volume_db = -2.5;
    kit.pads[6].pan = -0.3;
    kit.pads[10].choke = false;
    kit.pads[3].source = Source::Sound(drum_pad::Sound::Rim);
    let mut changes = Changes::new();
    changes.set(&drums, kit.clone());
    harness.project.commit("Change drums", changes).unwrap();
    let file = std::fs::read_to_string(harness.path(DRUMS_FILE)).unwrap();
    assert!(file.contains("\"36\": {\n        \"sound\": \"kick\",\n        \"volume_db\": 0.0,\n        \"pitch_semitones\": -3.0,"), "{file}");

    let mut harness = harness.reopen();
    let first = harness.play_from_the_start(2 * BAR);
    let mut harness = harness.reopen();
    assert_eq!(harness.project.problems(), []);
    let drums = harness.project.resolve::<DrumPadState>(&id).unwrap();
    assert_eq!(harness.project.state(&drums), Some(&kit));
    assert_eq!(
        std::fs::read_to_string(harness.path(DRUMS_FILE)).unwrap(),
        file
    );
    let second = harness.play_from_the_start(2 * BAR);
    let bytes = |samples: &[f32]| -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    };
    assert_eq!(bytes(&first), bytes(&second));
    assert!(peak(&first) > 0.1);
}

#[test]
fn a_missing_sample_is_in_problems_txt_and_the_rest_plays() {
    let with_file = record(r#"{"pads": {"42": {"sample": "gone.wav"}}}"#);
    let mut harness = beat(&with_file);
    let problems = harness.project.problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, DRUMS_FILE);
    assert_eq!(
        problems[0].message,
        "pad \"42\" (gone) is silent: assets/audio/gone.wav is not there. The rest of the pads play"
    );
    harness.project.poll().unwrap();
    let text = std::fs::read_to_string(harness.path("problems.txt")).unwrap();
    assert!(text.contains("pad \"42\" (gone) is silent"), "{text}");
    // The kick and snare play as they do with no hat at all.
    let played = harness.play_from_the_start(BAR);
    let mut without_hat = beat(&record(r#"{"pads": {"42": {"volume_db": -48.0}}}"#));
    let quiet_hat = without_hat.play_from_the_start(BAR);
    assert!(peak(&played) > 0.1);
    let largest = played
        .iter()
        .zip(&quiet_hat)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    // The hat at -48 dB is under 0.0006.
    assert!(largest < 0.001, "{largest}");
    // The sample is named by a valid asset, and the record keeps it.
    let id = InstanceId::new("arrangement/beat/instrument").unwrap();
    let drums = harness.project.resolve::<DrumPadState>(&id).unwrap();
    let state = harness.project.state(&drums).unwrap();
    assert_eq!(
        state.pads[6].source,
        Source::Sample(AudioAsset::new("gone.wav").unwrap())
    );
}
