//! How much faster than realtime an ordinary large project renders.

use std::time::Instant;

use instrument::{SynthState, Waveform};

use crate::support::{Harness, SAMPLE_RATE, note, peak};

const TRACKS: usize = 100;
const SECONDS: usize = 10;

fn render_and_report(label: &str, harness: &mut Harness) -> Vec<f32> {
    let started = Instant::now();
    let output = harness.render(SECONDS * SAMPLE_RATE as usize);
    let elapsed = started.elapsed().as_secs_f64();
    let ratio = SECONDS as f64 / elapsed;
    println!("{label}: {SECONDS} s of audio in {elapsed:.3} s, {ratio:.1} times realtime");
    output
}

/// Run with
/// `cargo nextest run -p instrument --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratios"]
fn realtime_ratio_of_one_hundred_synths() {
    for waveform in [Waveform::Saw, Waveform::Square] {
        let mut harness = Harness::new();
        for track in 0..TRACKS {
            // A chord of four notes per track, held for the whole render.
            let root = 36 + (track % 36) as u8;
            let chord = [0, 4, 7, 11].map(|interval| note(0, 96_000, root + interval, 100));
            let quiet = SynthState {
                waveform,
                gain: 0.002,
                ..SynthState::default()
            };
            harness.add_track(&format!("track-{track:03}"), chord.to_vec(), quiet);
        }

        // The transport is stopped, so no note has started: every synth is idle.
        let idle = render_and_report(&format!("100 {waveform:?} synths, idle"), &mut harness);
        assert_eq!(peak(&idle), 0.0);
        harness.project.engine().play();
        let label = format!("100 {waveform:?} synths, 4 voices each");
        let playing = render_and_report(&label, &mut harness);
        assert!(peak(&playing) > 0.01);
    }
}
