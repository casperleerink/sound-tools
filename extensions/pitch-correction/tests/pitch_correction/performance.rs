//! How much faster than realtime many pitch corrections render.

use std::time::Instant;

use pitch_correction::PitchCorrectionState;

use crate::support::{Rig, SAMPLE_RATE, voice};

const CORRECTIONS: usize = 100;
const SECONDS: usize = 10;

/// `CORRECTIONS` rigs of a voice into a correction, rendered one after another, as the engine
/// would run them side by side in one block.
fn render(label: &str, state: PitchCorrectionState, sings: bool) {
    let mut rigs: Vec<Rig> = (0..CORRECTIONS)
        .map(|index| {
            // A glide and a vibrato, a little different on each track.
            let start = 50.0 + (index % 12) as f64;
            let line = move |time: f64| {
                let vibrato = 0.3 * (std::f64::consts::TAU * 5.5 * time).sin();
                sings.then_some(start + 0.5 * (time % 2.0) + vibrato)
            };
            Rig::new(state, voice(line))
        })
        .collect();
    let block = 512;
    let started = Instant::now();
    for _ in 0..SECONDS * SAMPLE_RATE as usize / block {
        for rig in &mut rigs {
            rig.render_in(block, block);
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let ratio = SECONDS as f64 / elapsed;
    println!("100 corrections, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime");
}

/// Run with
/// `cargo nextest run -p pitch-correction --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_pitch_corrections() {
    let state = PitchCorrectionState::default();
    render("singing", state, true);
    let formants = PitchCorrectionState {
        keep_formants: true,
        ..state
    };
    render("singing, formants kept", formants, true);
    render("idle", state, false);
}
