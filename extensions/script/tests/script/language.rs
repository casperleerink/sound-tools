#![allow(clippy::unwrap_used)]

//! What a script sounds like, run frame by frame without an engine.

use script::{Machine, ScriptState};

const SAMPLE_RATE: f32 = 48_000.0;

fn machine(code: &[&str]) -> Machine {
    let state = ScriptState {
        code: code.iter().map(|line| line.to_string()).collect(),
        ..ScriptState::default()
    };
    let (code, values) = state.compile().unwrap();
    Machine::new(code, &values, SAMPLE_RATE)
}

/// The left channel of `frames` frames of a steady input.
fn run(machine: &mut Machine, input: impl Fn(usize) -> f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|frame| machine.frame([input(frame); 2])[0])
        .collect()
}

#[test]
fn a_tremolo_follows_its_formula() {
    let mut tremolo = machine(&[
        "param rate = 4 [0.1, 20]",
        "param depth = 0.5 [0, 1]",
        "wave = 0.5 + 0.5 * sin(phasor(rate) * tau)",
        "out = in * (1 - depth * wave)",
    ]);
    let output = run(&mut tremolo, |_| 1.0, 48_000);
    for (frame, sample) in output.iter().enumerate() {
        let phase = (4.0 * frame as f64 / 48_000.0).fract();
        let wave = 0.5 + 0.5 * (phase * std::f64::consts::TAU).sin();
        let expected = 1.0 - 0.5 * wave;
        assert!(
            (f64::from(*sample) - expected).abs() < 1e-3,
            "frame {frame}"
        );
    }
}

#[test]
fn a_history_feeds_an_echo_back_and_each_repeat_is_softer() {
    let mut echo = machine(&[
        "history echo",
        "wet = delay(in + echo * 0.5, 10)",
        "echo = wet",
        "out = wet",
    ]);
    // One click; 10 ms is 480 frames, and the history adds a frame to each repeat after it.
    let output = run(&mut echo, |frame| if frame == 0 { 1.0 } else { 0.0 }, 2_000);
    let loudest = |from: usize| {
        output[from..from + 10]
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
    };
    assert!(output[..470].iter().all(|sample| sample.abs() < 1e-6));
    assert!((loudest(475) - 1.0).abs() < 0.01);
    assert!((loudest(955) - 0.5).abs() < 0.01);
    assert!((loudest(1435) - 0.25).abs() < 0.01);
}

#[test]
fn a_lowpass_lets_the_lows_through_and_takes_the_highs_out() {
    let level = |hz: f32| {
        let mut filter = machine(&["out = lowpass(in, 1000)"]);
        let sine = |frame: usize| (std::f32::consts::TAU * hz * frame as f32 / SAMPLE_RATE).sin();
        let output = run(&mut filter, sine, 9_600);
        output[4_800..]
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
    };
    assert!(level(100.0) > 0.99);
    assert!((level(1000.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    assert!(level(10_000.0) < 0.02);
}

#[test]
fn a_runaway_feedback_is_held_and_a_division_by_zero_is_silence() {
    let mut runaway = machine(&["history loud", "loud = loud * 2 + in", "out = loud"]);
    let output = run(&mut runaway, |_| 1.0, 1_000);
    assert!(output.iter().all(|sample| sample.abs() <= 4.0));

    let mut divided = machine(&["out = 1 / (in - in)"]);
    assert!(
        run(&mut divided, |_| 1.0, 10)
            .iter()
            .all(|sample| *sample == 0.0)
    );
}
