//! The frames of a video file, decoded by the system: AVFoundation on macOS. Elsewhere a video
//! does not open, and says so.
//!
//! Frames are read in order, as a player reads them, which decodes each once; a time before the
//! last frame or far after it starts reading again from there.

use std::path::Path;

/// A frame of a video: red, green, blue and alpha, a byte each, row by row from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("this computer plays no video: only macOS does")]
    Unsupported,
    #[error("{0} is no video this computer reads")]
    Unreadable(String),
    #[error("no frame at {0} s: {1}")]
    NoFrame(f64, String),
    #[error("{0} cannot be written: {1}")]
    Unwritable(String, String),
    #[error("the film stopped being written: {0}")]
    Writing(String),
}

/// Further ahead than this, reading on costs more than starting again from there.
#[cfg(target_os = "macos")]
const READ_ON_SECONDS: f64 = 2.0;
/// How far before a time it starts reading again.
#[cfg(target_os = "macos")]
const PREROLL_SECONDS: f64 = 0.5;

/// An open video file. Each frame it gives is at most `longest` pixels across or down.
pub struct Video {
    #[cfg(target_os = "macos")]
    reading: apple::Reading,
    seconds: f64,
}

impl Video {
    /// How long it plays.
    pub fn seconds(&self) -> f64 {
        self.seconds
    }
}

#[cfg(not(target_os = "macos"))]
impl Video {
    pub fn open(_: &Path, _: u32) -> Result<Self, VideoError> {
        Err(VideoError::Unsupported)
    }

