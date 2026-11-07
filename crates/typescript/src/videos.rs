//! The videos pictures read: each decoded on a thread of its own, the frame last asked for, so
//! the window never waits for one. A picture shows the newest frame decoded.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};

use sound_media::video::Video;
use sound_pictures::Frame;

/// The most pixels across or down of a frame: as many as a picture works out.
const LONGEST: u32 = sound_pictures::MAX_ACROSS;
/// Frames a second it decodes at most, as most videos have: the window draws more often, and
/// each frame of the video then decodes once.
const RATE: f64 = 30.0;

/// A video being decoded. Dropping it ends its thread.
pub(crate) struct Player {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    /// The second to decode next, or that the player is gone.
    asked: Mutex<Asked>,
    wake: Condvar,
    /// The newest frame decoded, and how long the video is once it opened.
    shown: Mutex<(Option<Arc<Frame>>, Option<f64>)>,
}

#[derive(Default)]
struct Asked {
    at: Option<f64>,
    gone: bool,
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
        let (shown, length) = match self.shared.shown.lock() {
            Ok(shown) => (shown.0.clone(), shown.1),
            Err(_) => return None,
        };
        if let (Some(length), Ok(mut asked)) = (length, self.shared.asked.lock()) {
            let at = match loops {
                true => seconds.rem_euclid(length.max(f64::MIN_POSITIVE)),
                false => seconds.clamp(0.0, length),
            };
            asked.at = Some((at * RATE).floor() / RATE);
            self.shared.wake.notify_one();
        }
        shown
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Ok(mut asked) = self.shared.asked.lock() {
            asked.gone = true;
        }
        self.shared.wake.notify_one();
    }
}

fn decode(path: &Path, shared: &Shared) {
    let video = match Video::open(path, LONGEST) {
        Ok(video) => video,
        Err(error) => {
            eprintln!("error: {error}");
            return;
        }
    };
    let first = video.frame_at(0.0);
    if let Ok(mut shown) = shared.shown.lock() {
        shown.1 = Some(video.seconds());
        shown.0 = first.ok().map(|frame| Arc::new(frame_of(&frame)));
    }
    let mut last = None;
    loop {
        let at = {
            let Ok(mut asked) = shared.asked.lock() else {
                return;
            };
            loop {
                if asked.gone {
                    return;
                }
                if let Some(at) = asked.at.take()
                    && Some(at) != last
                {
                    break at;
                }
                asked = match shared.wake.wait(asked) {
                    Ok(asked) => asked,
                    Err(_) => return,
                };
            }
        };
        last = Some(at);
        match video.frame_at(at) {
            Ok(frame) => {
                if let Ok(mut shown) = shared.shown.lock() {
                    shown.0 = Some(Arc::new(frame_of(&frame)));
                }
            }
            Err(error) => eprintln!("error: {error}"),
        }
    }
}

fn frame_of(frame: &sound_media::video::VideoFrame) -> Frame {
    Frame::from_rgba(frame.width, frame.height, &frame.rgba)
}
