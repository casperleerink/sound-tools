//! Sources of sound into analyzers into the device, rendering offline.

use analyzer::Analyzer;
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, Ports, PrepareConfig, ProcessContext, Processor,
    Scope,
};

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// Plays the frames it is given, then silence.
pub(crate) struct Source {
    frames: Vec<[f32; 2]>,
    next: usize,
}

impl Source {
    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);
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
            if let Some(frame) = self.frames.get(self.next) {
                [*left, *right] = *frame;
            }
            self.next += 1;
        }
    }
}

/// White noise from -amplitude to amplitude, the same every run for a seed.
pub(crate) fn noise(amplitude: f32, seed: u64, frames: usize) -> Vec<[f32; 2]> {
    let mut state = 0x2545_f491_4f6c_dd1d_u64 ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * amplitude
    };
    (0..frames).map(|_| [next(), next()]).collect()
}

/// `count` sources, each through an analyzer into the device, and the scope of each analyzer.
pub(crate) fn rig(count: usize, sound: impl Fn(usize) -> Vec<[f32; 2]>) -> (Engine, Vec<Scope>) {
    let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2).rendering_offline());
    let mut edit = control.edit();
    let mut scopes = Vec::new();
    for index in 0..count {
        let source = Source {
            frames: sound(index),
            next: 0,
        };
        let source = edit
            .add_processor(&format!("source-{index}"), source)
            .unwrap();
        let scope = Scope::new();
        let analyzer = edit
            .add_processor(&format!("analyzer-{index}"), Analyzer::new(scope.clone()))
            .unwrap();
        let input = Connection::new(source.id(), Source::OUTPUT, analyzer.id(), Analyzer::INPUT);
        edit.connect(input).unwrap();
        edit.connect(Connection::to_device(analyzer.id(), Analyzer::OUTPUT, 0))
            .unwrap();
        scopes.push(scope);
    }
    edit.commit().unwrap();
    (engine, scopes)
}

/// `frames` frames in device buffers of 480 frames, so short sub-blocks are part of it.
pub(crate) fn render(engine: &mut Engine, frames: usize) -> Vec<[f32; 2]> {
    let mut output = vec![0.0; frames * 2];
    for buffer in output.chunks_mut(480 * 2) {
        engine.process_block(buffer);
    }
    output.as_chunks::<2>().0.to_vec()
}
