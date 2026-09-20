//! Rendering and sample checks shared by the Tone tests.

use sound_core::Engine;

pub const SAMPLE_RATE: u32 = 48_000;

/// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
pub fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * engine.channels()];
    for buffer in output.chunks_mut(480 * engine.channels()) {
        engine.process_block(buffer);
    }
    output
}

pub fn channel(interleaved: &[f32], channel: usize, channels: usize) -> Vec<f32> {
    interleaved
        .iter()
        .skip(channel)
        .step_by(channels)
        .copied()
        .collect()
}

pub fn rising_zero_crossings(samples: &[f32]) -> usize {
    samples
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
        .count()
}

pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// The largest step a sine of this frequency and gain can take between two samples.
pub fn largest_step(frequency_hz: f32, gain: f32) -> f32 {
    gain * std::f32::consts::TAU * frequency_hz / SAMPLE_RATE as f32
}

pub fn assert_continuous(samples: &[f32], limit: f32) {
    for (frame, pair) in samples.windows(2).enumerate() {
        let step = (pair[1] - pair[0]).abs();
        assert!(
            step <= limit * 1.001,
            "jump of {step} after frame {frame}, limit {limit}"
        );
    }
}