    pub fn frame_at(&mut self, _: f64) -> Result<VideoFrame, VideoError> {
        Err(VideoError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
impl Video {
    pub fn open(path: &Path, longest: u32) -> Result<Self, VideoError> {
        let reading = apple::Reading::open(path, longest)
            .ok_or_else(|| VideoError::Unreadable(path.display().to_string()))?;
        Ok(Self {
            seconds: reading.seconds,
            reading,
        })
    }

    /// The frame shown at `seconds` into it: the last that starts at or before it.
    pub fn frame_at(&mut self, seconds: f64) -> Result<VideoFrame, VideoError> {
        let at = seconds.clamp(0.0, self.seconds);
        let reading = &mut self.reading;
        let jumped = match &reading.shown {
            Some((shown, _)) => at < *shown || at > shown + READ_ON_SECONDS,
            None => true,
        };
        // From a little before: the first frame read may start after `at`, and at the very end
        // there is none.
        if jumped && !reading.start((at - PREROLL_SECONDS).max(0.0)) {
            return Err(VideoError::NoFrame(at, "it did not start to read".into()));
        }
        loop {
            match &reading.next {
                Some((next, _)) if *next <= at => reading.shown = reading.next.take(),
                Some(_) => break,
                None => match reading.read() {
                    Some(frame) => reading.next = Some(frame),
                    None => break,
                },
            }
        }
        if reading.shown.is_none() {
            reading.shown = reading.next.take();
        }
        (reading.shown.as_ref())
            .map(|(_, frame)| frame.clone())
            .ok_or_else(|| VideoError::NoFrame(at, "it has no frame there".into()))
    }
}

#[cfg(target_os = "macos")]
mod apple {
    use std::path::Path;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_av_foundation::{
        AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack,
        AVMediaTypeVideo, AVURLAsset,
    };
    use objc2_core_media::{CMTime, CMTimeRange};
    use objc2_core_video::{
        CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
        CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey,
        kCVPixelBufferWidthKey, kCVPixelFormatType_32BGRA,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

    use super::VideoFrame;

    /// A frame and the second it starts at.
    type Timed = (f64, VideoFrame);

    pub(super) struct Reading {
        asset: Retained<AVURLAsset>,
        track: Retained<AVAssetTrack>,
        settings: Retained<NSDictionary<NSString, AnyObject>>,
        reader: Option<(Retained<AVAssetReader>, Retained<AVAssetReaderTrackOutput>)>,
        pub(super) seconds: f64,
        /// The frame shown now, and the one after it, read but not yet due.
        pub(super) shown: Option<Timed>,
        pub(super) next: Option<Timed>,
    }

    // SAFETY: the objects are used by one owner, one call at a time, which AVFoundation allows
    // from any thread.
    unsafe impl Send for Reading {}

    impl Reading {
        pub(super) fn open(path: &Path, longest: u32) -> Option<Self> {
            if !path.is_file() {
                return None;
            }
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            // SAFETY: plain calls on objects made here; the asset reads the file on demand.
            unsafe {
                let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
                let seconds = asset.duration().seconds();
                #[allow(deprecated)]
                let track = asset.tracksWithMediaType(AVMediaTypeVideo?).firstObject()?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return None;
                }
                let natural = track.naturalSize();
                let scale = (f64::from(longest) / natural.width.max(natural.height)).min(1.0);
                let size =
                    |side: f64| NSNumber::new_usize(((side * scale).round() as usize).max(1));
                let keys: [&NSString; 3] = [
                    kCVPixelBufferPixelFormatTypeKey,
                    kCVPixelBufferWidthKey,
                    kCVPixelBufferHeightKey,
                ]
                .map(|key| &*(key as *const _ as *const NSString));
                let values: [Retained<NSNumber>; 3] = [
                    NSNumber::new_u32(kCVPixelFormatType_32BGRA),
                    size(natural.width),
                    size(natural.height),
                ];
                let values: Vec<&AnyObject> = values.iter().map(|value| value.as_ref()).collect();
                let settings = NSDictionary::from_slices(&keys, &values);
                Some(Self {
                    asset,
                    track,
                    settings,
                    reader: None,
                    seconds,
                    shown: None,
                    next: None,
                })
            }
        }

        /// Starts reading again from `seconds`. Whether it could.
        pub(super) fn start(&mut self, seconds: f64) -> bool {
            self.shown = None;
            self.next = None;
            if let Some((reader, _)) = self.reader.take() {
                // SAFETY: a reader that is done with.
                unsafe { reader.cancelReading() };
            }
            // SAFETY: a reader of the asset made here, on its own video track.
            unsafe {
                let Ok(reader) = AVAssetReader::assetReaderWithAsset_error(&self.asset) else {
                    return false;
                };
                let output =
                    AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                        &self.track,
                        Some(&self.settings),
                    );
                output.setAlwaysCopiesSampleData(false);
                if !reader.canAddOutput(&output) {
                    return false;
                }
                reader.addOutput(&output);
                let start = CMTime::with_seconds(seconds, 600);
                let rest = CMTime::with_seconds((self.seconds - seconds).max(0.0) + 1.0, 600);
                reader.setTimeRange(CMTimeRange {
                    start,
                    duration: rest,
                });
                if !reader.startReading() {
                    return false;
                }
                self.reader = Some((reader, output));
            }
            true
        }

        /// The next frame, or `None` at the end or when it cannot.
        pub(super) fn read(&mut self) -> Option<Timed> {
            let (reader, output) = self.reader.as_ref()?;
            // SAFETY: the pixels are read only while their buffer is locked, row by row within
            // its bytes per row.
            unsafe {
                if reader.status() != AVAssetReaderStatus::Reading {
                    return None;
                }
                let sample = output.copyNextSampleBuffer()?;
                let at = sample.presentation_time_stamp().seconds();
                let pixels = sample.image_buffer()?;
                if CVPixelBufferLockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) != 0 {
                    return None;
                }
                let (width, height) = (
                    CVPixelBufferGetWidth(&pixels),
                    CVPixelBufferGetHeight(&pixels),
                );
                let row_bytes = CVPixelBufferGetBytesPerRow(&pixels);
                let base = CVPixelBufferGetBaseAddress(&pixels).cast::<u8>();
                let mut rgba = Vec::with_capacity(width * height * 4);
                if !base.is_null() {
                    for row in 0..height {
                        let bgra = std::slice::from_raw_parts(base.add(row * row_bytes), width * 4);
                        for pixel in bgra.chunks_exact(4) {
                            if let [blue, green, red, alpha] = *pixel {
                                rgba.extend([red, green, blue, alpha]);
                            }
                        }
                    }
                }
                CVPixelBufferUnlockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly);
                Some((
                    at,
                    VideoFrame {
                        width,
                        height,
                        rgba,
                    },
                ))
            }
        }
    }
}

