//! Tone rendered offline through the engine. Asserts on samples.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

use sound_core::{Connection, Engine, EngineConfig, EngineControl, Node};
use tone::{Tone, ToneParameters};

const SAMPLE_RATE: u32 = 48_000;

fn tone_on_channel(
    control: &mut EngineControl,
    name: &str,
    parameters: ToneParameters,
    channel: usize,
) -> Node<Tone> {
    let mut edit = control.edit();
    let node = edit.add_processor(name, Tone::new(parameters)).unwrap();
    edit.connect(Connection::to_device(node.id(), Tone::OUTPUT, channel))
        .unwrap();
    edit.commit().unwrap();
    node
}

/// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * engine.channels()];
    for buffer in output.chunks_mut(480 * engine.channels()) {
        engine.process_block(buffer);
    }
    output
}

fn channel(interleaved: &[f32], channel: usize, channels: usize) -> Vec<f32> {
    interleaved
        .iter()
        .skip(channel)
        .step_by(channels)
        .copied()
        .collect()
}

fn rising_zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
        .count()
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// The largest step a sine of this frequency and gain can take between two samples.
fn largest_step(frequency_hz: f32, gain: f32) -> f32 {
    gain * std::f32::consts::TAU * frequency_hz / SAMPLE_RATE as f32
}

fn assert_continuous(samples: &[f32], limit: f32) {
    for (frame, pair) in samples.windows(2).enumerate() {
        let step = (pair[1] - pair[0]).abs();
        assert!(
            step <= limit * 1.001,
            "jump of {step} after frame {frame}, limit {limit}"
        );
    }
}

#[test]
fn frequency_and_gain_are_what_the_parameters_say() {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let parameters = ToneParameters {
        frequency_hz: 440.0,
        gain: 0.25,
    };
    tone_on_channel(&mut control, "tone", parameters, 0);

    let second = render(&mut engine, SAMPLE_RATE as usize);
    assert!((439..=441).contains(&rising_zero_crossings(&second)));
    assert!(
        (0.2499..=0.25).contains(&peak(&second)),
        "peak {}",
        peak(&second)
    );
}

#[test]
fn phase_continues_across_a_parameter_change() {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let before = ToneParameters {
        frequency_hz: 220.0,
        gain: 0.5,
    };
    let after = ToneParameters {
        frequency_hz: 330.0,
        gain: 0.5,
    };
    let tone = tone_on_channel(&mut control, "tone", before, 0);

    // 12 345 frames ends mid-cycle, where a phase reset would show as a jump.
    let mut samples = render(&mut engine, 12_345);
    assert!(samples.last().unwrap().abs() > 0.1);
    control.update(tone, after).unwrap();
    let changed = render(&mut engine, SAMPLE_RATE as usize);
    samples.extend(&changed);

    assert_continuous(&samples, largest_step(330.0, 0.5));
    assert!((329..=331).contains(&rising_zero_crossings(&changed)));
}

#[test]
fn gain_changes_at_the_next_block_start() {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let loud = ToneParameters {
        frequency_hz: 1_000.0,
        gain: 0.5,
    };
    let tone = tone_on_channel(&mut control, "tone", loud, 0);
    render(&mut engine, 1_000);
    control
        .update(
            tone,
            ToneParameters {
                gain: 0.125,
                ..loud
            },
        )
        .unwrap();
    let quiet = render(&mut engine, 1_000);
    assert!(
        (0.1249..=0.125).contains(&peak(&quiet)),
        "peak {}",
        peak(&quiet)
    );
}

#[test]
fn phase_continues_across_schedule_swaps() {
    let first = ToneParameters {
        frequency_hz: 220.0,
        gain: 0.5,
    };
    let second = ToneParameters {
        frequency_hz: 330.0,
        gain: 0.25,
    };

    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    tone_on_channel(&mut control, "first", first, 0);
    let undisturbed = channel(&render(&mut engine, 30_000), 0, 2);

    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
    tone_on_channel(&mut control, "first", first, 0);
    let mut output = render(&mut engine, 10_001);
    let added = tone_on_channel(&mut control, "second", second, 1);
    output.extend(render(&mut engine, 9_999));
    let mut edit = control.edit();
    edit.remove_processor(added.id()).unwrap();
    edit.commit().unwrap();
    output.extend(render(&mut engine, 10_000));

    // Two schedule swaps happened. The surviving Tone's samples are bit for bit the same.
    assert_eq!(channel(&output, 0, 2), undisturbed);
    let other = channel(&output, 1, 2);
    assert_eq!(peak(&other[..10_001]), 0.0);
    assert!(peak(&other[10_001..20_000]) > 0.2499);
    assert_eq!(peak(&other[20_000..]), 0.0);
    assert_eq!(control.poll().unwrap().batches_applied, 3);
}
