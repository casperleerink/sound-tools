//! How much faster than realtime many utilities render.

use std::time::Instant;

use sound_core::{Connection, Engine, EngineConfig};
use utility::{Utility, UtilityState};

use crate::support::{SAMPLE_RATE, Source, noise};

const UTILITIES: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p utility --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_utilities() {
    let everything = UtilityState {
        gain_db: -6.0,
        pan: 0.3,
        width: 1.5,
        bass_mono: true,
        invert_left: true,
        ..UtilityState::default()
    };
    for (label, state) in [
        ("at the defaults", UtilityState::default()),
        ("with bass mono and everything on", everything),
    ] {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        for index in 0..UTILITIES {
            let source = edit
                .add_processor(&format!("source-{index}"), Source::new(noise(0.01)))
                .unwrap();
            let utility = edit
                .add_processor(&format!("utility-{index}"), Utility::new(state))
                .unwrap();
            edit.connect(Connection::new(
                source.id(),
                Source::OUTPUT,
                utility.id(),
                Utility::INPUT,
            ))
            .unwrap();
            edit.connect(Connection::to_device(utility.id(), Utility::OUTPUT, 0))
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
        println!("100 utilities {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime");
    }
}
