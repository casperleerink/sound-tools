//! What a sound graph sounds like, one machine run frame by frame, without an engine.

use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use serde_json::json;

#[path = "../tests/sound/mod.rs"]
pub(crate) mod sound;

use sound_core::{MAX_BLOCK, SvfShape};

use crate::machine::{Block, Inputs, List, Machine, Note};
use crate::{ArraySpec, Code, Declarations, Graph};
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
fn frame(machine: &mut Machine, block: &Block, arrays: &[List], note: Note) -> [f32; 2] {
    let mut output = [[0.0; MAX_BLOCK]; 2];
    let inputs = Inputs {
        block,
        arrays,
        note,
    };
    machine.render(&inputs, 0..1, std::slice::from_mut(&mut output));
    output.map(|channel| channel[0])
}

/// The left channel of `frames` frames, with `input` in and `arrays` as the lists of the
/// record.
fn run_with(
    machine: &mut Machine,
    arrays: &[List],
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
            let span = start..(start + span).min(MAX_BLOCK);
            machine.render(&inputs, span, std::slice::from_mut(&mut output));
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

/// Each filter of the SDK, measured with a steady sine, gives what [`ShapedSvf::response`]
/// says, in level and in phase.
#[test]
fn every_filter_sounds_as_its_response_says() {
    const CUTOFF: f32 = 1_000.0;
    const Q: f32 = 2.0;
    const GAIN_DB: f32 = 6.0;
    let filters = [
        ("lowpass", SvfShape::LowPass),
        ("highpass", SvfShape::HighPass),
        ("bandpass", SvfShape::BandPass),
        ("notch", SvfShape::Notch),
        ("allpass", SvfShape::AllPass),
        ("peak", SvfShape::Peak),
        ("lowshelf", SvfShape::LowShelf),
        ("highshelf", SvfShape::HighShelf),
    ];
    // Whole periods of each fit in the window, so the sums below measure one frequency.
    const WINDOW: usize = 4_800;
    for (op, shape) in filters {
        let shaped = shape.shaped(CUTOFF, GAIN_DB, Q, SAMPLE_RATE);
        for hz in [100.0, 700.0, 1_000.0, 1_400.0, 6_000.0] {
            let mut filter = machine(compile(|| match shape {
                SvfShape::Peak | SvfShape::LowShelf | SvfShape::HighShelf => {
                    gained_filter(op, input(), CUTOFF, GAIN_DB, Q)
                }
                _ => filter(op, input(), CUTOFF, Q),
            }));
            let angle = |frame: usize| TAU as f64 * f64::from(hz) * frame as f64 / 48_000.0;
            let sine = |frame| angle(frame).sin() as f32;
            let [output, _] = render_in_spans(&mut filter, sine, 2 * WINDOW / MAX_BLOCK, MAX_BLOCK);
            // The output is `real sin + imaginary cos` of the input's angle.
            let (real, imaginary) =
                (WINDOW..2 * WINDOW).fold((0.0, 0.0), |(real, imaginary), frame| {
                    let sample = f64::from(output[frame]) * 2.0 / WINDOW as f64;
                    (
                        real + sample * angle(frame).sin(),
                        imaginary + sample * angle(frame).cos(),
                    )
                });
            let (expected_real, expected_imaginary) = shaped.response(hz, SAMPLE_RATE);
            assert!(
                (real - expected_real).abs() < 1e-3
                    && (imaginary - expected_imaginary).abs() < 1e-3,
                "{op} at {hz} Hz: ({real}, {imaginary}), not ({expected_real}, {expected_imaginary})"
            );
        }
    }
}

/// When only the gain differs between the channels, each channel hears its own.
#[test]
fn a_filter_hears_the_gain_of_its_channel() {
    let peak =
        |db: fn() -> Signal| machine(compile(|| gained_filter("peak", 0.1, 1_000.0, db(), 1.0)));
    let [left, right] = render_in_spans(&mut peak(|| channel() * 6.0), |_| 0.0, 1, MAX_BLOCK);
    let [flat, _] = render_in_spans(&mut peak(|| 0.0.into()), |_| 0.0, 1, MAX_BLOCK);
    let [raised, _] = render_in_spans(&mut peak(|| 6.0.into()), |_| 0.0, 1, MAX_BLOCK);
    assert_eq!((left, right), (flat, raised));
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

    // Through a filter it runs away too, and starts again from silence as it does without one.
    let mut comb = machine(compile(|| {
        let comb = feedback();
        comb.set(lowpass(input() + comb.read() * 2.0, 5000.0, FRAC_1_SQRT_2));
        comb.read()
    }));
    let output = run(&mut comb, |_| 1.0, 10_000);
    assert!(loudest(&output[9_000..]) > 1.0);

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
    let arrays = vec![List::One(vec![1.0, 0.0, 0.5, 0.25])];
    let output = run_with(&mut steps, &arrays, |_| Note::default(), |_| 0.0, 6);
    // The index is the whole part, and wraps.
    assert_eq!(output, [1.0, 0.0, 0.5, 0.25, 1.0, 0.0]);

    let mut table = machine(compile(|| lookup(&pattern("wave", 2), input())));
    let arrays = vec![List::One(vec![0.0, 1.0])];
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
fn a_looper_plays_two_minutes_of_its_input_back_bit_for_bit_on_each_channel() {
    const LOOP: usize = 120 * SAMPLE_RATE as usize;
    let mut looper = machine(compile(|| {
        let tape = buffer(120.0);
        let count = feedback();
        let frame = count.read();
        count.set(frame + 1.0);
        let recording = frame.lt(LOOP as f32);
        let old = tape.at(frame);
        tape.write(frame, input() * recording + old * (1.0 - recording));
        old
    }));
    // A different noise on each channel.
    let input = |channel: usize, frame: usize| {
        let seed = (frame * 2 + channel + 1) as u32;
        seed.wrapping_mul(2_654_435_761) as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    let mut block = defaults(&looper);
    let mut output = [[0.0; MAX_BLOCK]; 2];
    for start in (0..2 * LOOP).step_by(MAX_BLOCK) {
        for (channel, row) in block.input.iter_mut().enumerate() {
            for (offset, sample) in row.iter_mut().enumerate() {
                *sample = input(channel, start + offset);
            }
        }
        let inputs = Inputs {
            block: &block,
            arrays: &[],
            note: Note::default(),
        };
        looper.render(&inputs, 0..MAX_BLOCK, std::slice::from_mut(&mut output));
        for (channel, row) in output.iter().enumerate() {
            for (offset, sample) in row.iter().enumerate() {
                let frame = start + offset;
                let expected = frame.checked_sub(LOOP).map_or(0.0, |at| input(channel, at));
                assert_eq!(sample.to_bits(), expected.to_bits(), "frame {frame}");
            }
        }
    }
}

#[test]
fn a_sample_in_stereo_plays_each_side_on_its_channel_and_both_mixed_where_the_code_runs_once() {
    let read = |code: Code, arrays: &[List]| {
        let mut machine = machine(code);
        let block = defaults(&machine);
        frame(&mut machine, &block, arrays, Note::default())
    };
    let stereo = [List::Stereo(vec![1.0, 0.0], vec![0.5, 0.0])];
    let own = || sample("sound").at(0.0) * 2.0;
    assert_eq!(read(compile(own), &stereo), [2.0, 1.0]);
    let looked_up = || lookup(&sample("sound"), 0.0);
    assert_eq!(read(compile(looked_up), &stereo), [1.0, 0.5]);
    // A sample of one channel is the same on both.
    assert_eq!(read(compile(own), &[List::One(vec![0.25])]), [0.5, 0.5]);
    let crossed = || {
        let sound = sample("sound");
        (sound.side("right").at(0.0), sound.side("left").at(0.0))
    };
    assert_eq!(read(compile(crossed), &stereo), [0.5, 1.0]);
    let mixed = || {
        let sound = sample("sound");
        (sound.at(0.0), lookup(&sound, 0.0))
    };
    assert_eq!(read(compile(mixed), &stereo), [0.75, 0.75]);
}

#[test]
fn a_second_output_hears_a_sample_in_stereo_on_each_channel() {
    let nodes = json!([
        { "op": "constant", "value": 0.0 },
        { "op": "at", "table": { "list": "sound" }, "index": 0 },
    ]);
    let outputs = json!([{ "name": "low", "output": { "mono": 1 } }]);
    let graph = json!({ "nodes": nodes, "feedbacks": [], "buffers": [], "watches": [], "numbers": [], "bangs": [], "notes": [], "output": { "mono": 0 }, "outputs": outputs });
    let graph: Graph = serde_json::from_value(graph).unwrap();
    let mut declarations = Declarations::default();
    declarations.arrays.push(ArraySpec {
        name: "sound".to_string(),
        length: None,
        default: 0.0,
        min: -1.0,
        max: 1.0,
    });
    let mut machine = machine(graph.compile(&declarations).unwrap());
    let block = defaults(&machine);
    let inputs = Inputs {
        block: &block,
        arrays: &[List::Stereo(vec![1.0], vec![0.5])],
        note: Note::default(),
    };
    let mut outputs = [[[0.0; MAX_BLOCK]; 2]; 2];
    machine.render(&inputs, 0..1, &mut outputs);
    let [_, low] = outputs.map(|output| output.map(|channel| channel[0]));
    assert_eq!(low, [1.0, 0.5]);
}

#[test]
fn a_sample_read_for_samples_of_one_channel_runs_once_for_both_channels() {
    let own = compile(|| sample("sound").at(0.0) * 2.0);
    assert!(!machine(own.clone()).per_channel.is_empty());
    let mut once = machine(own.with_samples_of_one_channel());
    assert!(once.per_channel.is_empty());
    let block = defaults(&once);
    let one = [List::One(vec![0.25])];
    assert_eq!(frame(&mut once, &block, &one, Note::default()), [0.5, 0.5]);
    let stereo = [List::Stereo(vec![1.0], vec![0.5])];
    assert_eq!(
        frame(&mut once, &block, &stereo, Note::default()),
        [1.5, 1.5]
    );
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
fn a_hold_and_the_math_on_it_keep_its_value_over_spans_where_its_trigger_does_not_fire() {
    let mut held = machine(compile(|| {
        let x = input();
        hold(x, x.lt(-1.0)) * 0.5 + 1.0
    }));
    // A new value to hold every 50 frames, in spans that start anywhere in one.
    let input = |frame: usize| {
        if frame.is_multiple_of(50) {
            -1.5 - frame as f32 * 0.001
        } else {
            0.0
        }
    };
    let rendered = render_in_spans(&mut held, input, 3, 24);
    let mut kept = 0.0;
    let expected: Vec<f32> = (0..3 * MAX_BLOCK)
        .map(|frame| {
            if input(frame) < -1.0 {
                kept = input(frame);
            }
            kept * 0.5 + 1.0
        })
        .collect();
    assert_eq!(rendered, [expected.clone(), expected]);

    // On the left the trigger never fires, on the right it fires in every frame: each channel
    // keeps or takes by its own trigger, in the one row they share.
    let mut by_channel = machine(compile(|| hold(sound::input(), channel()) * 0.5 + 1.0));
    let ramp = |frame: usize| frame as f32 * 0.01;
    let [left, right] = render_in_spans(&mut by_channel, ramp, 3, 24);
    assert_eq!(left, vec![1.0; 3 * MAX_BLOCK]);
    let expected: Vec<f32> = (0..3 * MAX_BLOCK)
        .map(|frame| ramp(frame) * 0.5 + 1.0)
        .collect();
    assert_eq!(right, expected);
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
    let graph = |nodes: serde_json::Value| json!({ "nodes": nodes, "feedbacks": [], "buffers": [], "watches": [], "numbers": [], "bangs": [], "notes": [], "output": { "mono": 0 } });
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
    let bins = json!({ "nodes": [{ "op": "magnitude" }], "feedbacks": [], "captures": [], "output": { "magnitude": 0, "phase": 0 } });
    let spectral =
        json!([{ "op": "input" }, { "op": "spectral", "x": 0, "size": 1000, "bins": bins }]);
    assert_eq!(
        error(graph(spectral)),
        "spectral(x, 1000): the size is a power of two from 256 to 8192"
    );
}

#[test]
fn a_tool_of_one_voice_holds_16_minutes_of_delays_and_buffers_and_each_of_more_voices_2() {
    let compile = |buffer: f32, voices| {
        // A delay that gives no longest holds 4 s.
        let nodes = json!([{ "op": "input" }, { "op": "delay", "x": 0, "ms": 0 }]);
        let graph = json!({ "nodes": nodes, "feedbacks": [], "buffers": [buffer], "watches": [], "numbers": [], "bangs": [], "notes": [], "output": { "mono": 1 } });
        let graph: Graph = serde_json::from_value(graph).unwrap();
        let declarations = Declarations {
            voices,
            ..Declarations::default()
        };
        graph.compile(&declarations).map(|_| ())
    };
    assert!(compile(956.0, 1).is_ok());
    assert!(compile(956.5, 1).is_err());
    assert!(compile(116.0, 8).is_ok());
    assert_eq!(
        compile(116.5, 8).unwrap_err().to_string(),
        "the delays and buffers of a sound hold up to 960 s together, and 120 s in a tool of more voices than one: these hold 120.5 s. Shorten a buffer, or give a delay a shorter longest"
    );
}

#[test]
fn a_sine_through_bins_left_as_they_are_comes_out_as_it_went_in_as_late_as_the_code_says() {
    let through_feedback = compile(|| {
        let late = feedback();
        late.set(spectral(input(), 256, |bin| (bin.magnitude, bin.phase)));
        late.read()
    });
    assert_eq!(through_feedback.latency, 255);
    let code = compile(|| spectral(input(), 1024, |bin| (bin.magnitude, bin.phase)));
    assert_eq!(code.latency, 1023);
    let mut through = machine(code);
    let sine = |frame: usize| 0.5 * (TAU * 440.0 * frame as f32 / SAMPLE_RATE).sin();
    let [left, right] = render_in_spans(&mut through, sine, 100, MAX_BLOCK);
    for (frame, (left, right)) in left.iter().zip(&right).enumerate().skip(1023) {
        let expected = sine(frame - 1023);
        assert!(
            (left - expected).abs() < 1e-4,
            "frame {frame}: {left} for {expected}"
        );
        assert_eq!(left, right, "frame {frame}");
    }
}

#[test]
fn bins_read_a_signal_of_the_sound_around_them() {
    let mut quieter = machine(compile(|| {
        let gain = param("gain", 0.25, [0.0, 1.0]);
        spectral(input(), 256, |bin| {
            (bin.magnitude * capture(gain), bin.phase)
        })
    }));
    let sine = |frame: usize| (TAU * 1000.0 * frame as f32 / SAMPLE_RATE).sin();
    let output = run(&mut quieter, sine, 4_800);
    assert!((loudest(&output[1_000..]) - 0.25).abs() < 1e-3);
}

#[test]
fn a_freeze_holds_each_bin_and_plays_on_after_the_sound_stops() {
    // Frozen after 0.1 s, the sine stops at 0.2 s.
    let freeze = |at: f32| {
        compile(move || {
            let frames = feedback();
            frames.set(frames.read() + 1.0);
            let frozen = 1.0 - frames.read().lt(at);
            spectral(input(), 2048, |bin| {
                let live = 1.0 - capture(frozen);
                let magnitude = hold(bin.magnitude, live);
                let before = feedback();
                before.set(bin.phase);
                let step = hold(bin.phase - before.read(), live);
                let turned = feedback();
                let phase = mix(turned.read() + step, bin.phase, live);
                turned.set(phase);
                (magnitude, phase)
            })
        })
    };
    let sine = |frame: usize| match frame < 9_600 {
        true => 0.5 * (TAU * 440.0 * frame as f32 / SAMPLE_RATE).sin(),
        false => 0.0,
    };
    let after = 24_000..48_000;
    let output = run(&mut machine(freeze(4_800.0)), sine, after.end);
    let held = &output[after.clone()];
    assert!((loudest(held) - 0.5).abs() < 0.05, "{}", loudest(held));
    let crossings = held
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0);
    assert!((219..=221).contains(&crossings.count()), "not 440 Hz");
    let never = run(&mut machine(freeze(f32::MAX)), sine, after.end);
    assert!(loudest(&never[after]) < 1e-4);
}

#[test]
fn bins_of_two_inputs_meet_at_each_frequency_and_the_spectrum_of_the_first_goes_out() {
    let hz = |bin: f32| bin * SAMPLE_RATE / 1024.0;
    let sine = |hz: f32, frame: usize| (TAU * hz * frame as f32 / SAMPLE_RATE).sin();
    // A vocoder's core: the first input goes out where the second sounds.
    let code = compile(move || {
        let other = 0.5 * sin(phasor(hz(64.0)) * TAU);
        spectral_of_two(input(), other, 1024, |bin, other| {
            (bin.magnitude * (1.0 - other.magnitude.lt(0.1)), bin.phase)
        })
    });
    assert_eq!(code.latency, 1023);
    let both = move |frame: usize| sine(hz(32.0), frame) + sine(hz(64.0), frame);
    for span in [1, MAX_BLOCK] {
        let [left, _] = render_in_spans(&mut machine(code.clone()), both, 64, span);
        for (frame, sample) in left.iter().enumerate().skip(2048) {
            let expected = sine(hz(64.0), frame - 1023);
            assert!(
                (sample - expected).abs() < 1e-3,
                "span {span}, frame {frame}: {sample} for {expected}"
            );
        }
    }
    let late_other = compile(|| {
        let other = spectral(input(), 256, |bin| (bin.magnitude, bin.phase));
        spectral_of_two(input(), other, 1024, |bin, _| (bin.magnitude, bin.phase))
    });
    assert_eq!(late_other.latency, 1023 + 255);
}
