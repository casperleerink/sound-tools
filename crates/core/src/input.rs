//! Audio input: the default input device of the system, and what it captured, on its way from
//! the device's thread to whoever records it.
//!
//! Nothing here goes through the engine. There is no monitoring in software, so a captured
//! sample is never played: it goes from the device callback into a lock-free ring, and a reader
//! on an ordinary thread takes it from there and writes it to a file. Each captured frame keeps
//! the moment it was captured, on the clock of [`monotonic_nanos`], so a recorder can tell what
//! the composer heard while it was played (see [`StreamTiming::frame_sounding_at`]).
//!
//! [`StreamTiming::frame_sounding_at`]: crate::StreamTiming::frame_sounding_at

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtsan_standalone::nonblocking;

use crate::device::{DeviceError, monotonic_nanos};

/// How much input the ring holds for a reader that fell behind. A reader that is late by more
/// than this loses frames, which [`CaptureReader::lost_frames`] counts and reads as silence.
pub const CAPTURE_SECONDS: u32 = 10;

/// Stretches of lost frames the ring remembers until the reader comes back. Past that, the
/// writer keeps counting into the last one and leaves its buffers out as well.
const GAPS: usize = 256;

/// Frames left out because the ring was full, before the frame numbered `at`.
#[derive(Copy, Clone, Debug)]
struct Gap {
    at: u64,
    frames: u64,
}

/// The default input device of the system and its default configuration.
pub struct InputDevice {
    device: cpal::Device,
    config: cpal::StreamConfig,
}

impl InputDevice {
    /// The input device macOS has as its default, as set in the system settings. There is no
    /// choice of device in the application.
    pub fn default_input() -> Result<Self, DeviceError> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or(DeviceError::NoInputDevice)?;
        let supported = device.default_input_config()?;
        if supported.sample_format() != cpal::SampleFormat::F32 {
            return Err(DeviceError::UnsupportedSampleFormat(
                supported.sample_format(),
            ));
        }
        Ok(Self {
            device,
            config: supported.config(),
        })
    }

    /// The name people know the device by, for example "MacBook Pro Microphone".
    pub fn name(&self) -> Result<String, DeviceError> {
        Ok(self.device.description()?.name().to_string())
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    pub fn channels(&self) -> usize {
        usize::from(self.config.channels)
    }

    /// Starts capturing. What the device captures goes into the ring of the reader this gives,
    /// stamped with when it was captured. Dropping the stream stops the device.
    pub fn start(self) -> Result<(InputStream, CaptureReader), DeviceError> {
        let (mut writer, reader) = capture(self.sample_rate(), self.channels());
        let gone = reader.status();
        // Sets the clock up here, not in the first callback.
        monotonic_nanos();
        let stream = self.device.build_input_stream(
            self.config,
            move |samples: &[f32], info: &cpal::InputCallbackInfo| {
                let timestamp = info.timestamp();
                let latency = timestamp
                    .callback
                    .saturating_duration_since(timestamp.capture);
                let latency = u64::try_from(latency.as_nanos()).unwrap_or(u64::MAX);
                writer.write(samples, monotonic_nanos(), latency);
            },
            // cpal calls this from a thread of its own, not from the data callback.
            move |error: cpal::Error| {
                if matches!(
                    error.kind(),
                    cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::StreamInvalidated
                ) {
                    gone.0.gone.store(true, Ordering::Relaxed);
                }
            },
            None,
        )?;
        stream.play()?;
        Ok((InputStream { _stream: stream }, reader))
    }
}

/// A running input device. Dropping it stops the device.
pub struct InputStream {
    _stream: cpal::Stream,
}

/// What both ends of a capture share: when its frames were captured, and how it goes.
struct Shared {
    sample_rate: u32,
    channels: usize,
    /// The capture time of written frame 0, on the clock of [`monotonic_nanos`], worked out
    /// from the latest buffer. 0 before the first.
    frame_zero_nanos: AtomicU64,
    written: AtomicU64,
    /// Frames the ring had no room for.
    lost: AtomicU64,
    /// The input went away: the device was unplugged, or the writer was dropped.
    gone: AtomicBool,
    /// The loudest sample of each channel since the last [`CaptureStatus::take_levels`], as
    /// the bits of a float of 0 or more, which sort as the floats do.
    levels: Box<[AtomicU32]>,
}

impl Shared {
    fn nanos_of(&self, frames: u64) -> u64 {
        let nanos = u128::from(frames) * 1_000_000_000 / u128::from(self.sample_rate.max(1));
        u64::try_from(nanos).unwrap_or(u64::MAX)
    }
}

