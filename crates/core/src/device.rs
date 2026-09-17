use crate::{Error, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub fn audio_available() -> bool {
    cpal::default_host().default_output_device().is_some()
}

fn fill_output(
    consumer: &mut Consumer<[f32; 2]>,
    started: bool,
    output: &mut [f32],
    channels: usize,
) -> (u64, u64) {
    if !started {
        output.fill(0.0);
        return (0, 0);
    }
    let mut missing = 0;
    for frame in output.chunks_mut(channels) {
        let stereo = consumer.pop().unwrap_or_else(|_| {
            missing += 1;
            [0.0; 2]
        });
        frame.fill(0.0);
        frame[0] = stereo[0].clamp(-1.0, 1.0);
        frame[1] = stereo[1].clamp(-1.0, 1.0);
    }
    let served = output.len() as u64 / channels as u64;
    (missing, served)
}

pub struct DeviceOutput {
    stream: cpal::Stream,
    started: Arc<AtomicBool>,
    producer: Producer<[f32; 2]>,
    underruns: Arc<AtomicU64>,
    frames_served: Arc<AtomicU64>,
    sample_rate: u32,
    name: String,
}

impl DeviceOutput {
    pub fn open() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| Error("No default audio output device".into()))?;
        let name = device.name().map_err(|error| Error(error.to_string()))?;
        let supported = device
            .supported_output_configs()
            .map_err(|error| Error(error.to_string()))?
            .find(|config| {
                config.sample_format() == cpal::SampleFormat::F32
                    && config.channels() >= 2
                    && config.min_sample_rate().0 <= 48_000
                    && config.max_sample_rate().0 >= 48_000
            })
            .ok_or_else(|| Error("Output needs stereo float32 support at 48000 Hz".into()))?;
        let config = supported
            .with_sample_rate(cpal::SampleRate(48_000))
            .config();
        let channels = usize::from(config.channels);
        let (producer, mut consumer) = RingBuffer::<[f32; 2]>::new(8192);
        let underruns = Arc::new(AtomicU64::new(0));
        let frames_served = Arc::new(AtomicU64::new(0));
        let callback_underruns = underruns.clone();
        let callback_served = frames_served.clone();
        let started = Arc::new(AtomicBool::new(false));
        let callback_started = started.clone();
        let stream = device
            .build_output_stream(
                &config,
                move |output: &mut [f32], _| {
                    let (missing, served) = fill_output(
                        &mut consumer,
                        callback_started.load(Ordering::Acquire),
                        output,
                        channels,
                    );
                    callback_underruns.fetch_add(missing, Ordering::Relaxed);
                    callback_served.fetch_add(served, Ordering::Relaxed);
                },
                |_| {},
                None,
            )
            .map_err(|error| Error(error.to_string()))?;
        Ok(Self {
            stream,
            started,
            producer,
            underruns,
            frames_served,
            sample_rate: config.sample_rate.0,
            name,
        })
    }
    pub fn start(&self) -> Result<()> {
        self.stream
            .play()
            .map_err(|error| Error(error.to_string()))?;
        self.started.store(true, Ordering::Release);
        Ok(())
    }
    pub fn free_frames(&self) -> usize {
        self.producer.slots()
    }
    pub fn write(&mut self, left: &[f32], right: &[f32]) -> Result<()> {
        if left.len() != right.len() || left.len() > self.free_frames() {
            return Err(Error("Audio output buffer has insufficient space".into()));
        }
        for (&left, &right) in left.iter().zip(right) {
            self.producer
                .push([left, right])
                .map_err(|_| Error("Audio output buffer is full".into()))?;
        }
        Ok(())
    }
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
    pub fn frames_served(&self) -> u64 {
        self.frames_served.load(Ordering::Relaxed)
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_gate_does_not_read_ring_or_count_before_start() {
        let (mut producer, mut consumer) = RingBuffer::<[f32; 2]>::new(8);
        for frame in 0..3u32 {
            producer.push([((frame + 1) as f32) / 4.0, 0.0]).unwrap();
        }
        let mut output = [0.0f32; 4 * 2];
        let (missing, served) = fill_output(&mut consumer, false, &mut output, 2);
        assert_eq!((missing, served), (0, 0));
        assert_eq!(producer.slots(), 5);
        assert_eq!(output, [0.0; 8]);
        let (missing, served) = fill_output(&mut consumer, true, &mut output, 2);
        assert_eq!((missing, served), (1, 4));
        assert_eq!(producer.slots(), 8);
        assert_eq!(output, [0.25, 0.0, 0.5, 0.0, 0.75, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn callback_reports_missing_frames_after_start_on_empty_ring() {
        let (producer, mut consumer) = RingBuffer::<[f32; 2]>::new(8);
        assert_eq!(producer.slots(), 8);
        let mut output = [0.0f32; 2 * 2];
        let (missing, served) = fill_output(&mut consumer, true, &mut output, 2);
        assert_eq!((missing, served), (2, 2));
    }
}
