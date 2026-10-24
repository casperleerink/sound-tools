#![allow(clippy::unwrap_used)]

//! How much faster than realtime many `Signals` processors render: effects, spectral ones too,
//! and instruments with every voice playing.

use std::time::Instant;

use sound_core::{Connection, Engine, EngineConfig};
use sound_signals::{Code, Kind, Machine, Signals, Values};

mod sound;

use sound::*;

const SAMPLE_RATE: u32 = 48_000;
const PROCESSORS: usize = 100;
const SECONDS: usize = 10;

/// A tape echo with a darkening feedback: a typical effect of a project.
fn echo() -> Code {
    compile(|| {
        let time = param("time", 350.0, [1.0, 2000.0]);
        let again = param("again", 0.45, [0.0, 0.95]);
        let echo = feedback();
        let wet = delay(0.01 * noise() + echo.read() * again, time);
        echo.set(saturate(lowpass(
            wet,
            2500.0,
            std::f32::consts::FRAC_1_SQRT_2,
        )));
        mix(input(), wet, 0.35)
    })
}

/// A filtered saw with an envelope, played as a source so its one voice always runs.
fn voice() -> Code {
    compile(|| {
        let level = adsr(1.0, 5.0, 300.0, 0.4, 200.0);
        let saw = phasor(110.0) * 2.0 - 1.0;
        0.01 * lowpass(saw, 300.0 + 3000.0 * level, 2.0) * level
    })
}

/// A freeze of a spectrum of 2048: each bin holds its magnitude and turns on by its last step
/// of phase while frozen.
fn freeze() -> Code {
    compile(|| {
        let frozen = param("frozen", 1.0, [0.0, 1.0]);
        spectral(input(), 2048, |bin| {
            let live = 1.0 - capture(frozen);
            let before = feedback();
            before.set(bin.phase);
            let step = hold(bin.phase - before.read(), live);
            let turned = feedback();
            let phase = mix(turned.read() + step, bin.phase, live);
            turned.set(phase);
            (hold(bin.magnitude, live), phase)
        })
    })
}

/// The vocoder of the doc of sounds: a chord of saws, as loud in each bin of 512 as the input.
fn vocoder() -> Code {
    compile(|| {
        let saw = |hz: f32| phasor(hz) * 2.0 - 1.0;
        let chord = saw(130.8) + saw(196.0) + saw(261.6);
        spectral_of_two(chord, input(), 512, |saws, voice| {
            (
                saws.magnitude * smooth(voice.magnitude, 20.0) * 2.0,
                saws.phase,
            )
        })
    })
}

/// Run with
/// `cargo nextest run -p sound-signals --run-ignored only realtime_ratio --no-capture`.
#[test]
#[ignore = "a measurement, not a check; run it locally and read the printed ratio"]
fn realtime_ratio_of_one_hundred_processors() {
    for (label, code, kind) in [
        ("echo effect", echo(), Kind::One),
        ("saw source", voice(), Kind::One),
        ("spectral freeze effect", freeze(), Kind::One),
        ("spectral vocoder effect", vocoder(), Kind::One),
    ] {
        let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let mut edit = control.edit();
        for index in 0..PROCESSORS {
            let code = code.clone();
            let mut values = Values::default();
            for (value, parameter) in values.parameters.iter_mut().zip(&code.parameters) {
                *value = parameter.default;
            }
            let machine = Box::new(Machine::new(code, SAMPLE_RATE as f32));
            let signals = Signals::new(kind, machine, values, Vec::new());
            let signals = edit
                .add_processor(&format!("signals-{index}"), signals)
                .unwrap();
            edit.connect(Connection::to_device(signals.id(), Signals::OUTPUT, 0))
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
            "{PROCESSORS} signals processors, {label}: {SECONDS} s in {elapsed:.3} s, {ratio:.1} times realtime"
        );
    }
}
