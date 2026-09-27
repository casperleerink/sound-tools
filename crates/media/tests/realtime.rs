#![allow(clippy::unwrap_used)]
//! Reading and resampling inside `Engine::process_block`, which the realtime sanitizer checks
//! when the build sets `RTSAN_ENABLE=1`: no allocation, lock or system call.

use std::sync::Arc;

use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, Ports, PrepareConfig, ProcessContext, Processor,
};
use sound_media::{Audio, Resampler, SCRATCH_FRAMES, varispeed};

/// Plays a file from its start through a resampler, as a clip would, or at a speed of its own
/// through the varispeed, as a sampler would.
struct Reader {
    step: Option<f64>,
    audio: Arc<Audio>,
    resampler: Arc<Resampler>,
    scratch: Box<[[f32; 2]]>,
    frames: Box<[[f32; 2]]>,
    played: u64,
}

impl Reader {
    const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Reader {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, (): &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let frames = &mut self.frames[..context.frames];
        match self.step {
            Some(step) => {
                let position = self.played as f64 * step;
                varispeed().render(&self.audio, position, step, frames, &mut self.scratch);
            }
            None => self
                .resampler
                .render(&self.audio, 0, self.played, frames, &mut self.scratch),
        }
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for ((left, right), frame) in left.iter_mut().zip(right.iter_mut()).zip(frames.iter()) {
            *left = frame[0];
            *right = frame[1];
        }
        self.played += context.frames as u64;
    }
}

#[test]
fn reading_and_resampling_run_on_the_audio_thread_without_allocating_locking_or_calling_the_system()
{
    let folder = tempfile::tempdir().unwrap();
    for (rate, bits, format) in [
        (48_000, 16, hound::SampleFormat::Int),
        (44_100, 24, hound::SampleFormat::Int),
        (96_000, 32, hound::SampleFormat::Float),
    ] {
        let path = folder.path().join(format!("{rate}.wav"));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: bits,
            sample_format: format,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for frame in 0..rate {
            let value = ((frame % 200) as f32 / 100.0 - 1.0) * 0.5;
            match format {
                hound::SampleFormat::Float => {
                    writer.write_sample(value).unwrap();
                    writer.write_sample(-value).unwrap();
                }
                hound::SampleFormat::Int => {
                    let scale = (1_i32 << (bits - 1)) as f32;
                    writer.write_sample((value * scale) as i32).unwrap();
                    writer.write_sample((-value * scale) as i32).unwrap();
                }
            }
        }
        writer.finalize().unwrap();
        let audio = Arc::new(Audio::parse(std::fs::read(&path).unwrap()).unwrap());

        for step in [None, Some(1.0), Some(0.37), Some(2.9)] {
            let (mut control, mut engine) = Engine::new(EngineConfig::new(48_000, 2));
            // Made here, on the control side, and never first on the audio thread.
            varispeed();
            let reader = Reader {
                step,
                audio: audio.clone(),
                resampler: Arc::new(Resampler::new(rate, 48_000)),
                scratch: vec![[0.0; 2]; SCRATCH_FRAMES].into_boxed_slice(),
                frames: vec![[0.0; 2]; sound_core::MAX_BLOCK].into_boxed_slice(),
                played: 0,
            };
            let mut edit = control.edit();
            let node = edit.add_processor("reader", reader).unwrap();
            edit.connect(Connection::to_device(node.id(), Reader::OUTPUT, 0))
                .unwrap();
            edit.commit().unwrap();
            let mut output = vec![0.0_f32; 2 * 48_000];
            for buffer in output.chunks_mut(2 * 512) {
                engine.process_block(buffer);
            }
            control.poll().unwrap();
            assert!(
                output[2 * 1_000..].iter().any(|sample| *sample != 0.0),
                "{rate} {step:?}"
            );
        }
    }
}
