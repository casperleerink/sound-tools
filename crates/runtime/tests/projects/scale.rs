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
    drop(runtime::open_or_create(&root, control).unwrap());
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
    let (mut project, plugins) = runtime::open_or_create(&root, control).unwrap();
    println!("open: {:?}", started.elapsed());
    assert_eq!(project.problems(), []);
    assert_eq!(project.instances().count() as u64, 3 + TRACKS * (CLIPS + 2));

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
