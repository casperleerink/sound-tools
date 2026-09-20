//! Any valid state with any notes gives finite, bounded output, also when the state changes
//! while notes are held.

use std::sync::Arc;

use instrument::{Synth, SynthState, Waveform};
use proptest::prelude::*;
use sound_core::{Connection, Engine, EngineConfig, State};
use sound_notes::Note;

use crate::support::{Sequencer, note, peak};

/// The ends of a range come up often, because that is where a filter or an envelope breaks.
fn within(low: f32, high: f32) -> impl Strategy<Value = f32> {
    prop_oneof![Just(low), Just(high), low..=high]
}

fn any_state() -> impl Strategy<Value = SynthState> {
    let waveform = prop_oneof![Just(Waveform::Saw), Just(Waveform::Square)];
    let filter = (within(20.0, 20_000.0), within(0.0, 1.0));
    let times = (
        within(0.001, 10.0),
        within(0.001, 10.0),
        within(0.001, 10.0),
    );
    (waveform, filter, times, within(0.0, 1.0), within(0.0, 1.0)).prop_map(
        |(waveform, (cutoff_hz, resonance), (attack, decay, release), sustain, gain)| SynthState {
            waveform,
            cutoff_hz,
            resonance,
            attack_seconds: attack,
            decay_seconds: decay,
            sustain,
            release_seconds: release,
            gain,
        },
    )
}

/// Inside the first four seconds at 120 bpm, which is what the test renders.
fn any_note() -> impl Strategy<Value = Note> {
    (0..7_000_u64, 1..4_000_u64, 0..=127_u8, 1..=127_u8)
        .prop_map(|(start, length, pitch, velocity)| note(start, length, pitch, velocity))
}

/// 16 voices at full gain, each with the filter peak of 16 on its loudest partial, stay far
/// below this. Output that runs away does not.
const BOUND: f32 = 1_000.0;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn output_is_finite_and_bounded(
        first in any_state(),
        second in any_state(),
        notes in prop::collection::vec(any_note(), 1..48),
        sample_rate in prop::sample::select(vec![16_000_u32, 44_100, 48_000, 96_000]),
    ) {
        prop_assert_eq!(first.validate(), Ok(()));
        prop_assert_eq!(second.validate(), Ok(()));
        let (mut control, mut engine) = Engine::new(EngineConfig::new(sample_rate, 1));
        let mut edit = control.edit();
        let sequencer = edit.add_processor("sequencer", Sequencer::default()).unwrap();
        let synth = edit.add_processor("synth", Synth::new(first)).unwrap();
        edit.connect(Connection::new(sequencer.id(), Sequencer::NOTES, synth.id(), Synth::NOTES)).unwrap();
        edit.connect(Connection::to_device(synth.id(), Synth::OUTPUT, 0)).unwrap();
        edit.update(sequencer, Arc::new(notes)).unwrap();
        edit.commit().unwrap();
        control.play();

        let mut output = vec![0.0; 4 * sample_rate as usize];
        let (before, after) = output.split_at_mut(sample_rate as usize * 3 / 2);
        before.chunks_mut(512).for_each(|buffer| engine.process_block(buffer));
        control.update(synth, second).unwrap();
        after.chunks_mut(512).for_each(|buffer| engine.process_block(buffer));

        prop_assert!(output.iter().all(|sample| sample.is_finite()));
        prop_assert!(peak(&output) < BOUND, "peak {}", peak(&output));
        let status = control.poll().unwrap();
        prop_assert_eq!(status.event_overflows, 0);
        prop_assert_eq!(status.port_misuses, 0);
    }
}
