//! A generated project of 100 tracks with 100 clips each. Not part of the normal run:
//!
//! ```sh
//! cargo nextest run -p runtime --run-ignored only hundred_tracks --no-capture
//! SCALE_PROJECT=/tmp/scale cargo nextest run -p runtime --run-ignored only hundred_tracks --no-capture
//! ```
//!
//! With `SCALE_PROJECT` the project is left in that folder, to play it on a real device:
//! `cargo run -p runtime -- /tmp/scale`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use runtime::OFFLINE;

use crate::plugin_hosts::open_or_create;
use crate::support::{TRACK, clip, synth, write};

const TRACKS: u64 = 100;
const CLIPS: u64 = 100;
const BAR_TICKS: u64 = 3840;

/// Clip `c` of track `t` is one bar long and sits at bar `8c + t mod 8`. So an eighth of the
/// tracks play in any bar, each a chord of four quarter notes, over 800 bars.
fn generate(root: &Path) {
    for track in 0..TRACKS {
        let folder = format!("state/arrangement/track-{track:03}");
        let record = TRACK
            .replace("NAME", &format!("Track {track}"))
            .replace("ORDER", &track.to_string());
        write(root, &format!("{folder}/instance.json"), &record);
        write(root, &format!("{folder}/instrument.json"), &synth(0.02));
        for index in 0..CLIPS {
            let start = (index * 8 + track % 8) * BAR_TICKS;
            let pitch = 36 + (track % 40) as u8;
            let notes: Vec<(u64, u64, u8)> = (0..4)
                .map(|beat| (beat * 960, 900, pitch + beat as u8 * 3))
                .collect();
            let record = clip(start, BAR_TICKS, &notes);
            write(root, &format!("{folder}/clip-{index:03}.json"), &record);
        }
    }
}

#[test]
#[ignore = "scale numbers, run by hand"]
fn hundred_tracks_of_hundred_clips_open_play_and_take_an_edit() {
    let root = match std::env::var_os("SCALE_PROJECT") {
        Some(path) => PathBuf::from(path),
        None => tempfile::tempdir().unwrap().keep(),
    };
    // The default project first, so the folder has its project.json and its arrangement.
    let (control, _engine) = sound_core::Engine::new(OFFLINE);
    drop(open_or_create(&root, control));
    let started = Instant::now();
    generate(&root);
    println!(
        "generated {} records in {:?} at {}",
        TRACKS * (CLIPS + 2),
        started.elapsed(),
        root.display()
    );

    let started = Instant::now();
    let (control, mut engine) = sound_core::Engine::new(OFFLINE);
    let (mut project, plugins) = open_or_create(&root, control);
    println!("open: {:?}", started.elapsed());
    assert_eq!(project.problems(), []);
    assert_eq!(project.instances().count() as u64, 1 + TRACKS * (CLIPS + 2));

    // Play ten seconds, apply one outside clip edit, play on.
    project.engine().play();
    let seconds = 10;
    let frames = seconds * OFFLINE.sample_rate as usize;
    let started = Instant::now();
    let before = runtime::render(&mut project, &mut engine, &plugins, frames).unwrap();
    let ratio = seconds as f64 / started.elapsed().as_secs_f64();
    println!("offline, {seconds} s of playback: {ratio:.1} times realtime");
    assert!(before.iter().any(|sample| sample.abs() > 0.01));

    // A new part in bars 7 and 8 of one track.
    let part = clip(
        6 * BAR_TICKS,
        2 * BAR_TICKS,
        &[(0, 3840, 84), (3840, 3840, 86)],
    );
    // The canonical root: the paths of the watcher are canonical too.
    let root = project.root().to_path_buf();
    let path = write(&root, "state/arrangement/track-050/agent-part.json", &part);
    let started = Instant::now();
    assert_eq!(project.apply_outside_changes(&[path]).unwrap(), 1);
    println!("one outside clip edit applied in {:?}", started.elapsed());
    let started = Instant::now();
    project.poll().unwrap();
    project.engine().poll().unwrap();
    println!("poll after the edit: {:?}", started.elapsed());

    let started = Instant::now();
    let summary = runtime::summary(&project);
    println!(
        "summary: {} lines in {:?}",
        summary.lines().count(),
        started.elapsed()
    );

    let after = runtime::render(&mut project, &mut engine, &plugins, frames).unwrap();
    assert!(after.iter().any(|sample| sample.abs() > 0.01));
    let status = project.engine().poll().unwrap();
    assert_eq!((status.event_overflows, status.port_misuses), (0, 0));

    let started = Instant::now();
    project.undo().unwrap();
    println!("undo of the edit: {:?}", started.elapsed());
}

