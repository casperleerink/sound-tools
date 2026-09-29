//! How much faster than realtime many modulations render.

use std::time::Instant;

use modulation::{Mode, Modulation, ModulationState};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Source, noise};

const EFFECTS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p modulation --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_modulations() {
    for mode in Mode::ALL {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        let state = ModulationState {
            mode,
            feedback: 0.5,
            depth: 1.0,
            ..ModulationState::default()
        };
        for index in 0..EFFECTS {
            let source = edit
                .add_processor(&format!("source-{index}"), Source::new(noise(0.01)))
                .unwrap();
            let modulation = edit
                .add_processor(&format!("modulation-{index}"), Modulation::new(state))
                .unwrap();
            edit.connect(Connection::new(
                source.id(),
                Source::OUTPUT,
                modulation.id(),
                Modulation::INPUT,
            ))
            .unwrap();
            edit.connect(Connection::to_device(
                modulation.id(),
                Modulation::OUTPUT,
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
        println!(
            "{EFFECTS} modulations, {mode:?}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
