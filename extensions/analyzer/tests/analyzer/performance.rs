//! How much faster than realtime many analyzers render.

use std::time::Instant;

use crate::support::{SAMPLE_RATE, noise, render, rig};

const ANALYZERS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p analyzer --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratios"]
fn realtime_ratio_of_one_hundred_analyzers() {
    let frames = SECONDS * SAMPLE_RATE as usize;
    for (label, amplitude) in [("idle", 0.0), ("on noise", 0.01)] {
        let (mut engine, _scopes) = rig(ANALYZERS, |index| noise(amplitude, index as u64, frames));
        let started = Instant::now();
        render(&mut engine, frames);
        let elapsed = started.elapsed().as_secs_f64();
        let ratio = SECONDS as f64 / elapsed;
        println!(
            "100 analyzers, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
