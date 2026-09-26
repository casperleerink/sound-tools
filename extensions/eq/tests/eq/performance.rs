//! How much faster than realtime many EQs render.

use std::time::Instant;

use eq::{Eq, EqState, Shape};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, band, noise, with_bands};

const EQS: usize = 100;
const SECONDS: usize = 10;

/// Four bands at work, one of each kind of shape.
fn busy() -> EqState {
    with_bands(&[
        band(Shape::LowCut, 80.0, 0.0, 0.71),
        band(Shape::Bell, 400.0, -3.0, 1.5),
        band(Shape::Bell, 3_000.0, 2.0, 1.0),
        band(Shape::HighShelf, 10_000.0, 3.0, 0.71),
    ])
}

/// Run with
/// `cargo nextest run -p eq --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_eqs() {
    for (label, moving) in [("still", false), ("with every band gliding", true)] {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        let mut eqs = Vec::new();
        for index in 0..EQS {
            let source = edit
                .add_processor(&format!("source-{index}"), Source::new(noise(0.01)))
                .unwrap();
            let eq = edit
                .add_processor(&format!("eq-{index}"), Eq::new(busy()))
                .unwrap();
            edit.connect(Connection::new(
                source.id(),
                Source::OUTPUT,
                eq.id(),
                Eq::INPUT,
            ))
            .unwrap();
            edit.connect(Connection::to_device(eq.id(), Eq::OUTPUT, 0))
                .unwrap();
            eqs.push(eq);
        }
        edit.commit().unwrap();
        let mut output = vec![0.0; SECONDS * SAMPLE_RATE as usize * 2];
        let started = Instant::now();
        for (block, buffer) in output.chunks_mut(512 * 2).enumerate() {
            // Every 20 ms a new frequency for every band, so a glide never ends.
            if moving && block % 2 == 0 {
                let mut state = busy();
                for band in &mut state.bands {
                    band.frequency_hz *= if block % 4 == 0 { 1.5 } else { 1.0 };
                }
                for eq in &eqs {
                    control.update(*eq, state).unwrap();
                }
            }
            engine.process_block(buffer);
        }
        let elapsed = started.elapsed().as_secs_f64();
        let ratio = SECONDS as f64 / elapsed;
        println!(
            "100 EQs of four bands, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
