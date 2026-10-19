//! What rendering a song costs, written as tools of the project and as the Rust arrangement, on
//! the same content: twelve tracks of a synth and a filter whose cutoff a lane closes, each
//! sending -12 dB into a return with a reverb, 150 clips of 30 to 150 notes, an audio track with
//! one region, a second return that hears before the faders, and the master's limiter. A measurement,
//! not a check. Run with `cargo nextest run -p runtime --test projects --cargo-profile
//! release-fast --run-ignored only render_cost --no-capture`.

use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

use crate::shared_song::song_files;
use crate::support::{BAR, Harness, write};

const TRACKS: usize = 12;
const CLIPS: usize = 150;
const BARS: usize = 16;

/// Four seconds of a voice, in `assets/audio/voice.wav`.
fn write_voice(root: &Path) {
    let path = root.join("assets/audio/voice.wav");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in 0..4 * 48_000 {
        let seconds = f64::from(frame) / 48_000.0;
        let tone = (seconds * 220.0 * std::f64::consts::TAU).sin();
        writer.write_sample((0.3 * tone) as f32).unwrap();
    }
    writer.finalize().unwrap();
}

/// The song of the examples, with a region of the voice on its audio track.
fn typescript_song() -> Harness {
    let folder = tempfile::tempdir().unwrap();
    for (path, text) in song_files(TRACKS, CLIPS) {
        write(folder.path(), &path, &text);
    }
    let region = r#"{"tool": "region", "state": {"asset": "voice.wav", "start": 3840}}"#;
    write(folder.path(), "state/song/voice/take.json", region);
    write_voice(folder.path());
    Harness::open(folder)
}

/// The same tracks, clips, lane and region as the Rust arrangement's tracks and clips.
fn rust_arrangement() -> Harness {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    let project = json!({
        "format": 1,
        "extensions": ["arrangement", "instrument", "filter", "utility", "reverb"],
        "tempo_map": { "time_signature": "4/4", "tempo_changes": [{ "tick": 0, "bpm": 120.0 }] },
        "connections": [],
    });
    write(root, "project.json", &project.to_string());
    write(
        root,
        "state/arrangement/instance.json",
        r#"{"tool": "arrangement", "state": {}}"#,
    );
    let files = song_files(TRACKS, CLIPS);
    let lane = json!([{ "device": "fx", "parameter": "cutoff_hz", "points": [{ "tick": 0, "value": 6000 }, { "tick": 245_760, "value": 1500 }] }]);
    for track in 0..TRACKS {
        let folder = format!("state/arrangement/t{track:02}");
        let record = json!({ "tool": "arrangement.track", "state": { "name": format!("t{track:02}"), "order": track, "effects": ["fx"], "automation": lane, "sends": { "ret-a": -12 } } });
        write(
            root,
            &format!("{folder}/instance.json"),
            &record.to_string(),
        );
        write(
            root,
            &format!("{folder}/instrument.json"),
            r#"{"tool": "instrument.synth", "state": {}}"#,
        );
        write(
            root,
            &format!("{folder}/fx.json"),
            r#"{"tool": "filter", "state": {}}"#,
        );
        let prefix = format!("state/song/t{track:02}/");
        for (path, text) in files.iter().filter(|(path, _)| path.starts_with(&prefix)) {
            let clip: Value = serde_json::from_str(text).unwrap();
            if clip["tool"] != "timeline" {
                continue;
            }
            let record = json!({ "tool": "arrangement.clip", "state": clip["state"] });
            let name = path.rsplit('/').next().unwrap();
            write(root, &format!("{folder}/{name}"), &record.to_string());
        }
    }
    for (order, name, pre_fader, effects) in [
        (TRACKS + 1, "ret-a", false, vec!["verb"]),
        (TRACKS + 2, "ret-b", true, vec![]),
    ] {
        let record = json!({ "tool": "arrangement.track", "state": { "name": name, "kind": "return", "order": order, "pre_fader": pre_fader, "effects": effects } });
        write(
            root,
            &format!("state/arrangement/{name}/instance.json"),
            &record.to_string(),
        );
    }
    write(
        root,
        "state/arrangement/ret-a/verb.json",
        r#"{"tool": "reverb", "state": {"mix": 1}}"#,
    );
    let voice = json!({ "tool": "arrangement.track", "state": { "name": "voice", "kind": "audio", "order": TRACKS } });
    write(
        root,
        "state/arrangement/voice/instance.json",
        &voice.to_string(),
    );
    let clip =
        r#"{"tool": "arrangement.audio_clip", "state": {"asset": "voice.wav", "start": 3840}}"#;
    write(root, "state/arrangement/voice/take.json", clip);
    write_voice(root);
    Harness::open(folder)
}

/// Renders `BARS` bars from the start and gives how many times faster than real time.
fn realtime(harness: &mut Harness) -> f64 {
    assert_eq!(harness.project.problems(), []);
    let frames = BARS * BAR;
    let started = Instant::now();
    let played = harness.play_from_the_start(frames);
    let seconds = started.elapsed().as_secs_f64();
    assert!(played.iter().any(|sample| sample.abs() > 1e-3));
    frames as f64 / 48_000.0 / seconds
}

#[test]
#[ignore = "a measurement"]
fn render_cost() {
    if !sound_typescript::has_bun() {
        eprintln!("skipped: Bun is not installed");
        return;
    }
    let (mut typescript, mut rust) = (typescript_song(), rust_arrangement());
    // Once to warm the files and the code, then measured.
    realtime(&mut typescript);
    realtime(&mut rust);
    let typescript = (0..3)
        .map(|_| realtime(&mut typescript))
        .fold(0.0, f64::max);
    let rust = (0..3).map(|_| realtime(&mut rust)).fold(0.0, f64::max);
    println!(
        "{TRACKS} tracks, {CLIPS} clips, a lane per track and a region, {BARS} bars: tools of the project {typescript:.0}x real time, the Rust arrangement {rust:.0}x"
    );
}

/// Renders the song of tools of the project for as long as `RENDER_COST_SECONDS` says, for a
/// profiler to look at.
#[test]
#[ignore = "a measurement"]
fn render_cost_of_the_song_alone() {
    if !sound_typescript::has_bun() {
        return;
    }
    keep_rendering(&mut typescript_song());
}

/// The same for the Rust arrangement, to compare the two profiles.
#[test]
#[ignore = "a measurement"]
fn render_cost_of_the_arrangement_alone() {
    keep_rendering(&mut rust_arrangement());
}

fn keep_rendering(harness: &mut Harness) {
    let seconds: f64 = std::env::var("RENDER_COST_SECONDS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(10.0);
    let started = Instant::now();
    while started.elapsed().as_secs_f64() < seconds {
        realtime(harness);
    }
}
