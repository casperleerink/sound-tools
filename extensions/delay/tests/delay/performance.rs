//! How much faster than realtime many delays render.

use std::time::Instant;

use delay::{Delay, DelayState};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, noise};

const DELAYS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p delay --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_delays() {
    for (label, moving) in [
        ("still", false),
        ("with the time and the cuts moving", true),
    ] {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        let mut delays = Vec::new();
        for index in 0..DELAYS {
            let source = edit
                .add_processor(&format!("source-{index}"), Source::new(noise(0.01)))
                .unwrap();
            let delay = edit
                .add_processor(&format!("delay-{index}"), Delay::new(DelayState::default()))
                .unwrap();
            edit.connect(Connection::new(
                source.id(),
                Source::OUTPUT,
                delay.id(),
                Delay::INPUT,
            ))
            .unwrap();
            edit.connect(Connection::to_device(delay.id(), Delay::OUTPUT, 0))
                .unwrap();
            delays.push(delay);
        }
        edit.commit().unwrap();
        let mut output = vec![0.0; SECONDS * SAMPLE_RATE as usize * 2];
        let started = Instant::now();
        for (index, buffer) in output.chunks_mut(512 * 2).enumerate() {
            // A new time and new cuts every block of 10.7 ms keep every delay fading between
            // taps and working out its factors all the time: the most it ever does.
            if moving {
                let along = (index % 100) as f32 / 100.0;
                let state = DelayState {
                    sync: false,
                    time_ms: 100.0 + 400.0 * along,
                    low_cut_hz: 100.0 + 400.0 * along,
                    ..DelayState::default()
                };
                for delay in &delays {
                    control.update(*delay, state).unwrap();
                }
            }
            engine.process_block(buffer);
        }
        let elapsed = started.elapsed().as_secs_f64();
        let ratio = SECONDS as f64 / elapsed;
        println!(
            "{DELAYS} delays, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
