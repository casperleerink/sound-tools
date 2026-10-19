//! What rendering the song of the DAW template costs: twelve tracks of a synth and a filter whose cutoff a lane closes, each
//! sending -12 dB into a return with a reverb, 150 clips of 30 to 150 notes, an audio track with
//! one region, a second return that hears before the faders, and the master's limiter. A measurement,
//! not a check. Run with `cargo nextest run -p runtime --test projects --cargo-profile
//! release-fast --run-ignored only render_cost --no-capture`.

use std::path::Path;
use std::time::Instant;

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
    assert!(
        sound_typescript::has_bun(),
        "these tests run tools in Bun: see tooling/fetch-bun.sh"
    );
    let mut song = typescript_song();
    // Once to warm the files and the code, then measured.
    realtime(&mut song);
    let song = (0..3).map(|_| realtime(&mut song)).fold(0.0, f64::max);
    println!(
        "{TRACKS} tracks, {CLIPS} clips, a lane per track and a region, {BARS} bars: {song:.0}x real time"
    );
}

/// Renders the song of tools of the project for as long as `RENDER_COST_SECONDS` says, for a
/// profiler to look at.
#[test]
#[ignore = "a measurement"]
fn render_cost_of_the_song_alone() {
    keep_rendering(&mut typescript_song());
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
