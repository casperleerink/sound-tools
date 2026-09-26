//! How much faster than realtime many reverbs render.

use std::time::Instant;

use reverb::{Reverb, ReverbState};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, noise};

const REVERBS: usize = 20;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p reverb --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_twenty_reverbs() {
    for (label, moving) in [("still", false), ("with the size moving", true)] {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        let mut reverbs = Vec::new();
        for index in 0..REVERBS {
            let source = edit
                .add_processor(&format!("source-{index}"), Source::new(noise(0.01)))
                .unwrap();
            let reverb = edit
                .add_processor(&format!("reverb-{index}"), Reverb::new(ReverbState::default()))
                .unwrap();
            edit.connect(Connection::new(
                source.id(),
                Source::OUTPUT,
                reverb.id(),
                Reverb::INPUT,
            ))
            .unwrap();
            edit.connect(Connection::to_device(reverb.id(), Reverb::OUTPUT, 0))
                .unwrap();
            reverbs.push(reverb);
        }
        edit.commit().unwrap();
        let mut output = vec![0.0; SECONDS * SAMPLE_RATE as usize * 2];
        let started = Instant::now();
        for (index, buffer) in output.chunks_mut(512 * 2).enumerate() {
            // A new size every block of 10.7 ms keeps every reverb fading between taps and
            // working out its factors all the time: the most it ever does.
            if moving {
                let state = ReverbState {
                    size: (index % 100) as f32 / 100.0,
                    ..ReverbState::default()
                };
                for reverb in &reverbs {
                    control.update(*reverb, state).unwrap();
                }
            }
            engine.process_block(buffer);
        }
        let elapsed = started.elapsed().as_secs_f64();
        let ratio = SECONDS as f64 / elapsed;
        println!(
            "{REVERBS} reverbs, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
