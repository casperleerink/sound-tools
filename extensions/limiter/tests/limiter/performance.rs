//! How much faster than realtime many limiters render.

use std::time::Instant;

use limiter::{Limiter, LimiterState, Meters};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, noise};

const LIMITERS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p limiter --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_limiters() {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    for index in 0..LIMITERS {
        let source = edit
            .add_processor(
                &format!("source-{index}"),
                Source::new(noise(0.5, index as u64)),
            )
            .unwrap();
        // Limiting all the time, with the default lookahead.
        let state = LimiterState {
            gain_db: 12.0,
            ..LimiterState::default()
        };
        let limiter = edit
            .add_processor(
                &format!("limiter-{index}"),
                Limiter::new(state, Meters::default()),
            )
            .unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            limiter.id(),
            Limiter::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(limiter.id(), Limiter::OUTPUT, 0))
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
    println!("100 limiters: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime");
}
