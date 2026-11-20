//! The files feeds read: the overview of each audio file a shown wave binds, made once on a
//! thread of its own, so the thread that draws never reads a file.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use sound_core::feeds::Sources;
use sound_core::{AssetName, Assets};
use sound_media::{AUDIO_FOLDER, AudioAsset, Cached, Info, Overview};

/// The [`Sources`] of a window: the waves of audio files. Takes are the input's.
#[derive(Default)]
pub struct Files {
    overviews: RefCell<HashMap<PathBuf, Kept>>,
}

/// The overview of one file as it was when it was asked for: a file that changes is read again.
struct Kept {
    info: Info,
    overview: Reading,
}

enum Reading {
    Waiting(Receiver<Option<Arc<Overview>>>),
    Ready(Arc<Overview>),
    Failed,
}

impl Files {
    pub fn new() -> Self {
        Self::default()
    }

    /// The overview of `asset` once it is made; asks a thread to make it the first time.
    fn overview(&self, assets: &Assets, asset: &AssetName) -> Option<Arc<Overview>> {
        let file = asset.as_str();
        let audio = AudioAsset::new(file.strip_prefix(AUDIO_FOLDER)?.trim_start_matches('/'));
        let audio = audio.ok()?;
        // The control side reads a file a record names; until it has, there is nothing to draw.
        let Cached::Plays(info) = sound_media::cached(assets, &audio) else {
            return None;
        };
        let mut overviews = self.overviews.borrow_mut();
        let kept = (overviews.entry(assets.path(asset)))
            .and_modify(|kept| {
                if kept.info != info {
                    *kept = Kept::read(assets, &audio, info);
                }
            })
            .or_insert_with(|| Kept::read(assets, &audio, info));
        if let Reading::Waiting(made) = &kept.overview {
            kept.overview = match made.try_recv() {
                Ok(Some(overview)) => Reading::Ready(overview),
                Ok(None) | Err(TryRecvError::Disconnected) => Reading::Failed,
                Err(TryRecvError::Empty) => return None,
            };
        }
        match &kept.overview {
            Reading::Ready(overview) => Some(overview.clone()),
            _ => None,
        }
    }
}

impl Kept {
    fn read(assets: &Assets, asset: &AudioAsset, info: Info) -> Self {
        let (made, overview) = channel();
        let (assets, asset) = (assets.clone(), asset.clone());
        let spawned = std::thread::Builder::new()
            .name("overview".into())
            .spawn(move || {
                let audio = (sound_media::in_memory(&assets, &asset))
                    .or_else(|| sound_media::load(&assets, &asset).ok());
                // The window may be gone, and with it the one who waits.
                drop(made.send(audio.map(|audio| Arc::new(Overview::of(&audio)))));
            });
        let overview = match spawned {
            Ok(_) => Reading::Waiting(overview),
            Err(_) => Reading::Failed,
        };
        Self { info, overview }
    }
}

impl Sources for Files {
    fn wave(&self, assets: &Assets, asset: &AssetName, edges: &[f64]) -> Option<Vec<f32>> {
        let overview = self.overview(assets, asset)?;
        let rate = f64::from(overview.sample_rate());
        let frames: Vec<u64> = (edges.iter())
            .map(|seconds| (seconds * rate).round().max(0.0) as u64)
            .collect();
        Some(overview.peaks(&frames))
    }
}