/// A ring from the thread that captures to the one that records, and a handle for whoever
/// shows the level. A device makes one in [`InputDevice::start`]; a test makes one here and
/// writes into it what a device would have captured.
pub fn capture(sample_rate: u32, channels: usize) -> (CaptureWriter, CaptureReader) {
    let channels = channels.max(1);
    let capacity = sample_rate as usize * CAPTURE_SECONDS as usize * channels;
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    let (gaps, gaps_read) = rtrb::RingBuffer::new(GAPS);
    let shared = Arc::new(Shared {
        sample_rate,
        channels,
        frame_zero_nanos: AtomicU64::new(0),
        written: AtomicU64::new(0),
        lost: AtomicU64::new(0),
        gone: AtomicBool::new(false),
        levels: (0..channels).map(|_| AtomicU32::new(0)).collect(),
    });
    let writer = CaptureWriter {
        producer,
        gaps,
        shared: shared.clone(),
        position: 0,
        lost: 0,
    };
    let reader = CaptureReader {
        consumer,
        gaps: gaps_read,
        shared,
        read: 0,
    };
    (writer, reader)
}

/// The end of a capture on the thread of the device. Dropping it says the input is gone.
pub struct CaptureWriter {
    producer: rtrb::Producer<f32>,
    /// Where frames were left out, so the reader puts silence there and every frame after keeps
    /// its number, and with it the moment it was captured.
    gaps: rtrb::Producer<Gap>,
    shared: Arc<Shared>,
    /// Frames captured so far, the ones left out included: the number of the next frame.
    position: u64,
    /// Frames left out since the last buffer that went in, not yet told to the reader.
    lost: u64,
}

impl CaptureWriter {
    /// Puts one buffer of the device into the ring: interleaved samples of every channel.
    /// `callback_nanos` is when the callback that brings it began, on the clock of
    /// [`monotonic_nanos`], and `latency_nanos` what the device says passed from the capture of
    /// its first frame to then: its buffer and its own latency. Realtime safe: the device
    /// callback calls it.
    ///
    /// A buffer the ring has no room for is left out whole and counted, so a reader never
    /// takes half a frame. The next buffer that goes in says how many frames came before it,
    /// and the reader reads them as silence.
    #[nonblocking]
    pub fn write(&mut self, samples: &[f32], callback_nanos: u64, latency_nanos: u64) {
        let capture_nanos = callback_nanos.saturating_sub(latency_nanos);
        let channels = self.shared.channels;
        let whole = samples.len() - samples.len() % channels;
        let samples = samples.get(..whole).unwrap_or_default();
        for (channel, level) in self.shared.levels.iter().enumerate() {
            let channel_samples = samples.iter().skip(channel).step_by(channels);
            let peak = channel_samples.fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            // Not a number is no level, and -0.0 would sort above every level.
            if peak > 0.0 {
                level.fetch_max(peak.to_bits(), Ordering::Relaxed);
            }
        }
        let frames = (whole / channels) as u64;
        let room = self.producer.slots() >= samples.len();
        let told = self.lost == 0 || !self.gaps.is_full();
        if !room || !told {
            self.shared.lost.fetch_add(frames, Ordering::Relaxed);
            self.lost += frames;
            self.position += frames;
            return;
        }
        if self.lost > 0 {
            let gap = Gap {
                at: self.position - self.lost,
                frames: self.lost,
            };
            // There is room: it was looked at above, and only this side adds.
            if self.gaps.push(gap).is_ok() {
                self.lost = 0;
            }
        }
        if self.producer.push_entire_slice(samples).is_err() {
            return;
        }
        let zero = capture_nanos.saturating_sub(self.shared.nanos_of(self.position));
        self.shared.frame_zero_nanos.store(zero, Ordering::Relaxed);
        self.position += frames;
        self.shared.written.store(self.position, Ordering::Relaxed);
    }
}

impl Drop for CaptureWriter {
    fn drop(&mut self) {
        self.shared.gone.store(true, Ordering::Relaxed);
    }
}

/// The end of a capture that records: it takes what was captured, and knows when each frame
/// was. One reader per capture.
pub struct CaptureReader {
    consumer: rtrb::Consumer<f32>,
    gaps: rtrb::Consumer<Gap>,
    shared: Arc<Shared>,
    /// Frames read so far: the number of the next frame [`Self::read`] gives.
    read: u64,
}

