//! A rig of one source into one gate into the device, and what the tests measure with.

use std::f64::consts::TAU;

use gate::{Gate, GateState, Meters};
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, Node, Ports, PrepareConfig,
    ProcessContext, Processor,
};

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// Makes the next frame of a signal, left and right. Called on the audio thread.
pub(crate) type Signal = Box<dyn FnMut() -> [f32; 2] + Send>;

/// A processor that plays a signal.
pub(crate) struct Source {
    signal: Signal,
}

impl Source {
    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub(crate) fn new(signal: Signal) -> Self {
        Self { signal }
    }
}

impl Processor for Source {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            [*left, *right] = (self.signal)();
        }
    }
}

pub(crate) fn decibels(db: f32) -> f32 {
    10_f32.powf(db / 20.0)
}

/// A sine in both channels, from phase 0, with the amplitude `amplitude` gives for each frame.
pub(crate) fn shaped_sine(
    hz: f64,
    mut amplitude: impl FnMut(usize) -> f32 + Send + 'static,
) -> Signal {
    let mut frame = 0_usize;
    Box::new(move || {
        let time = frame as f64 / f64::from(SAMPLE_RATE);
        let sample = (TAU * hz * time).sin() as f32 * amplitude(frame);
        frame += 1;
        [sample; 2]
    })
}

pub(crate) fn sine(hz: f64, amplitude: f32) -> Signal {
    shaped_sine(hz, move |_| amplitude)
}

/// A drum hit every `period` frames: a 200 Hz sine that starts at -6 dBFS and falls by 60 dB
/// in 300 ms.
pub(crate) fn hits(period: usize) -> Signal {
    shaped_sine(200.0, move |frame| {
        let seconds = (frame % period) as f32 / SAMPLE_RATE as f32;
        0.5 * decibels(-200.0 * seconds)
    })
}

/// White noise from -amplitude to amplitude, the same every run, other in each channel.
pub(crate) fn noise(amplitude: f32) -> Signal {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    Box::new(move || [next() * amplitude, next() * amplitude])
}

/// A source, a gate and the device, rendering offline.
pub(crate) struct Rig {
    control: EngineControl,
    engine: Engine,
    gate: Node<Gate>,
}

impl Rig {
    pub(crate) fn new(state: GateState, signal: Signal) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(SAMPLE_RATE, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("source", Source::new(signal)).unwrap();
        let gate = edit
            .add_processor("gate", Gate::new(state, Meters::default()))
            .unwrap();
        let input = Connection::new(source.id(), Source::OUTPUT, gate.id(), Gate::INPUT);
        edit.connect(input).unwrap();
        edit.connect(Connection::to_device(gate.id(), Gate::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            gate,
        }
    }

    /// Feeds `key` to the sidechain.
    pub(crate) fn key(&mut self, key: Signal) {
        let mut edit = self.control.edit();
        let key = edit.add_processor("key", Source::new(key)).unwrap();
        let sidechain = Connection::new(key.id(), Source::OUTPUT, self.gate.id(), Gate::SIDECHAIN);
        edit.connect(sidechain).unwrap();
        edit.commit().unwrap();
    }

    /// Renders in device buffers of `block` frames. Left and right.
    pub(crate) fn render_in_blocks(&mut self, frames: usize, block: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(block * 2) {
            self.engine.process_block(buffer);
        }
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    /// The left channel, in device buffers of 480 frames.
    pub(crate) fn render(&mut self, frames: usize) -> Vec<f32> {
        let [left, _] = self.render_in_blocks(frames, 480);
        left
    }
}

/// What a signal plays with no gate, on the left.
pub(crate) fn dry(mut signal: Signal, frames: usize) -> Vec<f32> {
    (0..frames).map(|_| signal()[0]).collect()
}

pub(crate) fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}
