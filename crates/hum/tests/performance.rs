#![allow(clippy::unwrap_used)]

//! How much faster than realtime many Hum processors render: effects, and instruments with
//! every voice playing.

use std::time::Instant;

use sound_core::{Connection, Engine, EngineConfig};
use sound_hum::{Hum, Kind, Machine, Values, compile};

const SAMPLE_RATE: u32 = 48_000;
const PROCESSORS: usize = 100;
const SECONDS: usize = 10;

/// A tape echo with a darkening feedback: a typical effect of a project.
const ECHO: &[&str] = &[
    "param time = 350 [1, 2000]",
    "param again = 0.45 [0, 0.95]",
    "history echo",
    "wet = delay(0.01 * noise() + echo * again, time)",
    "echo = saturate(lowpass(wet, 2500))",
    "out = mix(in, wet, 0.35)",
];

/// A filtered saw with an envelope, played as a source so its one voice always runs.
const VOICE: &[&str] = &[
    "level = adsr(1, 5, 300, 0.4, 200)",
    "saw = phasor(110) * 2 - 1",
    "out = 0.01 * lowpass(saw, 300 + 3000 * level, 2) * level",
];

/// Run with
/// `cargo nextest run -p sound-hum --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_processors() {
    for (label, code, kind) in [
        ("echo effect", ECHO, Kind::Effect),
        ("saw source", VOICE, Kind::Source),
    ] {
        let lines: Vec<String> = code.iter().map(|line| line.to_string()).collect();
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        for index in 0..PROCESSORS {
            let code = compile(&lines).unwrap();
            let mut values = Values::default();
            for (value, parameter) in values.parameters.iter_mut().zip(&code.parameters) {
                *value = parameter.default;
            }
            let machine = Box::new(Machine::new(code, SAMPLE_RATE as f32));
            let hum = Hum::new(kind, machine, values, Vec::new());
            let hum = edit.add_processor(&format!("hum-{index}"), hum).unwrap();
            edit.connect(Connection::to_device(hum.id(), Hum::OUTPUT, 0))
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
            "{PROCESSORS} Hum processors, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
