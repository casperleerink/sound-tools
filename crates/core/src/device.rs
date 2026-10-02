//! Device output through cpal. A thin wrapper: open the default output, hand it an engine.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::clock::MIN_EXACT_SAMPLE_RATE;
use crate::engine::Engine;

/// Monotonic nanoseconds since the first call in this process.
///
/// One clock for the device callback and for anything outside it whose time is compared with a
/// frame of the engine, such as the moment a MIDI message arrived. `Instant` itself cannot be
/// shared through an atomic, and two clocks with two starting points cannot be subtracted.
/// The first call sets the clock up, which may lock. So a stream calls it once before it
/// starts, and its callback only reads the clock.
pub fn monotonic_nanos() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    let nanos = START.get_or_init(Instant::now).elapsed().as_nanos();
    u64::try_from(nanos).unwrap_or(u64::MAX)
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("no default audio output device")]
    NoOutputDevice,
    #[error("no default audio input device: set one in the Sound settings of macOS")]
    NoInputDevice,
    #[error("the device wants {0} samples, only f32 is supported")]
    UnsupportedSampleFormat(cpal::SampleFormat),
    #[error("the output device runs at {0} Hz, the clock needs {MIN_EXACT_SAMPLE_RATE} Hz or more")]
    SampleRateTooLow(u32),
    #[error("the engine has {engine} channels, the device has {device}")]
    ChannelMismatch { engine: usize, device: usize },
    #[error("the engine runs at {engine} Hz, the device at {device} Hz")]
    SampleRateMismatch { engine: u32, device: u32 },
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

    /// The name people know the device by, for example "MacBook Pro Speakers".
    pub fn name(&self) -> Result<String, DeviceError> {
        Ok(self.device.description()?.name().to_string())
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
        // The clock turns ticks into frames with the engine's rate. On a device with another
        // rate every tempo would play at the wrong speed.
        if engine.sample_rate() != self.sample_rate() {
            return Err(DeviceError::SampleRateMismatch {
                engine: engine.sample_rate(),
                device: self.sample_rate(),
            });
        }
        let counters = Arc::new(Counters::default());
        let timing = Arc::new(StreamTiming::new(self.sample_rate()));
        let (error_sender, errors) = mpsc::channel();
        let channels = self.channels().max(1) as u64;
        let sample_rate = u64::from(self.sample_rate().max(1));
        // Sets the clock up here, not in the first callback.
        monotonic_nanos();

        let stream = self.device.build_output_stream(
            self.config,
            {
                let (counters, timing) = (counters.clone(), timing.clone());
                move |output: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    // Before the block, so the frame count is that of its first frame.
                    timing.observe(monotonic_nanos(), engine.frames(), info);
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
            timing,
            errors,
        })
    }
}

/// When the sound of an engine frame starts at the device, on the clock of
/// [`monotonic_nanos`].
///
/// The callback publishes two numbers per buffer and nothing is computed on the audio thread.
/// Frames and real time run at the same speed, so one pair of (time, frame) describes them all
/// until the clocks drift, which is nothing within a buffer. Keeping the time of frame 0
/// instead of the last pair makes both numbers independent, so a reader can never take the
/// time of one callback with the frame count of another.
pub struct StreamTiming {
    sample_rate: u64,
    /// The monotonic time at which engine frame 0 played. 0 before the first callback.
    frame_zero_nanos: AtomicU64,
    /// What the device adds after the callback rendered: the reported playback time of a
    /// buffer minus the time the callback ran.
    output_delay_nanos: AtomicU64,
    callbacks: AtomicU64,
}

impl StreamTiming {
    /// The timing of a device whose engine frame 0 started to sound at `frame_zero_nanos` on
    /// the clock of [`monotonic_nanos`], and that adds `output_delay` after a callback. For an
    /// engine a test runs by hand, which plays the part of a device.
    pub fn simulated(sample_rate: u32, frame_zero_nanos: u64, output_delay: Duration) -> Self {
        let timing = Self::new(sample_rate);
        timing
            .frame_zero_nanos
            .store(frame_zero_nanos, Ordering::Relaxed);
        let delay = u64::try_from(output_delay.as_nanos()).unwrap_or(u64::MAX);
        timing.output_delay_nanos.store(delay, Ordering::Relaxed);
        timing.callbacks.store(1, Ordering::Relaxed);
        timing
    }

    fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: u64::from(sample_rate.max(1)),
            frame_zero_nanos: AtomicU64::new(0),
            output_delay_nanos: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
        }
    }

    fn nanos_of(&self, frames: u64) -> u64 {
        let nanos = u128::from(frames) * 1_000_000_000 / u128::from(self.sample_rate);
        u64::try_from(nanos).unwrap_or(u64::MAX)
    }

    /// The audio thread, at the start of a callback: `now` on the monotonic clock, `frame` the
    /// engine time of the first frame of this buffer.
    fn observe(&self, now: u64, frame: u64, info: &cpal::OutputCallbackInfo) {
        let timestamp = info.timestamp();
        let delay = timestamp
            .playback
            .saturating_duration_since(timestamp.callback);
        let delay = u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX);
        self.frame_zero_nanos
            .store(now.saturating_sub(self.nanos_of(frame)), Ordering::Relaxed);
        self.output_delay_nanos.store(delay, Ordering::Relaxed);
        self.callbacks.fetch_add(1, Ordering::Relaxed);
    }

    /// How long the device takes to play what a callback has just rendered.
    pub fn output_latency(&self) -> Duration {
        Duration::from_nanos(self.output_delay_nanos.load(Ordering::Relaxed))
    }

    /// The monotonic time at which the sound of engine `frame` starts at the device. `None`
    /// before the first callback, and for an offline engine, which has no device.
    pub fn sound_time_nanos(&self, frame: u64) -> Option<u64> {
        if self.callbacks.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let start = self.frame_zero_nanos.load(Ordering::Relaxed);
        let delay = self.output_delay_nanos.load(Ordering::Relaxed);
        Some(
            start
                .saturating_add(self.nanos_of(frame))
                .saturating_add(delay),
        )
    }
}

impl StreamTiming {
    /// The engine frame whose sound started at the device at `nanos`, the inverse of
    /// [`Self::sound_time_nanos`], to the nearest frame: what a player heard at that moment.
    /// Below zero for a moment before the engine began. `None` before the first callback.
    pub fn frame_sounding_at(&self, nanos: u64) -> Option<i64> {
        if self.callbacks.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let start = self.frame_zero_nanos.load(Ordering::Relaxed);
        let delay = self.output_delay_nanos.load(Ordering::Relaxed);
        let since = i128::from(nanos) - i128::from(start) - i128::from(delay);
        let rate = i128::from(self.sample_rate);
        // Rounded to the nearest frame, so a moment from `sound_time_nanos`, which rounds a
        // frame down to whole nanoseconds, gives its frame back.
        let frame = (since * rate + since.signum() * 500_000_000) / 1_000_000_000;
        i64::try_from(frame).ok()
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
    /// What the device says it adds after a callback has rendered a buffer.
    pub output_latency: Duration,
}

/// A playing stream. Dropping it stops playback and drops the engine.
pub struct OutputStream {
    _stream: cpal::Stream,
    counters: Arc<Counters>,
    timing: Arc<StreamTiming>,
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
            output_latency: self.timing.output_latency(),
        }
    }

    /// When the sound of an engine frame starts at the device. Whoever measures the way from
    /// an input to the sound keeps this handle.
    pub fn timing(&self) -> &Arc<StreamTiming> {
        &self.timing
    }

    /// Stream errors other than xruns since the last call, for example a lost device.
    pub fn take_errors(&self) -> Vec<DeviceError> {
        self.errors.try_iter().map(DeviceError::from).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_sounding_at_a_moment_is_the_inverse_of_when_a_frame_sounds() {
        let delay = Duration::from_micros(7_300);
        let timing = StreamTiming::simulated(48_000, 3_000_000_000, delay);
        for frame in [0, 1, 47_999, 48_000, 12_345_678] {
            let nanos = timing.sound_time_nanos(frame).unwrap();
            assert_eq!(timing.frame_sounding_at(nanos), Some(frame as i64));
        }
        // Before the engine began, and a moment between two frames.
        assert_eq!(timing.frame_sounding_at(3_007_300_000 - 20_834), Some(-1));
        assert_eq!(timing.frame_sounding_at(3_007_300_000 + 10_000), Some(0));
        assert_eq!(timing.frame_sounding_at(3_007_300_000 + 11_000), Some(1));
        let offline = StreamTiming::new(48_000);
        assert_eq!(offline.frame_sounding_at(0), None);
    }
}