/// The resident memory of this process in bytes, as `ps` sees it.
fn resident_bytes() -> u64 {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .unwrap();
    let kilobytes: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap();
    kilobytes * 1024
}

/// 16 audio tracks, each with one clip of its own file of 10 minutes, stereo 24-bit, half at
/// 48 kHz and half at 44.1 kHz so that half of them go through the resampler. Not part of the
/// normal run:
///
/// ```sh
/// cargo nextest run -p runtime --run-ignored only sixteen_audio_tracks --no-capture
/// ```
///
/// With `SCALE_PROJECT` the project is left in that folder, to play it on a real device.
#[test]
#[ignore = "scale numbers, run by hand"]
fn sixteen_audio_tracks_of_ten_minutes_open_play_and_stay_within_the_size_of_their_files() {
    const TRACKS: usize = 16;
    const SECONDS: u32 = 600;
    // Without `SCALE_PROJECT` the 2.6 GB of files go when the test ends.
    let temporary = tempfile::tempdir().unwrap();
    let root = match std::env::var_os("SCALE_PROJECT") {
        Some(path) => PathBuf::from(path),
        None => temporary.path().to_path_buf(),
    };
    let (control, _engine) = sound_core::Engine::new(OFFLINE);
    drop(open_or_create(&root, control));
    let started = Instant::now();
    let mut file_bytes = 0;
    for track in 0..TRACKS {
        let rate = if track % 2 == 0 { 48_000 } else { 44_100 };
        let name = format!("take-{track:02}.wav");
        let path = root.join("assets/audio").join(&name);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: rate,
                bits_per_sample: 24,
                sample_format: hound::SampleFormat::Int,
            };
            let mut writer = hound::WavWriter::create(&path, spec).unwrap();
            let hz = 110.0 * (1.0 + track as f64 / 4.0);
            for frame in 0..SECONDS * rate {
                let value =
                    0.05 * (std::f64::consts::TAU * hz * f64::from(frame) / f64::from(rate)).sin();
                let sample = (value * 8_388_607.0) as i32;
                writer.write_sample(sample).unwrap();
                writer.write_sample(-sample).unwrap();
            }
            writer.finalize().unwrap();
        }
        file_bytes += std::fs::metadata(&path).unwrap().len();
        let folder = format!("state/arrangement/take-{track:02}");
        let record = format!(
            r#"{{"tool": "arrangement.track", "state": {{"name": "Take {track}", "kind": "audio", "order": {}}}}}"#,
            track + 1
        );
        write(&root, &format!("{folder}/instance.json"), &record);
        let record = format!(
            r#"{{"tool": "arrangement.audio_clip", "state": {{"asset": "{name}", "start": 0}}}}"#
        );
        write(&root, &format!("{folder}/take.json"), &record);
    }
    println!(
        "{TRACKS} files, {:.1} MB on disk, ready in {:?} at {}",
        file_bytes as f64 / 1e6,
        started.elapsed(),
        root.display()
    );

    let before = resident_bytes();
    let started = Instant::now();
    let (control, mut engine) = sound_core::Engine::new(OFFLINE);
    let (mut project, plugins) = open_or_create(&root, control);
    let opened = started.elapsed();
    let after = resident_bytes();
    assert_eq!(project.problems(), []);
    let grown = after.saturating_sub(before);
    println!(
        "open: {opened:?}, resident memory grew by {:.1} MB for {:.1} MB of files ({:.3} of their size)",
        grown as f64 / 1e6,
        file_bytes as f64 / 1e6,
        grown as f64 / file_bytes as f64
    );
    // The bound: the files the clips name, each once, as they are on disk, and a little more.
    assert!(
        grown < file_bytes + file_bytes / 20,
        "{grown} for {file_bytes}"
    );

    // Ten seconds at the start and ten at the ninth minute, offline.
    for from in [0, 9 * 60 * 2 * 960] {
        project.engine().seek(sound_core::Ticks(from));
        project.engine().play();
        let seconds = 10;
        let frames = seconds * OFFLINE.sample_rate as usize;
        let started = Instant::now();
        let output = runtime::render(&mut project, &mut engine, &plugins, frames).unwrap();
        let ratio = seconds as f64 / started.elapsed().as_secs_f64();
        println!("offline from tick {from}, {seconds} s of playback: {ratio:.1} times realtime");
        assert!(output.iter().any(|sample| sample.abs() > 0.01));
    }
    println!(
        "resident memory after playing: {:.1} MB",
        resident_bytes() as f64 / 1e6
    );
}
