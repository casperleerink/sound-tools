//! How much faster than realtime many saturators render.

use std::time::Instant;

use saturator::{Curve, Saturator, SaturatorState};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, noise};

const SATURATORS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p saturator --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_saturators() {
    for (label, moving) in [("still", false), ("with the drive moving", true)] {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        let mut saturators = Vec::new();
        let state = SaturatorState {
            curve: Curve::Tube,
            tone_db: 3.0,
            mix: 0.7,
            ..SaturatorState::default()
        };
        for index in 0..SATURATORS {
            let source = edit
                .add_processor(&format!("source-{index}"), Source::new(noise(0.25)))
                .unwrap();
            let saturator = edit
                .add_processor(&format!("saturator-{index}"), Saturator::new(state))
                .unwrap();
            edit.connect(Connection::new(
                source.id(),
                Source::OUTPUT,
                saturator.id(),
                Saturator::INPUT,
            ))
            .unwrap();
            edit.connect(Connection::to_device(saturator.id(), Saturator::OUTPUT, 0))
                .unwrap();
            saturators.push(saturator);
        }
        edit.commit().unwrap();
        let mut output = vec![0.0; SECONDS * SAMPLE_RATE as usize * 2];
        let started = Instant::now();
        for (index, buffer) in output.chunks_mut(512 * 2).enumerate() {
            // A new drive and tone every block of 10.7 ms keeps every saturator working out its
            // automatic gain and its tone all the time: the most it ever does.
            if moving {
                let along = (index % 100) as f32 / 100.0;
                let moved = SaturatorState {
                    drive_db: 36.0 * along,
                    tone_db: 24.0 * along - 12.0,
                    ..state
                };
                for saturator in &saturators {
                    control.update(*saturator, moved).unwrap();
                }
            }
            engine.process_block(buffer);
        }
        let elapsed = started.elapsed().as_secs_f64();
        let ratio = SECONDS as f64 / elapsed;
        println!(
            "{SATURATORS} saturators, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
