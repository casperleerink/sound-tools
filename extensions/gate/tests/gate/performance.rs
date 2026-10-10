//! How much faster than realtime many gates render.

use std::time::Instant;

use gate::{Gate, GateState, Meters};
use sound_core::{Connection, Engine, EngineConfig};

use crate::support::{SAMPLE_RATE, Signal, Source, noise};

const GATES: usize = 100;
const SECONDS: usize = 10;

/// Noise hits every `period` frames that fall 60 dB in 300 ms: cheap to make, so the time is
/// the gates'.
fn noise_hits(period: usize) -> Signal {
    let mut noise = noise(0.5);
    let fall = 10_f32.powf(-60.0 / 20.0 / (0.3 * SAMPLE_RATE as f32));
    let (mut frame, mut level) = (0, 1.0);
    Box::new(move || {
        if frame % period == 0 {
            level = 1.0;
        }
        frame += 1;
        level *= fall;
        noise().map(|sample| sample * level)
    })
}

fn silence() -> Signal {
    Box::new(|| [0.0; 2])
}

/// 100 gates, each keyed and shaping, so every part of a gate runs.
fn render(label: &str, input: impl Fn() -> Signal, key: impl Fn() -> Signal) {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    let mut edit = control.edit();
    let state = GateState {
        threshold_db: -30.0,
        transient_db: 6.0,
        sustain_db: -6.0,
        ..GateState::default()
    };
    for index in 0..GATES {
        let source = Source::new(input());
        let source = edit.add_processor(&format!("source-{index}"), source);
        let key = edit.add_processor(&format!("key-{index}"), Source::new(key()));
        let gate = Gate::new(state, Meters::default());
        let gate = edit.add_processor(&format!("gate-{index}"), gate).unwrap();
        let (source, key) = (source.unwrap(), key.unwrap());
        let connections = [
            Connection::new(source.id(), Source::OUTPUT, gate.id(), Gate::INPUT),
            Connection::new(key.id(), Source::OUTPUT, gate.id(), Gate::SIDECHAIN),
            Connection::to_device(gate.id(), Gate::OUTPUT, 0),
        ];
        for connection in connections {
            edit.connect(connection).unwrap();
        }
    }
    edit.commit().unwrap();
    let mut output = vec![0.0; SECONDS * SAMPLE_RATE as usize * 2];
    let started = Instant::now();
    for buffer in output.chunks_mut(512 * 2) {
        engine.process_block(buffer);
    }
    let elapsed = started.elapsed().as_secs_f64();
    let ratio = SECONDS as f64 / elapsed;
    println!("100 gates, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime");
}

/// Run with
/// `cargo nextest run -p gate --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_gates() {
    let rate = SAMPLE_RATE as usize;
    render("playing", || noise_hits(rate / 4), || noise_hits(rate / 2));
    render("idle", silence, silence);
}
