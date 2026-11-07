//! The videos and photos canvases read. A video decodes on a thread of its own, the frame last
//! asked for, so the window never waits for one. A canvas shows the newest frame decoded.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use sound_canvas::Frame;
use sound_media::video::Video;

/// The most pixels across or down of a frame: as many as a canvas works out.
const LONGEST: u32 = sound_canvas::MAX_ACROSS;
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
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.state().gone = true;
        self.shared.asked.notify_one();
    }
}

fn decode(path: &Path, shared: &Shared) {
    let mut video = match Video::open(path, LONGEST) {
        Ok(video) => video,
        Err(error) => {
            eprintln!("error: {error}");
            return;
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
        match frame {
            Ok(frame) => shared.state().shown = Some((at, Arc::new(frame_of(&frame)))),
            Err(error) => eprintln!("error: {error}"),
        }
    }
}

fn frame_of(frame: &sound_media::video::VideoFrame) -> Frame {
    Frame::from_rgba(frame.width, frame.height, &frame.rgba)
}

/// A photo under `assets/images/`, read once on a thread of its own: black until it is.
pub(crate) struct Still(Arc<Mutex<Option<Arc<Frame>>>>);

impl Still {
    pub(crate) fn open(path: &Path) -> Self {
        let read = Arc::new(Mutex::new(None));
        let path = path.to_path_buf();
        let spawned = std::thread::Builder::new().name("photo".into()).spawn({
            let read = read.clone();
            move || match image::open(&path) {
                Ok(photo) => {
                    let photo = photo.thumbnail(LONGEST, LONGEST).to_rgba8();
                    let (width, height) = (photo.width() as usize, photo.height() as usize);
                    let frame = Arc::new(Frame::from_rgba(width, height, photo.as_raw()));
                    *read
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(frame);
                }
                Err(error) => eprintln!("error: {} did not open: {error}", path.display()),
            }
        });
        if let Err(error) = spawned {
            eprintln!("error: a photo did not start to open: {error}");
        }
        Self(read)
    }

    pub(crate) fn frame(&self) -> Option<Arc<Frame>> {
        self.0.lock().ok().and_then(|frame| frame.clone())
    }
}
