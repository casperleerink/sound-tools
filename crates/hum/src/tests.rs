//! What Hum code sounds like, one machine run frame by frame, without an engine.

use crate::language::{MAX_PARAMETERS, compile};
use crate::machine::{Inputs, Machine, Note};

const SAMPLE_RATE: f32 = 48_000.0;

fn machine(code: &[&str]) -> Machine {
    let lines: Vec<String> = code.iter().map(|line| line.to_string()).collect();
    Machine::new(compile(&lines).unwrap(), SAMPLE_RATE)
}

/// Every param at its default.
fn defaults(machine: &Machine) -> [f32; MAX_PARAMETERS] {
    let mut values = [0.0; MAX_PARAMETERS];
    for (value, parameter) in values.iter_mut().zip(&machine.code().parameters) {
        *value = parameter.default;
    }
    values
}

/// The left channel of `frames` frames, with `input` in and `arrays` as the lists of the
/// record.
fn run_with(
    machine: &mut Machine,
    arrays: &[Vec<f32>],
    note: impl Fn(usize) -> Note,
    input: impl Fn(usize) -> f32,
    frames: usize,
) -> Vec<f32> {
    let parameters = defaults(machine);
    (0..frames)
        .map(|frame| {
            let inputs = Inputs {
                input: [input(frame); 2],
                parameters: &parameters,
                lives: &[],
                arrays,
                triggers: 0,
                beat: 0.0,
                bpm: 120.0,
                playing: false,
                note: note(frame),
            };
            machine.frame(&inputs)[0]
        })
        .collect()
}

fn run(machine: &mut Machine, input: impl Fn(usize) -> f32, frames: usize) -> Vec<f32> {
    run_with(machine, &[], |_| Note::default(), input, frames)
}

fn loudest(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
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
    assert!(output[..470].iter().all(|sample| sample.abs() < 1e-6));
    assert!((loudest(&output[475..485]) - 1.0).abs() < 0.01);
    assert!((loudest(&output[955..965]) - 0.5).abs() < 0.01);
    assert!((loudest(&output[1435..1445]) - 0.25).abs() < 0.01);
}

#[test]
fn a_lowpass_lets_the_lows_through_and_takes_the_highs_out() {
    let level = |hz: f32| {
        let mut filter = machine(&["out = lowpass(in, 1000)"]);
        let sine = |frame: usize| (std::f32::consts::TAU * hz * frame as f32 / SAMPLE_RATE).sin();
        loudest(&run(&mut filter, sine, 9_600)[4_800..])
    };
    assert!(level(100.0) > 0.99);
    assert!((level(1000.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    assert!(level(10_000.0) < 0.02);
}

#[test]
fn a_runaway_feedback_is_held_and_a_division_by_zero_is_silence() {
    let mut runaway = machine(&["history loud", "loud = loud * 2 + in", "out = loud"]);
    assert!(
        run(&mut runaway, |_| 1.0, 1_000)
            .iter()
            .all(|sample| sample.abs() <= 4.0)
    );

    let mut divided = machine(&["out = 1 / (in - in)"]);
    assert!(
        run(&mut divided, |_| 1.0, 10)
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn a_buffer_keeps_what_was_written_and_a_read_further_back_hears_it_later() {
    let mut tape = machine(&[
        "buffer tape = 0.01",
        "history position",
        "position = position + 1",
        "tape[position] = in",
        "out = tape[position - 100]",
    ]);
    let input = |frame: usize| (frame as f32 * 0.01).sin();
    let output = run(&mut tape, input, 400);
    for frame in 100..400 {
        assert!(
            (output[frame] - input(frame - 100)).abs() < 1e-6,
            "frame {frame}"
        );
    }
}

#[test]
fn a_list_of_the_record_is_read_by_index_and_looked_up_between_its_values() {
    let mut steps = machine(&[
        "param steps[4] = 0 [0, 1]",
        "history position",
        "position = position + 1",
        // `position` reads what it was in the frame before: 0, 1, 2 ...
        "out = steps[position]",
    ]);
    let arrays = vec![vec![1.0, 0.0, 0.5, 0.25]];
    let output = run_with(&mut steps, &arrays, |_| Note::default(), |_| 0.0, 6);
    // The index is the whole part, and wraps.
    assert_eq!(output, [1.0, 0.0, 0.5, 0.25, 1.0, 0.0]);

    let mut table = machine(&["param wave[2] = 0 [0, 1]", "out = lookup(wave, in)"]);
    let arrays = vec![vec![0.0, 1.0]];
    let phases = [0.0, 0.25, 0.5, 0.75];
    let output = run_with(
        &mut table,
        &arrays,
        |_| Note::default(),
        |frame| phases[frame],
        4,
    );
    // Between the values in a straight line, and from the last back to the first.
    assert_eq!(output, [0.0, 0.5, 1.0, 0.5]);
}

#[test]
fn an_adsr_rises_holds_at_its_sustain_and_falls_to_silence_after_the_gate() {
    let mut envelope = machine(&["out = adsr(gate, 10, 10, 0.5, 20)"]);
    // 10 ms is 480 frames. The gate is up for 2400 frames.
    let note = |frame: usize| Note {
        gate: frame < 2_400,
        ..Note::default()
    };
    let output = run_with(&mut envelope, &[], note, |_| 0.0, 4_000);
    assert!((output[479] - 1.0).abs() < 0.01, "{}", output[479]);
    assert!((output[1_500] - 0.5).abs() < 0.01);
    // Half way through the release, half way down.
    assert!(
        (output[2_400 + 480] - 0.25).abs() < 0.01,
        "{}",
        output[2_400 + 480]
    );
    assert_eq!(output[2_400 + 961], 0.0);
}

#[test]
fn an_adsr_whose_sustain_was_not_a_number_sounds_again_after_it() {
    // `in / in` is not a number while `in` is 0.
    let mut envelope = machine(&["out = adsr(gate, 1, 1, 0.5 * in / in, 1)"]);
    let held = |_| Note {
        gate: true,
        ..Note::default()
    };
    let input = |frame: usize| if frame < 100 { 0.0 } else { 1.0 };
    let output = run_with(&mut envelope, &[], held, input, 200);
    assert_eq!(output[199], 0.5);
}

#[test]
fn rise_fires_once_when_a_value_goes_up_and_hold_keeps_it_until_the_next() {
    let mut clock = machine(&[
        "tick = rise(wrap(in) < 0.5)",
        "held = hold(in, tick)",
        "out = tick + held * 0.001",
    ]);
    // `in` counts up by a quarter each frame, so `wrap(in) < 0.5` goes up every 4 frames.
    let output = run(&mut clock, |frame| frame as f32 * 0.25, 9);
    let ticks: Vec<bool> = output.iter().map(|sample| *sample >= 1.0).collect();
    assert_eq!(
        ticks,
        [true, false, false, false, true, false, false, false, true]
    );
    // What `hold` kept in frame 4, while `in` was 1, is still there in frame 7.
    assert!((output[7] - 0.001).abs() < 1e-6);
}
