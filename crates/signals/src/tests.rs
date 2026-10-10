//! What a sound graph sounds like, one machine run frame by frame, without an engine.

use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use serde_json::json;

#[path = "../tests/sound/mod.rs"]
pub(crate) mod sound;

use sound_core::MAX_BLOCK;

use crate::machine::{Block, Inputs, Machine, Note};
use crate::{Code, Declarations, Graph};
use sound::*;

const SAMPLE_RATE: f32 = 48_000.0;

fn machine(code: Code) -> Machine {
    Machine::new(code, SAMPLE_RATE)
}

/// A block with every param at its default in its first frame.
fn defaults(machine: &Machine) -> Box<Block> {
    let mut block = Box::new(Block::new());
    for (row, parameter) in block.parameters.iter_mut().zip(&machine.code().parameters) {
        row[0] = parameter.default;
    }
    block
}

/// One frame of both channels, the first of `block`.
fn frame(machine: &mut Machine, block: &Block, arrays: &[Vec<f32>], note: Note) -> [f32; 2] {
    let mut output = [[0.0; MAX_BLOCK]; 2];
    let inputs = Inputs {
        block,
        arrays,
        note,
    };
    machine.render(&inputs, 0..1, &mut output);
    output.map(|channel| channel[0])
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
    let mut block = defaults(machine);
    block.bpm = 120.0;
    (0..frames)
        .map(|at| {
            block.input = [[input(at); MAX_BLOCK]; 2];
            self::frame(machine, &block, arrays, note(at))[0]
        })
        .collect()
}

fn run(machine: &mut Machine, input: impl Fn(usize) -> f32, frames: usize) -> Vec<f32> {
    run_with(machine, &[], |_| Note::default(), input, frames)
}

/// Both channels of `blocks` blocks of `input`, each rendered in spans of `span` frames.
/// In spans of one frame every operation runs frame by frame, in an order where each comes
/// after what it reads: what the code means.
fn render_in_spans(
    machine: &mut Machine,
    input: impl Fn(usize) -> f32,
    blocks: usize,
    span: usize,
) -> [Vec<f32>; 2] {
    let mut block = defaults(machine);
    let mut output = [[0.0; MAX_BLOCK]; 2];
    let mut rendered = [Vec::new(), Vec::new()];
    for index in 0..blocks {
        for (frame, sample) in block.input[0].iter_mut().enumerate() {
            *sample = input(index * MAX_BLOCK + frame);
        }
        block.input[1] = block.input[0];
        let inputs = Inputs {
            block: &block,
            arrays: &[],
            note: Note::default(),
        };
        for start in (0..MAX_BLOCK).step_by(span) {
            machine.render(&inputs, start..(start + span).min(MAX_BLOCK), &mut output);
        }
        for (rendered, output) in rendered.iter_mut().zip(&output) {
            rendered.extend_from_slice(output);
        }
    }
    rendered
}

