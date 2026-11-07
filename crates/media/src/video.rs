//! The frames of a video file, decoded by the system: AVFoundation on macOS. Elsewhere a video
//! does not open, and says so.
//!
//! Frames are read in order, as a player reads them, which decodes each once; a time before the
//! last frame or far after it starts reading again from there.

use std::path::Path;

use sound_core::{AssetName, Assets};

use crate::MediaError;

/// The folder under `assets/` a film or photo goes in: `video/` for a film and
/// `images/` for a photo. `None` for anything else, such as sound.
pub fn visual_folder(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_string_lossy().to_lowercase();
    match extension.as_str() {
        "mp4" | "mov" | "m4v" => Some("video"),
        "jpg" | "jpeg" | "png" => Some("images"),
        _ => None,
    }
}

/// Copies a film or a photo into `assets/<folder>/` under a name of its own, never over a file
/// that is there, and gives its file name there, as a canvas names it.
pub fn import_visual(assets: &Assets, source: &Path, folder: &str) -> Result<String, MediaError> {
    let io_error = |source| MediaError::Io {
        path: folder.to_string(),
        source,
    };
    let part = |text: Option<&std::ffi::OsStr>, fallback: &str| {
        crate::asset_part(
            &text.map(|text| text.to_string_lossy()).unwrap_or_default(),
            fallback,
        )
    };
    let (stem, extension) = (
        part(source.file_stem(), folder),
        part(source.extension(), "mp4"),
    );
    let wanted = AssetName::new(folder, &stem, &extension)
        .map_err(|error| io_error(std::io::Error::new(std::io::ErrorKind::InvalidInput, error)))?;
    let reserved = assets
        .reserve(&wanted)
        .map_err(|error| io_error(std::io::Error::other(error)))?;
    let target = assets.path(&reserved);
    // Copied next to its place, then renamed over the empty file that holds its name. Its
    // name is reserved for this import alone, so no other import writes the same copy.
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temporary = target.with_file_name(format!(".{name}.import.tmp"));
    let copied =
        std::fs::copy(source, &temporary).and_then(|_| std::fs::rename(&temporary, &target));
    if let Err(error) = copied {
        crate::remove_quietly(&temporary);
        crate::remove_quietly(&target);
        return Err(io_error(error));
    }
    Ok(target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default())
}

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
        // A thread of its own has no pool of its own: what AVFoundation autoreleases would stay.
        let reading = objc2::rc::autoreleasepool(|_| apple::Reading::open(path, longest))
            .ok_or_else(|| {
                VideoError::Unreadable(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                )
            })?;
        Ok(Self {
            seconds: reading.seconds,
            reading,
        })
    }

    /// The frame shown at `seconds` into it: the last that starts at or before it.
    pub fn frame_at(&mut self, seconds: f64) -> Result<VideoFrame, VideoError> {
        objc2::rc::autoreleasepool(|_| self.read_to(seconds))
    }

    fn read_to(&mut self, seconds: f64) -> Result<VideoFrame, VideoError> {
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