/// Writes a film: frames and stereo sound into an `.mp4`, H.264 and AAC, by the system.
pub struct VideoWriter {
    #[cfg(target_os = "macos")]
    writing: apple_writer::Writing,
}

#[cfg(not(target_os = "macos"))]
impl VideoWriter {
    pub fn create(_: &Path, _: (usize, usize), _: u32) -> Result<Self, VideoError> {
        Err(VideoError::Unsupported)
    }

    pub fn frame(&mut self, _: &VideoFrame, _: f64) -> Result<(), VideoError> {
        Err(VideoError::Unsupported)
    }

    pub fn sound(&mut self, _: &[f32], _: u64) -> Result<(), VideoError> {
        Err(VideoError::Unsupported)
    }

    pub fn finish(self) -> Result<(), VideoError> {
        Err(VideoError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
impl VideoWriter {
    /// A film of frames `size` across and down, with sound at `sample_rate`. The file must not
    /// exist yet.
    pub fn create(path: &Path, size: (usize, usize), sample_rate: u32) -> Result<Self, VideoError> {
        let writing = apple_writer::Writing::create(path, size, sample_rate)
            .map_err(|why| VideoError::Unwritable(path.display().to_string(), why))?;
        Ok(Self { writing })
    }

    /// Adds `frame`, shown from `seconds` on.
    pub fn frame(&mut self, frame: &VideoFrame, seconds: f64) -> Result<(), VideoError> {
        self.writing
            .frame(frame, seconds)
            .map_err(VideoError::Writing)
    }

    /// Adds `samples`, left and right in turn, starting at frame `first` of the sound.
    pub fn sound(&mut self, samples: &[f32], first: u64) -> Result<(), VideoError> {
        self.writing
            .sound(samples, first)
            .map_err(VideoError::Writing)
    }

    /// Ends the file. Blocks until it is written.
    pub fn finish(self) -> Result<(), VideoError> {
        self.writing.finish().map_err(VideoError::Writing)
    }
}

#[cfg(target_os = "macos")]
mod apple_writer {
    use std::collections::VecDeque;
    use std::path::Path;
    use std::ptr::NonNull;

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_av_foundation::{
        AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor,
        AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo,
    };
    use objc2_core_audio_types::{
        AudioStreamBasicDescription, kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked,
        kAudioFormatLinearPCM,
    };
    use objc2_core_foundation::CFRetained;
    use objc2_core_media::{
        CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateReadyWithPacketDescriptions,
        CMBlockBuffer, CMBlockBufferFlags, CMFormatDescription, CMSampleBuffer, CMTime,
    };
    use objc2_core_video::{
        CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress,
        CVPixelBufferGetBytesPerRow, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

    use super::VideoFrame;

    /// `'aac '`: AAC, as `kAudioFormatMPEG4AAC` is.
    const AAC: u32 = 0x6161_6320;

    pub(super) struct Writing {
        writer: Retained<AVAssetWriter>,
        video: Retained<AVAssetWriterInput>,
        pixels: Retained<AVAssetWriterInputPixelBufferAdaptor>,
        audio: Retained<AVAssetWriterInput>,
        format: CFRetained<CMFormatDescription>,
        size: (usize, usize),
        sample_rate: u32,
        /// What waits for its input to take more: the writer takes one track only as far as
        /// the other has come, so neither may block the other.
        frames: VecDeque<(CFRetained<CVPixelBuffer>, CMTime)>,
        sounds: VecDeque<CFRetained<CMSampleBuffer>>,
    }

    fn settings(
        pairs: &[(&str, Retained<AnyObject>)],
    ) -> Retained<NSDictionary<NSString, AnyObject>> {
        let keys: Vec<Retained<NSString>> = pairs
            .iter()
            .map(|(key, _)| NSString::from_str(key))
            .collect();
        let keys: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
        let values: Vec<&AnyObject> = pairs.iter().map(|(_, value)| &**value).collect();
        NSDictionary::from_slices(&keys, &values)
    }

    fn number(value: usize) -> Retained<AnyObject> {
        Retained::into_super(Retained::into_super(NSNumber::new_usize(value))).into()
    }

    impl Writing {
        pub(super) fn create(
            path: &Path,
            size: (usize, usize),
            sample_rate: u32,
        ) -> Result<Self, String> {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            // SAFETY: objects made here, set up before writing starts, as AVFoundation asks.
            unsafe {
                let mpeg4 = AVFileTypeMPEG4.ok_or("no MPEG-4 here")?;
                let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, mpeg4)
                    .map_err(|error| error.localizedDescription().to_string())?;
                let codec: Retained<AnyObject> =
                    Retained::into_super(NSString::from_str("avc1")).into();
                let video_settings = settings(&[
                    ("AVVideoCodecKey", codec),
                    ("AVVideoWidthKey", number(size.0)),
                    ("AVVideoHeightKey", number(size.1)),
                ]);
                let video = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                    AVMediaTypeVideo.ok_or("no video here")?,
                    Some(&video_settings),
                );
                video.setExpectsMediaDataInRealTime(false);
                let pixels = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&video, None);
                let audio_settings = settings(&[
                    ("AVFormatIDKey", number(AAC as usize)),
                    ("AVSampleRateKey", number(sample_rate as usize)),
                    ("AVNumberOfChannelsKey", number(2)),
                    ("AVEncoderBitRateKey", number(256_000)),
                ]);
                let audio = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                    AVMediaTypeAudio.ok_or("no audio here")?,
                    Some(&audio_settings),
                );
                audio.setExpectsMediaDataInRealTime(false);
                for input in [&video, &audio] {
                    if !writer.canAddInput(input) {
                        return Err("the film takes no such track".into());
                    }
                    writer.addInput(input);
                }
                let description = AudioStreamBasicDescription {
                    mSampleRate: f64::from(sample_rate),
                    mFormatID: kAudioFormatLinearPCM,
                    mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
                    mBytesPerPacket: 8,
                    mFramesPerPacket: 1,
                    mBytesPerFrame: 8,
                    mChannelsPerFrame: 2,
                    mBitsPerChannel: 32,
                    mReserved: 0,
                };
                let mut format: *const CMFormatDescription = std::ptr::null();
                let status = CMAudioFormatDescriptionCreate(
                    None,
                    NonNull::from(&description),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    None,
                    NonNull::from(&mut format),
                );
                let format = NonNull::new(format.cast_mut())
                    .filter(|_| status == 0)
                    .map(|format| CFRetained::from_raw(format))
                    .ok_or("the sound has no format")?;
                if !writer.startWriting() {
                    return Err(failure(&writer));
                }
                writer.startSessionAtSourceTime(CMTime::with_seconds(0.0, 600));
                Ok(Self {
                    writer,
                    video,
                    pixels,
                    audio,
                    format,
                    size,
                    sample_rate,
                    frames: VecDeque::new(),
                    sounds: VecDeque::new(),
                })
            }
        }

        pub(super) fn frame(&mut self, frame: &VideoFrame, seconds: f64) -> Result<(), String> {
            let (width, height) = self.size;
            // SAFETY: the buffer is written only while locked, row by row within its bytes per
            // row, and the source row within the frame's bytes.
            unsafe {
                let mut buffer: *mut CVPixelBuffer = std::ptr::null_mut();
                let made = CVPixelBufferCreate(
                    None,
                    width,
                    height,
                    kCVPixelFormatType_32BGRA,
                    None,
                    NonNull::from(&mut buffer),
                );
                let buffer = NonNull::new(buffer)
                    .filter(|_| made == 0)
                    .map(|buffer| CFRetained::from_raw(buffer))
                    .ok_or("no room for a frame")?;
                CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0));
                let row_bytes = CVPixelBufferGetBytesPerRow(&buffer);
                let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
                if !base.is_null() {
                    for row in 0..height.min(frame.height) {
                        let into =
                            std::slice::from_raw_parts_mut(base.add(row * row_bytes), width * 4);
                        let from = frame
                            .rgba
                            .get(row * frame.width * 4..(row + 1) * frame.width * 4)
                            .unwrap_or_default();
                        for (into, from) in into.chunks_exact_mut(4).zip(from.chunks_exact(4)) {
                            if let [red, green, blue, _] = *from {
                                into.copy_from_slice(&[blue, green, red, 255]);
                            }
                        }
                    }
                }
                CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0));
                self.frames
                    .push_back((buffer, CMTime::with_seconds(seconds, 6000)));
            }
            self.hand_over()
        }

        pub(super) fn sound(&mut self, samples: &[f32], first: u64) -> Result<(), String> {
            let frames = samples.len() / 2;
            if frames == 0 {
                return Ok(());
            }
            let bytes = frames * 8;
            // SAFETY: the block buffer owns `bytes` bytes, which the samples fill.
            unsafe {
                let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
                let made = CMBlockBuffer::create_with_memory_block(
                    None,
                    std::ptr::null_mut(),
                    bytes,
                    None,
                    std::ptr::null(),
                    0,
                    bytes,
                    1 as CMBlockBufferFlags,
                    NonNull::from(&mut block),
                );
                let block = NonNull::new(block)
                    .filter(|_| made == 0)
                    .map(|block| CFRetained::from_raw(block))
                    .ok_or("no room for sound")?;
                let source = NonNull::new(samples.as_ptr().cast_mut().cast()).ok_or("no sound")?;
                if CMBlockBuffer::replace_data_bytes(source, &block, 0, bytes) != 0 {
                    return Err("the sound did not copy".into());
                }
                let at = CMTime {
                    value: first as i64,
                    timescale: self.sample_rate as i32,
                    ..CMTime::with_seconds(0.0, self.sample_rate as i32)
                };
                let mut sample: *mut CMSampleBuffer = std::ptr::null_mut();
                let made = CMAudioSampleBufferCreateReadyWithPacketDescriptions(
                    None,
                    &block,
                    &self.format,
                    frames as isize,
                    at,
                    std::ptr::null(),
                    NonNull::from(&mut sample),
                );
                let sample = NonNull::new(sample)
                    .filter(|_| made == 0)
                    .map(|sample| CFRetained::from_raw(sample))
                    .ok_or("no sound buffer")?;
                self.sounds.push_back(sample);
            }
            self.hand_over()
        }

        /// Hands each input what waits for it, as far as it takes it now.
        fn hand_over(&mut self) -> Result<(), String> {
            // SAFETY: appends of buffers made here, each once, in order of time.
            unsafe {
                if self.writer.status() != AVAssetWriterStatus::Writing {
                    return Err(failure(&self.writer));
                }
                while self.video.isReadyForMoreMediaData() {
                    let Some((buffer, at)) = self.frames.pop_front() else {
                        break;
                    };
                    if !self
                        .pixels
                        .appendPixelBuffer_withPresentationTime(&buffer, at)
                    {
                        return Err(failure(&self.writer));
                    }
                }
                while self.audio.isReadyForMoreMediaData() {
                    let Some(sample) = self.sounds.pop_front() else {
                        break;
                    };
                    if !self.audio.appendSampleBuffer(&sample) {
                        return Err(failure(&self.writer));
                    }
                }
            }
            Ok(())
        }

        pub(super) fn finish(mut self) -> Result<(), String> {
            // The sound ends first: the writer holds the last frames back until the sound has
            // come as far, or has ended.
            while !self.sounds.is_empty() {
                self.hand_over()?;
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            // SAFETY: an input whose buffers are all in.
            unsafe { self.audio.markAsFinished() };
            while !self.frames.is_empty() {
                self.hand_over()?;
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            // SAFETY: the inputs are done; the writer ends the file.
            unsafe {
                self.video.markAsFinished();
                #[allow(deprecated)]
                let finished = self.writer.finishWriting();
                match finished && self.writer.status() == AVAssetWriterStatus::Completed {
                    true => Ok(()),
                    false => Err(failure(&self.writer)),
                }
            }
        }
    }

    fn failure(writer: &AVAssetWriter) -> String {
        // SAFETY: a plain read of the writer's error.
        unsafe { writer.error() }.map_or_else(
            || "the film was not written".into(),
            |error| error.localizedDescription().to_string(),
        )
    }
}
