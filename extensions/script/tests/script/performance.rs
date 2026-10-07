#![allow(clippy::unwrap_used)]

//! How much faster than realtime many scripts render.

use std::time::Instant;

use script::{Machine, Script, ScriptState};
use sound_core::{Connection, Engine, EngineConfig};

const SAMPLE_RATE: u32 = 48_000;
const SCRIPTS: usize = 100;
const SECONDS: usize = 10;

/// Run with
/// `cargo nextest run -p script --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_scripts() {
    // Each makes its own sound, so no source runs next to it.
    let tremolo = [
        "param rate = 4 [0.1, 20]",
        "param depth = 0.5 [0, 1]",
        "wave = 0.5 + 0.5 * sin(phasor(rate) * tau)",
        "out = 0.01 * noise() * (1 - depth * wave)",
    ];
    let tape_echo = [
        "param time = 350 [1, 2000]",
        "param feedback = 0.45 [0, 0.95]",
        "param tone = 2500 [200, 12000]",
        "history echo",
        "wet = delay(0.01 * noise() + echo * feedback, time)",
        "echo = saturate(lowpass(wet, tone))",
        "out = mix(in, wet, 0.35)",
    ];
    for (label, code) in [("tremolo", &tremolo[..]), ("tape echo", &tape_echo[..])] {
        let state = ScriptState {
            code: code.iter().map(|line| line.to_string()).collect(),
            ..ScriptState::default()
        };
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        for index in 0..SCRIPTS {
            let (code, values) = state.compile().unwrap();
            let machine = Machine::new(code, &values, SAMPLE_RATE as f32);
            let script = edit
                .add_processor(&format!("script-{index}"), Script::new(Box::new(machine)))
                .unwrap();
            edit.connect(Connection::to_device(script.id(), Script::OUTPUT, 0))
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
        println!("100 scripts, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime");
    }
}
