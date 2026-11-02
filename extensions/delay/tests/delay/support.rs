//! A rig of one source into one delay into the device, and what the tests measure with.

use std::f64::consts::TAU;

use delay::{Delay, DelayState};
use sound_core::{
    Connection, Engine, EngineConfig, EngineControl, Node, Tempo, TempoMap, TimeSignature,
};
pub(crate) use sound_dsp::testing::{Signal, Source, db, largest_step, noise, peak, rms};

pub(crate) const SAMPLE_RATE: u32 = 48_000;
pub(crate) const SECOND: usize = SAMPLE_RATE as usize;

/// A sine of this frequency and amplitude in both channels, from phase 0, for `frames` frames
/// and then silence. `usize::MAX` plays for ever.
pub(crate) fn sine_for(hz: f64, amplitude: f32, frames: usize, sample_rate: u32) -> Signal {
    let mut phase = 0.0_f64;
    let mut played = 0_usize;
    let step = hz / f64::from(sample_rate);
    Box::new(move || {
        if played >= frames {
            return [0.0; 2];
        }
        played += 1;
        let sample = (TAU * phase).sin() as f32 * amplitude;
        phase = (phase + step).fract();
        [sample; 2]
    })
}

pub(crate) fn sine(hz: f64, amplitude: f32) -> Signal {
    sine_for(hz, amplitude, usize::MAX, SAMPLE_RATE)
}

/// The same noise for `frames` frames, then silence.
pub(crate) fn burst(amplitude: f32, frames: usize) -> Signal {
    let mut sound = noise(amplitude);
    let mut played = 0_usize;
    Box::new(move || {
        played += 1;
        if played <= frames { sound() } else { [0.0; 2] }
    })
}

/// One frame of full scale at each of these frames, in the left channel, and silence between.
pub(crate) fn clicks(at: Vec<usize>) -> Signal {
    let mut frame = 0_usize;
    Box::new(move || {
        let click = at.contains(&frame);
        frame += 1;
        [if click { 1.0 } else { 0.0 }, 0.0]
    })
}

/// One click at the first frame.
pub(crate) fn impulse() -> Signal {
    clicks(vec![0])
}

/// A tempo map of one tempo in 4/4.
pub(crate) fn tempo(bpm: f64) -> TempoMap {
    let four_four = TimeSignature::new(4, 4).unwrap();
    TempoMap::constant(four_four, Tempo::from_bpm(bpm).unwrap())
}

/// A source, a delay and the device, rendering offline. The transport stands at the start, at
/// 120 bpm, until a test says otherwise.
pub(crate) struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub delay: Node<Delay>,
}

impl Rig {
    pub(crate) fn new(state: DelayState, signal: Signal) -> Self {
        Self::at_rate(state, signal, SAMPLE_RATE)
    }

    pub(crate) fn at_rate(state: DelayState, signal: Signal, sample_rate: u32) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(sample_rate, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let delay = edit.add_processor("delay", Delay::new(state)).unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            delay.id(),
            Delay::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(delay.id(), Delay::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            delay,
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

    pub(crate) fn update(&mut self, state: DelayState) {
        self.control.update(self.delay, state).unwrap();
    }
}

/// Only the repeats, with nothing cut: what a test of time and level measures.
pub(crate) fn wet() -> DelayState {
    DelayState {
        low_cut_hz: 20.0,
        high_cut_hz: 20_000.0,
        mix: 1.0,
        ..DelayState::default()
    }
}

/// The frame of the loudest sample in `from..to`.
pub(crate) fn loudest_frame(samples: &[f32], from: usize, to: usize) -> usize {
    let window = &samples[from..to];
    let (index, _) =
        window
            .iter()
            .enumerate()
            .fold((0, 0.0_f32), |(best, level), (index, sample)| {
                if sample.abs() > level {
                    (index, sample.abs())
                } else {
                    (best, level)
                }
            });
    from + index
}
