//! A rig of a track and a voice into one vocoder into the device, and what the tests measure
//! with.

use std::f64::consts::TAU;

use sound_core::{Connection, Engine, EngineConfig, EngineControl, Node};
pub(crate) use sound_dsp::testing::{Signal, Source, noise};
use vocoder::{Vocoder, VocoderState};

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// Sines of these frequencies and amplitudes, added, in both channels.
pub(crate) fn sines(parts: &[(f64, f32)]) -> Signal {
    let parts = parts.to_vec();
    let mut frame = 0_usize;
    Box::new(move || {
        let time = frame as f64 / f64::from(SAMPLE_RATE);
        let sample = (parts.iter())
            .map(|(hz, amplitude)| (TAU * hz * time).sin() as f32 * amplitude)
            .sum();
        frame += 1;
        [sample; 2]
    })
}

pub(crate) fn silence() -> Signal {
    Box::new(|| [0.0; 2])
}

/// A track, a vocoder and the device, rendering offline.
pub(crate) struct Rig {
    control: EngineControl,
    engine: Engine,
    vocoder: Node<Vocoder>,
}

impl Rig {
    pub(crate) fn new(state: VocoderState, track: Signal) -> Self {
        let (mut control, engine) =
            Engine::new(EngineConfig::new(SAMPLE_RATE, 2).rendering_offline());
        let mut edit = control.edit();
        let source = edit.add_processor("track", Source::new(track)).unwrap();
        let vocoder = edit.add_processor("vocoder", Vocoder::new(state)).unwrap();
        let input = Connection::new(source.id(), Source::OUTPUT, vocoder.id(), Vocoder::INPUT);
        edit.connect(input).unwrap();
        edit.connect(Connection::to_device(vocoder.id(), Vocoder::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            vocoder,
        }
    }

    /// Feeds `voice` to the sidechain.
    pub(crate) fn voice(mut self, voice: Signal) -> Self {
        let mut edit = self.control.edit();
        let voice = edit.add_processor("voice", Source::new(voice)).unwrap();
        let sidechain = Connection::new(
            voice.id(),
            Source::OUTPUT,
            self.vocoder.id(),
            Vocoder::SIDECHAIN,
        );
        edit.connect(sidechain).unwrap();
        edit.commit().unwrap();
        self
    }

    /// The left channel, in device buffers of 480 frames.
    pub(crate) fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(480 * 2) {
            self.engine.process_block(buffer);
        }
        output.iter().step_by(2).copied().collect()
    }
}

/// The amplitude of the sine at `hz` in `samples`, which hold a whole number of its cycles.
pub(crate) fn amplitude(samples: &[f32], hz: f64) -> f32 {
    let (mut sine, mut cosine) = (0.0, 0.0);
    for (frame, sample) in samples.iter().enumerate() {
        let phase = TAU * hz * frame as f64 / f64::from(SAMPLE_RATE);
        sine += f64::from(*sample) * phase.sin();
        cosine += f64::from(*sample) * phase.cos();
    }
    (2.0 * sine.hypot(cosine) / samples.len() as f64) as f32
}
