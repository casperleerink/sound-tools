//! How much faster than realtime a large project of Drum pads renders, and how long the kit
//! takes to make.

use std::time::Instant;

use drum_pad::DrumPadState;

use crate::support::{Harness, SAMPLE_RATE, peak};

const TRACKS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p drum-pad --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratios"]
fn realtime_ratio_of_one_hundred_drum_pads_playing_a_beat() {
    let started = Instant::now();
    let mut harness = Harness::new();
    // Each track its own tuning, so no two make the same sounds and every kit is made anew.
    let mut first = DrumPadState::default();
    first.pads[0].pitch_semitones = 0.5;
    harness.add_track("track-000", crate::listen::beat(), first);
    println!(
        "the kit made in {:.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    for track in 1..TRACKS {
        let mut drums = DrumPadState::default();
        drums.pads[0].pitch_semitones = track as f32 * 0.01;
        harness.add_track(&format!("track-{track:03}"), crate::listen::beat(), drums);
    }
    let idle_started = Instant::now();
    let idle = harness.render(SECONDS * SAMPLE_RATE as usize);
    let ratio = SECONDS as f64 / idle_started.elapsed().as_secs_f64();
    println!("100 Drum pads, idle: {ratio:.0} times realtime");
    assert_eq!(peak(&idle.left), 0.0);
    harness.project.engine().play();
    let playing_started = Instant::now();
    let playing = harness.render(SECONDS * SAMPLE_RATE as usize);
    let ratio = SECONDS as f64 / playing_started.elapsed().as_secs_f64();
    println!("100 Drum pads, each playing the beat: {ratio:.1} times realtime");
    assert!(peak(&playing.left) > 0.01);
}
