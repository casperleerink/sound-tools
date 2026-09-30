//! How much faster than realtime a project of many synths renders.

use std::time::Instant;

use wavetable::WavetableState;
use wavetable::state::Unison;

use crate::support::{Harness, SAMPLE_RATE, Track, note, peak};

const SECONDS: usize = 10;

fn render_and_report(label: &str, harness: &mut Harness) -> Vec<f32> {
    let started = Instant::now();
    let [left, _] = harness.render(SECONDS * SAMPLE_RATE as usize);
    let elapsed = started.elapsed().as_secs_f64();
    let ratio = SECONDS as f64 / elapsed;
    println!("{label}: {SECONDS} s of audio in {elapsed:.3} s, {ratio:.1} times realtime");
    left
}

/// Run with
/// `cargo test -p wavetable --test wavetable realtime_ratio -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratios"]
fn realtime_ratio_of_many_synths() {
    let unison = WavetableState {
        unison: Unison {
            voices: 8,
            amount: 0.3,
        },
        ..WavetableState::default()
    };
    for (name, tracks, state) in [
        ("default patch", 20, WavetableState::default()),
        ("unison 8", 5, unison),
    ] {
        let mut harness = Harness::new();
        for track in 0..tracks {
            // A chord of four notes per track, held for the whole render.
            let root = 36 + (track % 36) as u8;
            let chord = [0, 4, 7, 11].map(|interval| note(0, 96_000, root + interval, 100));
            let quiet = WavetableState {
                gain: 0.01,
                ..state.clone()
            };
            let track_record = Track {
                notes: chord.to_vec(),
                ..Track::default()
            };
            harness.add(&format!("track-{track:03}"), track_record, quiet);
        }
        let idle = render_and_report(&format!("{tracks} synths, {name}, idle"), &mut harness);
        assert_eq!(peak(&idle), 0.0);
        harness.project.engine().play();
        let label = format!("{tracks} synths, {name}, 4 voices each");
        let playing = render_and_report(&label, &mut harness);
        assert!(peak(&playing) > 0.001);
    }
}