impl CaptureReader {
    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate
    }

    pub fn channels(&self) -> usize {
        self.shared.channels
    }

    /// Appends every whole frame the ring holds to `into`, interleaved, and gives the number
    /// of the first of them. Frames are numbered from the first one ever captured. Frames the
    /// ring had no room for come as silence where they were, so every frame keeps its number.
    pub fn read(&mut self, into: &mut Vec<f32>) -> u64 {
        let first = self.read;
        let channels = self.shared.channels;
        let available = self.consumer.slots();
        let whole = available - available % channels;
        let Ok(chunk) = self.consumer.read_chunk(whole) else {
            return first;
        };
        let (head, tail) = chunk.as_slices();
        let mut samples = head.iter().chain(tail).copied();
        let mut left = (whole / channels) as u64;
        loop {
            // A gap is told before the frames after it go in, so it is here when they are.
            while let Ok(gap) = self.gaps.peek().copied()
                && gap.at <= self.read
            {
                let silence = gap.frames as usize * channels;
                into.extend(std::iter::repeat_n(0.0, silence));
                self.read += gap.frames;
                if self.gaps.pop().is_err() {
                    break;
                }
            }
            if left == 0 {
                break;
            }
            let next_gap = self.gaps.peek().map_or(u64::MAX, |gap| gap.at);
            let frames = left.min(next_gap.saturating_sub(self.read).max(1));
            into.extend(samples.by_ref().take(frames as usize * channels));
            self.read += frames;
            left -= frames;
        }
        chunk.commit_all();
        first
    }

    /// When frame `frame` was captured, on the clock of [`monotonic_nanos`]. `None` before
    /// anything was written. Worked out from the latest buffer, so it follows the clock of the
    /// device as it drifts from this one.
    pub fn nanos_of(&self, frame: u64) -> Option<u64> {
        if self.shared.written.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let zero = self.shared.frame_zero_nanos.load(Ordering::Relaxed);
        Some(zero.saturating_add(self.shared.nanos_of(frame)))
    }

    /// Frames the ring had no room for, because this reader fell behind. They are read as
    /// silence where they were, so a recording that spans them stays in time, with a hole.
    pub fn lost_frames(&self) -> u64 {
        self.shared.lost.load(Ordering::Relaxed)
    }

    /// The input went away, see [`CaptureStatus::is_gone`].
    pub fn is_gone(&self) -> bool {
        self.shared.gone.load(Ordering::Relaxed)
    }

    /// A handle for the thread that shows the level and watches the input.
    pub fn status(&self) -> CaptureStatus {
        CaptureStatus(self.shared.clone())
    }
}

/// The level of a capture and whether its input is still there, for the thread that shows
/// them. Clones share them.
#[derive(Clone)]
pub struct CaptureStatus(Arc<Shared>);

impl CaptureStatus {
    pub fn channels(&self) -> usize {
        self.0.channels
    }

    /// The loudest sample of each channel since the last take, and zero from now on. One
    /// reader: a take empties them.
    pub fn take_levels(&self) -> Vec<f32> {
        let levels = self.0.levels.iter();
        levels
            .map(|level| f32::from_bits(level.swap(0, Ordering::Relaxed)))
            .collect()
    }

    /// The input went away: the device was unplugged or stopped, or the writer was dropped.
    /// What was captured before stays in the ring for the reader.
    pub fn is_gone(&self) -> bool {
        self.0.gone.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_arrive_in_order_with_the_time_they_were_captured() {
        let (mut writer, mut reader) = capture(48_000, 2);
        let status = reader.status();
        assert_eq!(reader.nanos_of(0), None);
        // Captured 3 ms before the callback that brings it.
        writer.write(&[0.5, -0.25, 0.1, 0.2], 4_000_000, 3_000_000);
        // A half frame at the end is left out.
        writer.write(&[0.3, -0.75, 0.9], 4_041_666, 3_000_000);
        let mut samples = Vec::new();
        assert_eq!(reader.read(&mut samples), 0);
        assert_eq!(samples, [0.5, -0.25, 0.1, 0.2, 0.3, -0.75]);
        // Frame 2 came in the second buffer, which says when it was captured.
        assert_eq!(reader.nanos_of(2), Some(1_041_666));
        assert_eq!(reader.nanos_of(0), Some(1_041_666 - 41_666));
        assert_eq!(reader.read(&mut samples), 3);
        assert_eq!(status.take_levels(), [0.5, 0.75]);
        assert_eq!(status.take_levels(), [0.0, 0.0]);
        assert!(!status.is_gone());
        drop(writer);
        assert!(status.is_gone());
    }

    #[test]
    fn a_reader_that_falls_behind_reads_what_was_lost_as_silence_in_its_place() {
        // 100 frames a second, so the ring holds 1000 frames and a buffer is 400: 4 s.
        let (mut writer, mut reader) = capture(100, 1);
        let at = |buffer: u64| buffer * 4_000_000_000;
        for buffer in 0..3 {
            writer.write(&[0.1 + buffer as f32 / 10.; 400], at(buffer), 0);
        }
        // Two buffers fit, the third does not.
        assert_eq!(reader.lost_frames(), 400);
        let mut samples = Vec::new();
        assert_eq!(reader.read(&mut samples), 0);
        assert_eq!(samples.len(), 800);
        // The fourth goes in after 400 frames that are not there.
        writer.write(&[0.4; 400], at(3), 0);
        samples.clear();
        assert_eq!(reader.read(&mut samples), 800);
        assert_eq!(samples.len(), 800);
        assert!(samples[..400].iter().all(|sample| *sample == 0.0));
        assert!(samples[400..].iter().all(|sample| *sample == 0.4));
        // Its frames keep the moments they were captured at.
        assert_eq!(reader.nanos_of(1_200), Some(at(3)));
    }
}

// The window opens the input on a background thread and keeps the stream on its own.
const _: fn() = || {
    fn send<T: Send>() {}
    send::<InputStream>();
    send::<CaptureReader>();
};
