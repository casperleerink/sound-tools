//! The waveforms of the audio files of a project: an [`Overview`] of each file something
//! draws, made on a thread of its own the first time it is asked for and kept in memory while
//! the app runs. Never written into the project folder. One cache for the app, so a song's
//! regions and the Sampler draw from the same overview of a file.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use sound_core::Assets;

use crate::{AudioAsset, Cached, Info, Overview};

/// What [`Waves`] calls when an overview is ready, so the owner draws again.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Where the waveform of one file is.
enum State {
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
pub struct Waves {
    known: Arc<Mutex<HashMap<PathBuf, Entry>>>,
    /// How many overviews were made, for a test that waits for one.
    made: Arc<AtomicU64>,
    wake: Wake,
}

impl Waves {
    pub fn new(wake: Wake) -> Self {
        Self {
            known: Arc::default(),
            made: Arc::default(),
            wake,
        }
    }

    /// The overview of a file of a project, when it is made. It never touches the disk: what
    /// the file is comes from [`crate::cached`]. The first time it is asked for, and after the
    /// file changed, the file is read and its overview made on a thread of its own, which
    /// wakes the owner, and this gives `None` until then, as it does for a file that is not
    /// there or does not play.
    pub fn overview(&self, assets: &Assets, asset: &AudioAsset) -> Option<Arc<Overview>> {
        let info = match crate::cached(assets, asset) {
            Cached::Plays(info) => Some(info),
            Cached::Unknown => None,
            Cached::DoesNotPlay(_) | Cached::Missing => return None,
        };
        let path = assets.path(asset.asset_name());
        let mut known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = known.get(&path) {
            let same = entry.info == info || (info.is_none() && entry.info.is_some());
            match &entry.state {
                State::Ready(overview) if same => return Some(overview.clone()),
                State::Making => return None,
                State::Failed if same => return None,
                State::Ready(_) | State::Failed => {}
            }
        }
        let making = Entry {
            info,
            state: State::Making,
        };
        known.insert(path.clone(), making);
        drop(known);
        let (assets, asset) = (assets.clone(), asset.clone());
        let (known, made, wake) = (self.known.clone(), self.made.clone(), self.wake.clone());
        let started = std::thread::Builder::new()
            .name("waveform".into())
            .spawn(move || {
                // What the file is, for the cache of the crate, then the file and its overview.
                let info = crate::info(&assets, &asset).ok();
                let audio = crate::load(&assets, &asset).ok();
                let state = match audio {
                    Some(audio) => State::Ready(Arc::new(Overview::of(&audio))),
                    None => State::Failed,
                };
                let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
                known.insert(path, Entry { info, state });
                made.fetch_add(1, Ordering::Relaxed);
                drop(known);
                wake();
            });
        if let Err(error) = started {
            eprintln!("error: no thread to make a waveform on: {error}");
        }
        None
    }

    /// The loudest sample of each of `width` columns of equal width over the file, from second
    /// `from` to second `to`, once its overview is made: what a waveform `width` pixels wide
    /// draws of that part.
    pub fn columns(
        &self,
        assets: &Assets,
        asset: &AudioAsset,
        (from, to): (f64, f64),
        width: usize,
    ) -> Option<Vec<f32>> {
        let overview = self.overview(assets, asset)?;
        Some(overview.columns(from, to, width))
    }

    /// How many overviews were made since the cache was.
    pub fn made(&self) -> u64 {
        self.made.load(Ordering::Relaxed)
    }

    /// How many overviews are being made now.
    pub fn making(&self) -> usize {
        let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);
        let making = known
            .values()
            .filter(|entry| matches!(entry.state, State::Making));
        making.count()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

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

    /// Waits until the thread woke the owner `count` times.
    fn until_woken(woken: &AtomicU64, count: u64) {
        let start = Instant::now();
        while woken.load(Ordering::Relaxed) < count {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "no overview came"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// The thread that asks gets `None` at once and goes on drawing: the file is read and the
    /// overview made on a thread of its own, which wakes it.
    #[test]
    fn an_overview_is_made_away_from_the_thread_that_asks_for_it() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("assets/audio/tone.wav");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, wav(10, 16_384)).unwrap();
        let assets = Assets::new(folder.path());
        let asset = AudioAsset::new("tone.wav").unwrap();
        let woken = Arc::new(AtomicU64::new(0));
        let waves = Waves::new({
            let woken = woken.clone();
            Arc::new(move || {
                woken.fetch_add(1, Ordering::Relaxed);
            })
        });

        assert!(waves.overview(&assets, &asset).is_none());
        // Asked again while it is made: no second one starts.
        waves.overview(&assets, &asset);
        assert_eq!(waves.made() + waves.making() as u64, 1);
        until_woken(&woken, 1);
        let overview = waves.overview(&assets, &asset).unwrap();
        assert_eq!(overview.frames(), 480_000);
        assert_eq!(overview.peak(0, 480_000), 0.5);
        let columns = waves.columns(&assets, &asset, (0.0, 10.0), 4).unwrap();
        assert_eq!(columns, [0.5; 4]);

        // A file that is not there has none: the thread finds it missing, and it is not asked
        // for again.
        let missing = AudioAsset::new("gone.wav").unwrap();
        assert!(waves.overview(&assets, &missing).is_none());
        until_woken(&woken, 2);
        assert!(waves.overview(&assets, &missing).is_none());
        assert_eq!((waves.made(), waves.making()), (2, 0));

        // The same file replaced by a longer one gets a new overview, in the place of the old.
        std::fs::write(&path, wav(12, 8_192)).unwrap();
        crate::info(&assets, &asset).unwrap();
        assert!(waves.overview(&assets, &asset).is_none());
        until_woken(&woken, 3);
        let overview = waves.overview(&assets, &asset).unwrap();
        assert_eq!(overview.frames(), 576_000);
        assert_eq!(waves.known.lock().unwrap().len(), 2);
    }
}
