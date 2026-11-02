//! A rig of one source into one compressor into the device, and what the tests measure with.

use compressor::{Compressor, CompressorState, Meters};
use sound_core::{Connection, Engine, EngineConfig, EngineControl, Node};
pub(crate) use sound_dsp::testing::{Signal, Source, largest_step, noise, peak, sine_at};

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// A sine of this frequency and amplitude in both channels, from phase 0.
pub(crate) fn sine(hz: f64, amplitude: f32) -> Signal {
    sine_at(hz, amplitude, SAMPLE_RATE)
}

/// A steady value in both channels that changes to another at a frame: the cleanest level
/// step, because the level of a constant is the constant and the gain is the output over it.
pub(crate) fn step(before: f32, at: usize, after: f32) -> Signal {
    let mut frame = 0_usize;
    Box::new(move || {
        let value = if frame < at { before } else { after };
        frame += 1;
        [value; 2]
    })
}

/// Silence for `frames`, then another signal from its start.
pub(crate) fn after_silence(frames: usize, mut then: Signal) -> Signal {
    let mut frame = 0_usize;
    Box::new(move || {
        frame += 1;
        match frame > frames {
            true => then(),
            false => [0.0; 2],
        }
    })
}

/// A source, a compressor and the device, rendering offline.
pub(crate) struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub compressor: Node<Compressor>,
    /// What the card of this compressor would read.
    pub meters: Meters,
}

impl Rig {
    pub(crate) fn new(state: CompressorState, signal: Signal) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(SAMPLE_RATE, 2).rendering_offline());
        let mut edit = control.edit();
        let meters = Meters::default();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let compressor = edit
            .add_processor("compressor", Compressor::new(state, meters.clone()))
            .unwrap();
        edit.connect(Connection::new(
            source.id(),
            Source::OUTPUT,
            compressor.id(),
            Compressor::INPUT,
        ))
        .unwrap();
        edit.connect(Connection::to_device(
            compressor.id(),
            Compressor::OUTPUT,
            0,
        ))
        .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            compressor,
            meters,
        }
    }

    /// Adds a source of `key` and gives the connection that feeds it to the sidechain, not yet
    /// made.
    pub(crate) fn add_key(&mut self, key: Signal) -> Connection {
        let mut edit = self.control.edit();
        let key = edit.add_processor("key", Source::new(key)).unwrap();
        edit.commit().unwrap();
        Connection::new(
            key.id(),
            Source::OUTPUT,
            self.compressor.id(),
            Compressor::SIDECHAIN,
        )
    }

    pub(crate) fn connect(&mut self, connection: Connection) {
        let mut edit = self.control.edit();
        edit.connect(connection).unwrap();
        edit.commit().unwrap();
    }

    pub(crate) fn disconnect(&mut self, connection: Connection) {
        let mut edit = self.control.edit();
        edit.disconnect(&connection).unwrap();
        edit.commit().unwrap();
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

    /// The same in device buffers of `block` frames.
    pub(crate) fn render_in_blocks(&mut self, frames: usize, block: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(block * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    pub(crate) fn update(&mut self, state: CompressorState) {
        self.control.update(self.compressor, state).unwrap();
    }
}

/// The amplitude of the part of `samples` at `hz`, by correlation with a sine and a cosine of
/// that frequency. `samples` start at frame `start` of a signal whose phase was 0 at frame 0.
pub(crate) fn amplitude_at(samples: &[f32], start: usize, hz: f64) -> f64 {
    sound_dsp::testing::amplitude_at(samples, start, hz, SAMPLE_RATE)
}

/// The gain of a compressor with this record on a steady sine of this peak level, measured once
/// it has settled, in dB. The settling is two seconds: every release of these tests is
/// shorter than a tenth of that.
pub(crate) fn measured_gain_db(state: CompressorState, hz: f64, level_db: f32) -> f64 {
    let amplitude = 10_f32.powf(level_db / 20.0);
    let mut rig = Rig::new(state, sine(hz, amplitude));
    let settle = 2 * SAMPLE_RATE as usize;
    rig.render(settle);
    // Whole cycles over a quarter of a second, so the correlation sees no part of a cycle.
    let window = ((hz * 0.25).ceil() * f64::from(SAMPLE_RATE) / hz).round() as usize;
    let [left, right] = rig.render(window);
    assert_eq!(left, right);
    let latency = state.lookahead.frames(SAMPLE_RATE as f32);
    let measured = amplitude_at(&left, settle - latency, hz);
    20.0 * (measured / f64::from(amplitude)).log10()
}
