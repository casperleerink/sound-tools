//! The Tone processor rendered offline through the engine. Asserts on samples.

use sound_core::{Connection, Engine, EngineConfig, EngineControl, Node};
use tone::{Tone, ToneState};

use crate::support::{
    SAMPLE_RATE, assert_continuous, channel, largest_step, peak, render, rising_zero_crossings,
};

fn tone_on_channel(
    control: &mut EngineControl,
    name: &str,
    parameters: ToneState,
    channel: usize,
) -> Node<Tone> {
    let mut edit = control.edit();
    let node = edit.add_processor(name, Tone::new(parameters)).unwrap();
    edit.connect(Connection::to_device(node.id(), Tone::OUTPUT, channel))
        .unwrap();
    edit.commit().unwrap();
    node
}

#[test]
fn frequency_and_gain_are_what_the_parameters_say() {
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
    let parameters = ToneState {
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
    let before = ToneState {
        frequency_hz: 220.0,
        gain: 0.5,
    };
    let after = ToneState {
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
    let loud = ToneState {
        frequency_hz: 1_000.0,
        gain: 0.5,
    };
    let tone = tone_on_channel(&mut control, "tone", loud, 0);
    render(&mut engine, 1_000);
    control
        .update(
            tone,
            ToneState {
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
    let first = ToneState {
        frequency_hz: 220.0,
        gain: 0.5,
    };
    let second = ToneState {
        frequency_hz: 330.0,
        gain: 0.25,
    };

    // Four device channels, so each Tone has a stereo pair of its own.
    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 4));
    tone_on_channel(&mut control, "first", first, 0);
    let undisturbed = channel(&render(&mut engine, 30_000), 0, 4);

    let (mut control, mut engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 4));
    tone_on_channel(&mut control, "first", first, 0);
    let mut output = render(&mut engine, 10_001);
    let added = tone_on_channel(&mut control, "second", second, 2);
    output.extend(render(&mut engine, 9_999));
    let mut edit = control.edit();
    edit.remove_processor(added.id()).unwrap();
    edit.commit().unwrap();
    output.extend(render(&mut engine, 10_000));

    // Two schedule swaps happened. The surviving Tone's samples are bit for bit the same.
    assert_eq!(channel(&output, 0, 4), undisturbed);
    let other = channel(&output, 2, 4);
    assert_eq!(peak(&other[..10_001]), 0.0);
    assert!(peak(&other[10_001..20_000]) > 0.2499);
    assert_eq!(peak(&other[20_000..]), 0.0);
    assert_eq!(control.poll().unwrap().batches_applied, 3);
}
