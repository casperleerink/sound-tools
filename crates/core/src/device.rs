//! Device output through cpal. A thin wrapper: open the default output, hand it an engine.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::clock::MIN_EXACT_SAMPLE_RATE;
use crate::engine::Engine;

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("no default audio output device")]
    NoOutputDevice,
    #[error("the output device wants {0} samples, only f32 is supported")]
    UnsupportedSampleFormat(cpal::SampleFormat),
    #[error("the output device runs at {0} Hz, the clock needs {MIN_EXACT_SAMPLE_RATE} Hz or more")]
    SampleRateTooLow(u32),
    #[error("the engine has {engine} channels, the device has {device}")]
    ChannelMismatch { engine: usize, device: usize },
    #[error(transparent)]
    Backend(#[from] cpal::Error),
}

/// The default output device and its default configuration.
pub struct OutputDevice {
    device: cpal::Device,
    config: cpal::StreamConfig,
}

impl OutputDevice {
    pub fn default_output() -> Result<Self, DeviceError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(DeviceError::NoOutputDevice)?;
        let supported = device.default_output_config()?;
        if supported.sample_format() != cpal::SampleFormat::F32 {
            return Err(DeviceError::UnsupportedSampleFormat(
                supported.sample_format(),
            ));
        }
        let config = supported.config();
        if config.sample_rate < MIN_EXACT_SAMPLE_RATE {
            return Err(DeviceError::SampleRateTooLow(config.sample_rate));
        }
        Ok(Self { device, config })
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    pub fn channels(&self) -> usize {
        usize::from(self.config.channels)
    }

    /// Moves the engine into the device callback and starts playback.
    pub fn start(self, mut engine: Engine) -> Result<OutputStream, DeviceError> {
        if engine.channels() != self.channels() {
            return Err(DeviceError::ChannelMismatch {
                engine: engine.channels(),
                device: self.channels(),
            });
        }
        let counters = Arc::new(Counters::default());
        let (error_sender, errors) = mpsc::channel();
        let channels = self.channels().max(1) as u64;
        let sample_rate = u64::from(self.sample_rate().max(1));

        let stream = self.device.build_output_stream(
            self.config,
            {
                let counters = counters.clone();
                move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let started = Instant::now();
                    engine.process_block(output);
                    let elapsed = started.elapsed();
                    let frames = output.len() as u64 / channels;
                    if elapsed > Duration::from_nanos(frames * 1_000_000_000 / sample_rate) {
                        counters.late_callbacks.fetch_add(1, Ordering::Relaxed);
                    }
                    let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
                    counters
                        .slowest_callback_nanos
                        .fetch_max(nanos, Ordering::Relaxed);
                }
            },
            {
                let counters = counters.clone();
                // cpal calls this from a thread of its own, not from the audio callback.
                move |error: cpal::Error| {
                    if error.kind() == cpal::ErrorKind::Xrun {
                        counters.xruns.fetch_add(1, Ordering::Relaxed);
                    } else if error_sender.send(error).is_err() {
                        // The stream owner is gone, so nobody is left to tell.
                    }
                }
            },
            None,
        )?;
        stream.play()?;
        Ok(OutputStream {
            _stream: stream,
            counters,
            errors,
        })
    }
}

#[derive(Default)]
struct Counters {
    xruns: AtomicU64,
    late_callbacks: AtomicU64,
    slowest_callback_nanos: AtomicU64,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceStatus {
    /// Overloads reported by the device.
    pub xruns: u64,
    /// Callbacks that took longer to render than the time their buffer plays for.
    pub late_callbacks: u64,
    pub slowest_callback: Duration,
}

/// A playing stream. Dropping it stops playback and drops the engine.
pub struct OutputStream {
    _stream: cpal::Stream,
    counters: Arc<Counters>,
    errors: mpsc::Receiver<cpal::Error>,
}

impl OutputStream {
    pub fn status(&self) -> DeviceStatus {
        DeviceStatus {
            xruns: self.counters.xruns.load(Ordering::Relaxed),
            late_callbacks: self.counters.late_callbacks.load(Ordering::Relaxed),
            slowest_callback: Duration::from_nanos(
                self.counters.slowest_callback_nanos.load(Ordering::Relaxed),
            ),
        }
    }

    /// Stream errors other than xruns since the last call, for example a lost device.
    pub fn take_errors(&self) -> Vec<DeviceError> {
        self.errors.try_iter().map(DeviceError::from).collect()
    }
}
