//! The videos and photos canvases read. A video decodes on a thread of its own, the frame last
//! asked for, so the window never waits for one. A canvas shows the newest frame decoded.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use sound_canvas::Frame;
use sound_media::video::Video;

/// The most pixels across or down of a frame: as many as a canvas works out.
const LONGEST: u32 = sound_canvas::MAX_ACROSS;
/// How often a video or photo that did not open is looked for again.
const AGAIN: Duration = Duration::from_secs(1);
/// Frames a second it decodes at most, as most videos have: the window draws more often, and
/// each frame of the video then decodes once.
const RATE: f64 = 30.0;

/// A video being decoded. Dropping it ends its thread.
pub(crate) struct Player {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// Wakes the thread when a second is asked for.
    asked: Condvar,
}

#[derive(Default)]
struct State {
    /// The second to decode next.
    asked: Option<f64>,
    /// The newest frame decoded and the second it was asked for, and how long the video is
    /// once it opened.
    shown: Option<(f64, Arc<Frame>)>,
    length: Option<f64>,
    /// What went wrong reading it last.
    problem: Option<String>,
    /// The player is gone.
    gone: bool,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Player {
    pub(crate) fn open(path: &Path) -> Self {
        let shared = Arc::new(Shared::default());
        let path = path.to_path_buf();
        let spawned = std::thread::Builder::new().name("video".into()).spawn({
            let shared = shared.clone();
            move || decode(&path, &shared)
        });
        if let Err(error) = spawned {
            eprintln!("error: a video did not start: {error}");
        }
        Self { shared }
    }

    /// Asks for the frame at `seconds`, round and round the video when it `loops`, else held at
    /// its ends, and gives the newest decoded.
    pub(crate) fn frame(&self, seconds: f64, loops: bool) -> Option<Arc<Frame>> {
        let shared = &self.shared;
        let mut state = shared.state();
        let length = state.length?;
        let at = match loops {
            true => seconds.rem_euclid(length.max(f64::MIN_POSITIVE)),
            false => seconds.clamp(0.0, length),
        };
        let at = (at * RATE).floor() / RATE;
        state.asked = Some(at);
        shared.asked.notify_one();
        state.shown.as_ref().map(|(_, frame)| frame.clone())
    }

    pub(crate) fn problem(&self) -> Option<String> {
        self.shared.state().problem.clone()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.state().gone = true;
        self.shared.asked.notify_one();
    }
}

fn decode(path: &Path, shared: &Shared) {
    let mut video = loop {
        match Video::open(path, LONGEST) {
            Ok(video) => break video,
            Err(error) => {
                let mut state = shared.state();
                state.problem = Some(error.to_string());
                // Looked for again each second, as the file may come.
                let waited = (shared.asked).wait_timeout_while(state, AGAIN, |state| !state.gone);
                let gone = match waited {
                    Ok((state, _)) => state.gone,
                    Err(poisoned) => poisoned.into_inner().0.gone,
                };
                if gone {
                    return;
                }
            }
        }
    };
    let first = video
        .frame_at(0.0)
        .ok()
        .map(|frame| (0.0, Arc::new(frame_of(&frame))));
    {
        let mut state = shared.state();
        state.length = Some(video.seconds());
        state.shown = first;
        state.problem = None;
    }
    loop {
        let at = {
            let state = shared.state();
            let waited = shared.asked.wait_while(state, |state| {
                let done = state.shown.as_ref().map(|(shown, _)| *shown);
                !state.gone && (state.asked.is_none() || state.asked == done)
            });
            let mut state = match waited {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            if state.gone {
                return;
            }
            match state.asked.take() {
                Some(at) => at,
                None => continue,
            }
        };
        let frame = video.frame_at(at);
        let mut state = shared.state();
        match frame {
            Ok(frame) => {
                state.shown = Some((at, Arc::new(frame_of(&frame))));
                state.problem = None;
            }
            Err(error) => state.problem = Some(error.to_string()),
        }
    }
}

fn frame_of(frame: &sound_media::video::VideoFrame) -> Frame {
    Frame::from_rgba(frame.width, frame.height, &frame.rgba)
}

/// A photo under `assets/images/`, read once on a thread of its own: black until it is.
pub(crate) struct Still(Arc<Mutex<Read>>);

#[derive(Default)]
struct Read {
    frame: Option<Arc<Frame>>,
    problem: Option<String>,
}

impl Still {
    pub(crate) fn open(path: &Path) -> Self {
        let read = Arc::new(Mutex::new(Read::default()));
        let path = path.to_path_buf();
        let spawned = std::thread::Builder::new().name("photo".into()).spawn({
            let read = read.clone();
            // Looked for again each second, as the file may come, while the photo is shown.
            move || {
                while Arc::strong_count(&read) > 1 {
                    let opened = upright(&path).map(|photo| {
                        let photo = photo.thumbnail(LONGEST, LONGEST).to_rgba8();
                        let (width, height) = (photo.width() as usize, photo.height() as usize);
                        Arc::new(Frame::from_rgba(width, height, photo.as_raw()))
                    });
                    let mut read = read
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    match opened {
                        Ok(frame) => {
                            read.frame = Some(frame);
                            read.problem = None;
                            return;
                        }
                        Err(error) => {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            read.problem = Some(format!("the photo {name} did not open: {error}"));
                        }
                    }
                    drop(read);
                    std::thread::sleep(AGAIN);
                }
            }
        });
        if let Err(error) = spawned {
            eprintln!("error: a photo did not start to open: {error}");
        }
        Self(read)
    }

    fn read(&self) -> std::sync::MutexGuard<'_, Read> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn frame(&self) -> Option<Arc<Frame>> {
        self.read().frame.clone()
    }

    pub(crate) fn problem(&self) -> Option<String> {
        self.read().problem.clone()
    }
}

/// The photo at `path`, turned as its camera noted.
fn upright(path: &Path) -> image::ImageResult<image::DynamicImage> {
    use image::ImageDecoder as _;
    let mut decoder = image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut photo = image::DynamicImage::from_decoder(decoder)?;
    photo.apply_orientation(orientation);
    Ok(photo)
}