fn loudest(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

#[test]
fn a_tremolo_follows_its_formula() {
    let mut tremolo = machine(compile(|| {
        let rate = param("rate", 4.0, [0.1, 20.0]);
        let depth = param("depth", 0.5, [0.0, 1.0]);
        let wave = 0.5 + 0.5 * sin(phasor(rate) * TAU);
        input() * (1.0 - depth * wave)
    }));
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
fn a_feedback_feeds_an_echo_back_and_each_repeat_is_softer() {
    let mut echo = machine(compile(|| {
        let echo = feedback();
        let wet = delay(input() + echo.read() * 0.5, 10.0);
        echo.set(wet);
        wet
    }));
    // One click; 10 ms is 480 frames, and the feedback adds a frame to each repeat after it.
    let output = run(&mut echo, |frame| if frame == 0 { 1.0 } else { 0.0 }, 2_000);
    assert!(output[..470].iter().all(|sample| sample.abs() < 1e-6));
    assert!((loudest(&output[475..485]) - 1.0).abs() < 0.01);
    assert!((loudest(&output[955..965]) - 0.5).abs() < 0.01);
    assert!((loudest(&output[1435..1445]) - 0.25).abs() < 0.01);
}

#[test]
fn a_lowpass_lets_the_lows_through_and_takes_the_highs_out() {
    let level = |hz: f32| {
        let mut filter = machine(compile(|| lowpass(input(), 1000.0, FRAC_1_SQRT_2)));
        let sine = |frame: usize| (std::f32::consts::TAU * hz * frame as f32 / SAMPLE_RATE).sin();
        loudest(&run(&mut filter, sine, 9_600)[4_800..])
    };
    assert!(level(100.0) > 0.99);
    assert!((level(1000.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    assert!(level(10_000.0) < 0.02);
}

#[test]
fn a_runaway_feedback_is_held_and_a_division_by_zero_is_silence() {
    let mut runaway = machine(compile(|| {
        let loud = feedback();
        loud.set(loud.read() * 2.0 + input());
        loud.read()
    }));
    assert!(
        run(&mut runaway, |_| 1.0, 1_000)
            .iter()
            .all(|sample| sample.abs() <= 4.0)
    );

    let mut divided = machine(compile(|| 1.0 / (input() - input())));
    assert!(
        run(&mut divided, |_| 1.0, 10)
            .iter()
            .all(|sample| *sample == 0.0)
    );
}

#[test]
fn a_buffer_keeps_what_was_written_and_a_read_further_back_hears_it_later() {
    let mut tape = machine(compile(|| {
        let tape = buffer(0.01);
        let position = feedback();
        position.set(position.read() + 1.0);
        tape.write(position.read(), input());
        tape.at(position.read() - 100.0)
    }));
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
fn feedbacks_through_filters_render_over_a_whole_span_as_they_do_frame_by_frame() {
    let sound = || {
        compile(|| {
            // A read whose source is in no loop with it: what `input` was a frame before.
            let before = feedback();
            let late = before.read();
            before.set(input());
            let ring = feedback();
            let wet = lowpass(input() + ring.read() * 0.9, 2000.0, 4.0);
            ring.set(saturate(wet));
            // A loop that hears nothing of the first runs in the same pass over the frames.
            // It hears the channel, so the right runs it again, but not the first.
            let other = feedback();
            let panned = lowpass(input() * channel() + other.read() * 0.8, 700.0, 2.0);
            other.set(saturate(panned));
            wet + panned + late * 0.5
        })
    };
    let input = |frame: usize| if frame % 97 < 3 { 1.0 } else { 0.0 };
    let whole = render_in_spans(&mut machine(sound()), input, 20, MAX_BLOCK);
    let by_frame = render_in_spans(&mut machine(sound()), input, 20, 1);
    assert!(loudest(&whole[0]) > 0.5);
    assert!(loudest(&whole[1]) > loudest(&whole[0]));
    assert_eq!(whole, by_frame);
}

#[test]
fn a_loop_that_hears_another_loop_hears_it_in_the_same_frame() {
    let mut echoes = machine(compile(|| {
        let first = feedback();
        let heard = input() + first.read() * 0.5;
        first.set(heard);
        let second = feedback();
        let echoed = heard + second.read() * 0.5;
        second.set(echoed);
        echoed
    }));
    // `heard` halves in every frame after the click, and `echoed` is `n + 1` halved `n` times.
    // Run before the first loop, the second would read 0 in the first frame.
    let click = |frame: usize| if frame == 0 { 1.0 } else { 0.0 };
    let [output, _] = render_in_spans(&mut echoes, click, 1, MAX_BLOCK);
    assert_eq!(output[..4], [1.0, 1.0, 0.75, 0.5]);
}

#[test]
fn loops_of_the_same_formulas_take_turns_and_each_hears_only_itself() {
    // As a tool makes them in a `for`.
    let again = [0.5, 0.25, -0.75];
    let mut echoes = machine(compile(|| {
        let [first, second, third] = again.map(|again| {
            let echo = feedback();
            let wet = input() + echo.read() * again;
            echo.set(wet);
            wet
        });
        first + second + third
    }));
    let click = |frame: usize| if frame.is_multiple_of(7) { 0.25 } else { 0.0 };
    let [output, _] = render_in_spans(&mut echoes, click, 2, MAX_BLOCK);
    let mut wet = [0.0_f32; 3];
    let expected: Vec<f32> = (0..output.len())
        .map(|frame| {
            for (wet, again) in wet.iter_mut().zip(again) {
                *wet = click(frame) + *wet * again;
            }
            wet[0] + wet[1] + wet[2]
        })
        .collect();
    assert_eq!(output, expected);
}

#[test]
fn a_buffer_read_hears_a_write_before_it_in_the_same_frame_and_one_after_it_a_frame_later() {
    let ramp = |frame: usize| frame as f32 * 0.01;
    let mut write_first = machine(compile(|| {
        let tape = buffer(0.01);
        tape.write(0.0, input());
        tape.at(0.0)
    }));
    let [output, _] = render_in_spans(&mut write_first, ramp, 1, MAX_BLOCK);
    assert_eq!(output, (0..MAX_BLOCK).map(ramp).collect::<Vec<_>>());

    let mut read_first = machine(compile(|| {
        let tape = buffer(0.01);
        let heard = tape.at(0.0);
        tape.write(0.0, input());
        heard
    }));
    let [output, _] = render_in_spans(&mut read_first, ramp, 1, MAX_BLOCK);
    let before = |frame: usize| frame.checked_sub(1).map_or(0.0, ramp);
    assert_eq!(output, (0..MAX_BLOCK).map(before).collect::<Vec<_>>());
}

#[test]
fn a_list_of_the_record_is_read_by_index_and_looked_up_between_its_values() {
    let mut steps = machine(compile(|| {
        let steps = pattern("steps", 4);
        let position = feedback();
        position.set(position.read() + 1.0);
        // `position` reads what it was in the frame before: 0, 1, 2 ...
        steps.at(position.read())
    }));
    let arrays = vec![vec![1.0, 0.0, 0.5, 0.25]];
    let output = run_with(&mut steps, &arrays, |_| Note::default(), |_| 0.0, 6);
    // The index is the whole part, and wraps.
    assert_eq!(output, [1.0, 0.0, 0.5, 0.25, 1.0, 0.0]);

    let mut table = machine(compile(|| lookup(&pattern("wave", 2), input())));
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
    let mut envelope = machine(compile(|| adsr(gate(), 10.0, 10.0, 0.5, 20.0)));
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
    let mut envelope = machine(compile(|| {
        adsr(gate(), 1.0, 1.0, 0.5 * input() / input(), 1.0)
    }));
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
    let mut clock = machine(compile(|| {
        let tick = rise(wrap(input()).lt(0.5));
        let held = hold(input(), tick);
        tick + held * 0.001
    }));
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

#[test]
fn mono_code_and_what_hears_it_run_with_the_index_of_each_channel() {
    let mut panned = machine(compile(|| channel() * 0.5 + 0.25));
    let block = defaults(&panned);
    assert_eq!(
        frame(&mut panned, &block, &[], Note::default()),
        [0.25, 0.75]
    );
}

#[test]
fn stereo_code_hears_both_channels_and_sets_both() {
    let mut swap = machine(compile(|| (input_right(), input_left() * 0.5)));
    let mut block = defaults(&swap);
    block.input = [[0.25; MAX_BLOCK], [1.0; MAX_BLOCK]];
    assert_eq!(frame(&mut swap, &block, &[], Note::default()), [1.0, 0.125]);
}

#[test]
fn a_graph_that_does_not_hold_says_what_is_wrong_in_the_words_of_the_sdk() {
    let error = |graph: serde_json::Value| {
        let graph: Graph = serde_json::from_value(graph).unwrap();
        graph
            .compile(&Declarations::default())
            .unwrap_err()
            .to_string()
    };
    let graph = |nodes: serde_json::Value| json!({ "nodes": nodes, "feedbacks": [], "buffers": [], "watches": [], "output": { "mono": 0 } });
    let found = error(graph(json!([{ "op": "add", "a": 0, "b": 1 }])));
    assert_eq!(found, "node 0 is read before it is made");
    let found = error(graph(json!([{ "op": "param", "name": "rate" }])));
    assert_eq!(found, "`rate` is no field or control of the tool");
    let found = error(graph(json!([{ "op": "constant", "value": 1e39 }])));
    assert!(found.starts_with("a number of a sound is at most"));
    let longest = json!([
        { "op": "input" },
        { "op": "delay", "x": 0, "ms": 0, "longest": 0 },
    ]);
    assert!(error(graph(longest)).starts_with("delay(x, ms, longest): longest is a number"));
    let delays: Vec<_> = (0..17)
        .map(|_| json!({ "op": "delay", "x": 0, "ms": 0 }))
        .collect();
    let nodes = [vec![json!({ "op": "input" })], delays].concat();
    assert_eq!(error(graph(json!(nodes))), "a sound has at most 16 delays");
}
