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
use sound_media::{AudioAsset, Cached, Info, Overview};

/// Where the waveform of one file is.
enum State {
    /// It is being made on a background thread.
    Making,
    Ready(Arc<Overview>),
    /// The file could not be read. It is tried again when the file changes.
    Failed,
}

/// The waveform of one file, and what the file was when it was made: a file replaced by
/// another of another length gets a new one, which takes the place of the old one.
struct Entry {
    info: Option<Info>,
    state: State,
}

/// The overviews, one per file.
#[derive(Default)]
pub struct Waveforms {
    known: HashMap<PathBuf, Entry>,
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

    /// How many overviews are being made now.
    pub fn making(&self) -> usize {
        let entries = self.known.values();
        entries
            .filter(|entry| matches!(entry.state, State::Making))
            .count()
    }

    /// The overview of a file of a project, when it is made. It never touches the disk: what
    /// the file is comes from [`sound_media::cached`]. The first time it is asked for, and after
    /// the file changed, the file is looked at, read and its overview made on a background
    /// thread, and this returns `None` until then, as it does for a file that is not there or
    /// does not play. The cache notifies its observers when an overview is ready, and what the
    /// background thread learned of the file is then in [`sound_media::cached`] too.
    pub fn overview(assets: &Assets, asset: &AudioAsset, cx: &mut App) -> Option<Arc<Overview>> {
        let info = match sound_media::cached(assets, asset) {
            Cached::Plays(info) => Some(info),
            Cached::Unknown => None,
            Cached::DoesNotPlay(_) | Cached::Missing => return None,
        };
        let path = assets.path(asset.asset_name());
        if let Some(GlobalWaveforms(entity)) = cx.try_global::<GlobalWaveforms>()
            && let Some(entry) = entity.read(cx).known.get(&path)
        {
            let same = entry.info == info || (info.is_none() && entry.info.is_some());
            match &entry.state {
                State::Ready(overview) if same => return Some(overview.clone()),
                State::Making => return None,
                State::Failed if same => return None,
                State::Ready(_) | State::Failed => {}
            }
        }
        // A view asks while it draws, where no entity may change: it starts after the frame.
        let (assets, asset) = (assets.clone(), asset.clone());
        cx.defer(move |cx| Self::make(assets, asset, path, info, cx));
        None
    }

    /// Reads the file and makes its overview on a background thread.
    fn make(assets: Assets, asset: AudioAsset, path: PathBuf, info: Option<Info>, cx: &mut App) {
        let entity = Self::entity(cx);
        // Asked twice in one frame: the first one makes it.
        let known = entity.read(cx).known.get(&path);
        if known.is_some_and(|entry| matches!(entry.state, State::Making)) {
            return;
        }
        let making = Entry {
            info,
            state: State::Making,
        };
        entity.update(cx, |waveforms, _| {
            waveforms.known.insert(path.clone(), making)
        });
        let work = cx.background_spawn(async move {
            // What the file is, for the cache of sound-media, then the file and its overview.
            let info = sound_media::info(&assets, &asset).ok();
            let audio = sound_media::load(&assets, &asset).ok();
            (info, audio.map(|audio| Arc::new(Overview::of(&audio))))
        });
        cx.spawn(async move |cx| {
            let (info, overview) = work.await;
            entity.update(cx, |waveforms, cx| {
                let state = match overview {
                    Some(overview) => State::Ready(overview),
                    None => State::Failed,
                };
                waveforms.known.insert(path, Entry { info, state });
                waveforms.made += 1;
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    /// `seconds` of mono 16-bit samples at 48 kHz, all at `level`, as a WAV file.
    fn wav(seconds: u32, level: i16) -> Vec<u8> {
        let data = seconds * 48_000 * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&96_000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for _ in 0..seconds * 48_000 {
            bytes.extend_from_slice(&level.to_le_bytes());
        }
        bytes
    }

    /// The thread that asks gets `None` at once and goes on drawing: the file is read and the
    /// overview made by a task of the background executor, which a GPUI test runs only when the
    /// test lets it. So nothing of it ran on the asking thread.
    #[gpui::test]
    fn an_overview_is_made_away_from_the_thread_that_asks_for_it(cx: &mut TestAppContext) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("assets/audio/tone.wav");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, wav(10, 16_384)).unwrap();
        let assets = Assets::new(folder.path());
        let asset = AudioAsset::new("tone.wav").unwrap();

        let asked = cx.update(|cx| Waveforms::overview(&assets, &asset, cx));
        assert!(asked.is_none());
        // Asked again while it is made: still nothing, and no second one starts.
        let again = cx.update(|cx| Waveforms::overview(&assets, &asset, cx));
        assert!(again.is_none());
        let counts = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                let waveforms = Waveforms::entity(cx);
                let waveforms = waveforms.read(cx);
                (waveforms.making(), waveforms.made())
            })
        };
        assert_eq!(counts(cx), (1, 0));

        cx.run_until_parked();
        assert_eq!(counts(cx), (0, 1));
        let overview = cx
            .update(|cx| Waveforms::overview(&assets, &asset, cx))
            .unwrap();
        assert_eq!(overview.frames(), 480_000);
        assert_eq!(overview.peak(0, 480_000), 0.5);
        // A file that is not there has none: the background thread finds it missing, and it
        // is not asked for again.
        let missing = AudioAsset::new("gone.wav").unwrap();
        assert!(
            cx.update(|cx| Waveforms::overview(&assets, &missing, cx))
                .is_none()
        );
        cx.run_until_parked();
        assert!(
            cx.update(|cx| Waveforms::overview(&assets, &missing, cx))
                .is_none()
        );
        assert_eq!(counts(cx), (0, 2));

        // The same file replaced by a longer one gets a new overview, in the place of the old.
        std::fs::write(&path, wav(12, 8_192)).unwrap();
        sound_media::info(&assets, &asset).unwrap();
        assert!(
            cx.update(|cx| Waveforms::overview(&assets, &asset, cx))
                .is_none()
        );
        cx.run_until_parked();
        let overview = cx
            .update(|cx| Waveforms::overview(&assets, &asset, cx))
            .unwrap();
        assert_eq!(overview.frames(), 576_000);
        let entries = cx.update(|cx| Waveforms::entity(cx).read(cx).known.len());
        assert_eq!(entries, 2);
    }
}
