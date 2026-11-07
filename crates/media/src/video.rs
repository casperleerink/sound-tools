//! The frames of a video file, decoded by the system: AVFoundation on macOS. Elsewhere a video
//! does not open, and says so.

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

/// An open video file. Each frame it gives is at most `longest` pixels across or down.
pub struct Video {
    #[cfg(target_os = "macos")]
    generator: objc2::rc::Retained<objc2_av_foundation::AVAssetImageGenerator>,
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

    pub fn frame_at(&self, _: f64) -> Result<VideoFrame, VideoError> {
        Err(VideoError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
impl Video {
    pub fn open(path: &Path, longest: u32) -> Result<Self, VideoError> {
        use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
        use objc2_core_foundation::CGSize;
        use objc2_core_media::CMTime;
        use objc2_foundation::{NSString, NSURL};

        let unreadable = || VideoError::Unreadable(path.display().to_string());
        if !path.is_file() {
            return Err(unreadable());
        }
        let text = NSString::from_str(&path.to_string_lossy());
        let url = NSURL::fileURLWithPath(&text);
        // SAFETY: plain calls on objects made here; the asset reads the file on demand.
        unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            let seconds = asset.duration().seconds();
            if !seconds.is_finite() || seconds <= 0.0 {
                return Err(unreadable());
            }
            let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
            generator.setAppliesPreferredTrackTransform(true);
            let longest = f64::from(longest);
            generator.setMaximumSize(CGSize::new(longest, longest));
            // A frame near the time is enough while it plays: the exact one is slow to find.
            let near = CMTime::with_seconds(1.0 / 30.0, 600);
            generator.setRequestedTimeToleranceBefore(near);
            generator.setRequestedTimeToleranceAfter(near);
            Ok(Self { generator, seconds })
        }
    }

    /// The frame at `seconds` into it, or one near it. Takes milliseconds: call it off the main
    /// thread.
    pub fn frame_at(&self, seconds: f64) -> Result<VideoFrame, VideoError> {
        use objc2_core_foundation::{CGPoint, CGRect, CGSize};
        use objc2_core_graphics::{
            CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
        };
        use objc2_core_media::CMTime;

        let at = seconds.clamp(0.0, self.seconds);
        let no_frame = |why: String| VideoError::NoFrame(at, why);
        // SAFETY: the bitmap context writes into `rgba`, which is as large as it is told and
        // outlives it.
        unsafe {
            let time = CMTime::with_seconds(at, 600);
            #[allow(deprecated)]
            let image = self
                .generator
                .copyCGImageAtTime_actualTime_error(time, std::ptr::null_mut())
                .map_err(|error| no_frame(error.localizedDescription().to_string()))?;
            let (width, height) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
            let mut rgba = vec![0_u8; width * height * 4];
            let space = CGColorSpace::new_device_rgb();
            let context = CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                space.as_deref(),
                CGImageAlphaInfo::PremultipliedLast.0,
            )
            .ok_or_else(|| no_frame("no room to draw it".to_string()))?;
            let rect = CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(width as f64, height as f64),
            );
            CGContext::draw_image(Some(&context), rect, Some(&image));
            drop(context);
            Ok(VideoFrame {
                width,
                height,
                rgba,
            })
        }
    }
}

// SAFETY: AVAssetImageGenerator may be used from any thread, one call at a time, which `&self`
// of one owner gives.
#[cfg(target_os = "macos")]
unsafe impl Send for Video {}
