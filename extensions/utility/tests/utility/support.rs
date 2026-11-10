//! A rig of one source into one utility into the device, and what the tests measure with.

use std::f64::consts::TAU;

use sound_core::{Connection, Engine, EngineConfig, EngineControl, Node};
pub(crate) use sound_dsp::testing::{Signal, Source, amplitude_at, largest_step, noise, peak};
use utility::{Utility, UtilityState};

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// A sine of this frequency, from phase 0, with its amplitude on the left and on the right. A
/// negative amplitude is the sine upside down.
pub(crate) fn sine(hz: f64, amplitude: [f32; 2]) -> Signal {
    sine_at(hz, amplitude, SAMPLE_RATE)
}

/// The same at another sample rate.
pub(crate) fn sine_at(hz: f64, [left, right]: [f32; 2], sample_rate: u32) -> Signal {
    let mut phase = 0.0_f64;
    let step = hz / f64::from(sample_rate);
    Box::new(move || {
        let sample = (TAU * phase).sin() as f32;
        phase = (phase + step).fract();
        [sample * left, sample * right]
    })
}

/// A sine of one frequency on the left and one of another on the right, so every channel can be
/// told apart.
pub(crate) fn two_sines(left_hz: f64, right_hz: f64, amplitude: f32) -> Signal {
    let [mut left, mut right] = [
        sine(left_hz, [amplitude; 2]),
        sine(right_hz, [amplitude; 2]),
    ];
    Box::new(move || [left()[0], right()[0]])
}

/// The first `frames` of a signal, left and right: what the utility hears.
pub(crate) fn frames_of(mut signal: Signal, frames: usize) -> [Vec<f32>; 2] {
    let frames: Vec<[f32; 2]> = (0..frames).map(|_| signal()).collect();
    [0, 1].map(|channel| frames.iter().map(|frame| frame[channel]).collect())
}

/// A source, a utility and the device, rendering offline.
pub(crate) struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub utility: Node<Utility>,
}

impl Rig {
    pub(crate) fn new(state: UtilityState, signal: Signal) -> Self {
        Self::at_rate(state, signal, SAMPLE_RATE)
    }

    pub(crate) fn at_rate(state: UtilityState, signal: Signal, sample_rate: u32) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let utility = edit.add_processor("utility", Utility::new(state)).unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            utility.id(),
            Utility::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(utility.id(), Utility::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            utility,
        }
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub(crate) fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(480 * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    pub(crate) fn update(&mut self, state: UtilityState) {
        self.control.update(self.utility, state).unwrap();
    }
}

/// The amplitude of a sine through a utility on the left and on the right, once it has settled.
pub(crate) fn measured(state: UtilityState, hz: f64, input: [f32; 2]) -> [f64; 2] {
    measured_at(state, hz, input, SAMPLE_RATE)
}

/// The same at another sample rate.
pub(crate) fn measured_at(
    state: UtilityState,
    hz: f64,
    input: [f32; 2],
    sample_rate: u32,
) -> [f64; 2] {
    // Long enough for the slowest crossover to settle, then whole cycles over about a quarter
    // of a second, so the correlation sees no part of a cycle.
    let settle = sample_rate as usize;
    let cycles = (hz * 0.25).ceil();
    let window = (cycles * f64::from(sample_rate) / hz).round() as usize;
    let mut rig = Rig::at_rate(state, sine_at(hz, input, sample_rate), sample_rate);
    rig.render(settle);
    rig.render(window)
        .map(|channel| amplitude_at(&channel, settle, hz, sample_rate))
}
