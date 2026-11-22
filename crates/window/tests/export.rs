//! Real renders, as the app runs them without a window: the export dialog by `--act`, the save
//! panel answered by `--script`, and a part of the song that its code renders onto a new
//! audio track.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// Runs the app on `song` without a window, in the folder itself: `steps` by `--act` and
/// `script` by `--script`. What `--tree` says then.
fn run(song: &Path, steps: &[Value], script: &[Value]) -> Value {
    let scratch = tempfile::tempdir().unwrap();
    let file = |name: &str, value: &[Value]| {
        let path = scratch.path().join(name);
        std::fs::write(&path, serde_json::to_string(value).unwrap()).unwrap();
        path
    };
    let tree = scratch.path().join("tree.json");
    let ran = Command::new(env!("CARGO_BIN_EXE_sound-tools"))
        .arg(song)
        .args(["--fps", "10", "--tree"])
        .arg(&tree)
        .arg("--act")
        .arg(file("steps.json", steps))
        .arg("--script")
        .arg(file(
            "script.json",
            &[
                script,
                &[json!({ "t": 0.1, "type": "mark", "name": "start" })],
            ]
            .concat(),
        ))
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    serde_json::from_slice(&std::fs::read(tree).unwrap()).unwrap()
}

/// A new project of the window's DAW whose clips end after two seconds.
fn two_seconds(scratch: &Path) -> PathBuf {
    let song = scratch.join("song");
    run(&song, &[], &[]);
    for clip in ["t00/bass.json", "t01/chords.json"] {
        let path = song.join("state/song").join(clip);
        let mut record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let state = &mut record["state"];
        state["length"] = json!(3840);
        let notes = state["notes"].as_array_mut().unwrap();
        notes.retain(|note| note["start"].as_u64().unwrap() < 3840);
        std::fs::write(&path, record.to_string()).unwrap();
    }
    song
}

/// The seconds of `file` and its loudest sample, after it says it is 16-bit at 44.1 kHz.
fn sixteen_bit(file: &Path) -> (f64, f64) {
    let mut reader = hound::WavReader::open(file).unwrap();
    let spec = reader.spec();
    assert_eq!((spec.bits_per_sample, spec.sample_rate), (16, 44_100));
    let seconds = f64::from(reader.duration()) / 44_100.0;
    let samples = reader.samples::<i16>().map(Result::unwrap);
    let peak = samples
        .map(|sample| f64::from(sample.unsigned_abs()))
        .fold(0.0, f64::max);
    (seconds, peak / f64::from(i16::MAX))
}

fn labels(tree: &Value, start: &str) -> Vec<String> {
    let controls = tree["controls"].as_array().unwrap().iter();
    controls
        .filter(|control| control["key"].as_str().unwrap().starts_with(start))
        .map(|control| control["label"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_piece_exports_through_the_dialog_and_the_save_panel() {
    let scratch = tempfile::tempdir().unwrap();
    let song = two_seconds(scratch.path());
    let file = scratch.path().join("song.wav");
    let steps = [
        json!({ "action": "app.export" }),
        json!({ "label": "16-bit", "action": "press" }),
        json!({ "label": "44.1 kHz", "action": "press" }),
        json!({ "label": "Export…", "action": "press" }),
    ];
    let answer = json!({ "t": 0.2, "type": "file-dialog", "path": file });
    let tree = run(&song, &steps, &[answer]);
    assert_eq!(tree["notice"], Value::Null);
    assert_eq!(
        labels(&tree, "exported-").len(),
        2,
        "the dialog that says it is done"
    );
    let (seconds, peak) = sixteen_bit(&file);
    assert!(
        (2.0..12.0).contains(&seconds),
        "the piece and its tail: {seconds}"
    );
    assert!(peak > 0.01, "silent: {peak}");
}

#[test]
fn a_track_renders_onto_a_new_audio_track_in_one_undo_step() {
    let scratch = tempfile::tempdir().unwrap();
    let song = two_seconds(scratch.path());
    // The song's menus are the view's: a button here asks what its menu would.
    let view = song.join("extensions/song.tsx");
    let code = std::fs::read_to_string(&view).unwrap().replacen(
        "band={<Ruler page={page} />}",
        r#"band={<col w="full"><Ruler page={page} /><box key="render" label="Render Bass" w={20} h={10} onPress={() => void page.edits.tracks.render("t00", ["t00/bass"], false)} /></col>}"#,
        1,
    );
    std::fs::write(&view, code).unwrap();
    let press = json!({ "label": "Render Bass", "action": "press" });
    let tree = run(&song, &[press], &[]);
    assert_eq!(tree["notice"], Value::Null);
    let region: Value = serde_json::from_slice(
        &std::fs::read(song.join("state/song/audio-1/render.json")).unwrap(),
    )
    .unwrap();
    let asset = region["state"]["asset"].as_str().unwrap();
    let mut reader = hound::WavReader::open(song.join("assets/audio").join(asset)).unwrap();
    let seconds = f64::from(reader.duration()) / f64::from(reader.spec().sample_rate);
    assert!(
        (seconds - 2.0).abs() < 0.05,
        "two seconds, no tail: {seconds}"
    );
    assert!(
        reader
            .samples::<f32>()
            .map(Result::unwrap)
            .any(|sample| sample.abs() > 0.01)
    );

    run(&song, &[json!({ "action": "undo" })], &[]);
    assert!(!song.join("state/song/audio-1").exists());
    let mix = std::fs::read_to_string(song.join("state/song/t00/mix.json")).unwrap();
    assert!(!mix.contains("mute"), "{mix}");
}
