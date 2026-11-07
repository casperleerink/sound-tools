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
}

/// Further ahead than this, reading on costs more than starting again from there.
#[cfg(target_os = "macos")]
const READ_ON_SECONDS: f64 = 2.0;

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
        if jumped && !reading.start(at) {
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
    use objc2_core_foundation::CGSize;
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
                let natural: CGSize = track.naturalSize();
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
