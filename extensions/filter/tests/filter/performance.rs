//! How much faster than realtime many filters render.

use std::time::Instant;

use filter::{Filter, FilterState, Slope};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, noise};

const FILTERS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p filter --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_filters() {
    for (label, lfo) in [("still", 0.0), ("with the LFO moving", 2.0)] {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        for index in 0..FILTERS {
            let source = edit
                .add_processor(&format!("source-{index}"), Source::new(noise(0.01)))
                .unwrap();
            let state = FilterState {
                slope: Slope::TwentyFour,
                resonance: 0.5,
                lfo_depth_octaves: lfo,
                ..FilterState::default()
            };
            let filter = edit
                .add_processor(&format!("filter-{index}"), Filter::new(state))
                .unwrap();
            edit.connect(Connection::new(
                source.id(),
                Source::OUTPUT,
                filter.id(),
                Filter::INPUT,
            ))
            .unwrap();
            edit.connect(Connection::to_device(filter.id(), Filter::OUTPUT, 0))
                .unwrap();
        }
        edit.commit().unwrap();
        let mut output = vec![0.0; SECONDS * SAMPLE_RATE as usize * 2];
        let started = Instant::now();
        for buffer in output.chunks_mut(512 * 2) {
            engine.process_block(buffer);
        }
        let elapsed = started.elapsed().as_secs_f64();
        let ratio = SECONDS as f64 / elapsed;
        println!(
            "100 filters at 24 dB, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
