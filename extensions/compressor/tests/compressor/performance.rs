//! How much faster than realtime many compressors render.

use std::time::Instant;

use compressor::{Compressor, CompressorState, Lookahead, Meters};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, noise};

const COMPRESSORS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p compressor --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_compressors() {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    for index in 0..COMPRESSORS {
        let source = edit
            .add_processor(&format!("source-{index}"), Source::new(noise(0.5)))
            .unwrap();
        let state = CompressorState {
            lookahead: Lookahead::One,
            ..CompressorState::default()
        };
        let compressor = edit
            .add_processor(
                &format!("compressor-{index}"),
                Compressor::new(state, Meters::default()),
            )
            .unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            compressor.id(),
            Compressor::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(
            compressor.id(),
            Compressor::OUTPUT,
            0,
        ))
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
    println!("100 compressors: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime");
}
