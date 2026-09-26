//! The waveforms of audio files: an [`Overview`] of each file a view draws, made on a background
//! thread the first time it is asked for, and kept in memory while the application runs. Never
//! written into the project folder.
//!
//! One cache for the whole application, a GPUI global, so the arrangement and the Sampler draw
//! from the same overview of a file. A view that draws a waveform observes
//! [`Waveforms::entity`], which notifies when an overview is ready, and asks
//! [`Waveforms::overview`] while it draws: that returns at once, with `None` until the overview
//! is made. So the thread that draws never reads a file for a waveform.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext, Entity, Global};
use sound_core::Assets;
use sound_media::{AudioAsset, Info, Overview};

/// What is known of the waveform of one file.
enum Known {
    /// It is being made on a background thread.
    Making,
    Ready(Arc<Overview>),
    /// The file could not be read. The next change of the file tries again.
    Failed,
}

/// The overviews, by path and by what the file is, so a file replaced by another of another
/// length gets a new one.
#[derive(Default)]
pub struct Waveforms {
    known: HashMap<(PathBuf, Info), Known>,
    /// How many overviews were made, for a test that waits for one.
    made: u64,
}

struct GlobalWaveforms(Entity<Waveforms>);

impl Global for GlobalWaveforms {}

impl Waveforms {
    /// The cache of the application. Observe it to draw again when an overview is ready.
    pub fn entity(cx: &mut App) -> Entity<Waveforms> {
        if let Some(GlobalWaveforms(entity)) = cx.try_global::<GlobalWaveforms>() {
            return entity.clone();
        }
        let entity = cx.new(|_| Waveforms::default());
        cx.set_global(GlobalWaveforms(entity.clone()));
        entity
    }

    /// How many overviews were made since the application started.
    pub fn made(&self) -> u64 {
        self.made
    }

    /// The overview of a file of a project, when it is made. The first time it is asked for it
    /// is made on a background thread, and this returns `None` until then, as it does for a
    /// file that is not there or does not play. It costs one look at the size and time of the
    /// file, see [`sound_media::info`].
    pub fn overview(assets: &Assets, asset: &AudioAsset, cx: &mut App) -> Option<Arc<Overview>> {
        let info = sound_media::info(assets, asset).ok()?;
        let key = (assets.path(asset.asset_name()), info);
        let entity = Self::entity(cx);
        match entity.read(cx).known.get(&key) {
            Some(Known::Ready(overview)) => return Some(overview.clone()),
            Some(Known::Making | Known::Failed) => return None,
            None => {}
        }
        entity.update(cx, |waveforms, _| {
            waveforms.known.insert(key.clone(), Known::Making)
        });
        let (assets, asset) = (assets.clone(), asset.clone());
        let making = cx.background_spawn(async move {
            let audio = sound_media::load(&assets, &asset).ok()?;
            Some(Arc::new(Overview::of(&audio)))
        });
        cx.spawn(async move |cx| {
            let made = making.await;
            entity.update(cx, |waveforms, cx| {
                let known = match made {
                    Some(overview) => Known::Ready(overview),
                    None => Known::Failed,
                };
                waveforms.known.insert(key, known);
                waveforms.made += 1;
                cx.notify();
            });
        })
        .detach();
        None
    }
}
