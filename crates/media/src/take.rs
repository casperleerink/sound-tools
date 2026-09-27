//! A take on its way into `assets/audio/`: a WAV file that grows while it records, and the
//! overview of what it holds so far, which the window draws as the take grows.

use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::sync::{Arc, Mutex, PoisonError};

use sound_core::{AssetName, Assets};

use crate::overview::{FINEST_FRAMES, Overview};
use crate::{AUDIO_FOLDER, AudioAsset, Imported, MediaError, asset_part, invalid, load};

/// The overview of a take while it records, shared by the thread that writes the take and
/// the one that draws it. Clones share it.
#[derive(Clone, Debug)]
pub struct TakeOverview(Arc<Mutex<Overview>>);

impl TakeOverview {
    /// Reads the overview as it is now. The writer holds it only to add one peak, never while
    /// it writes the file, so this never waits for a disk.
    pub fn read<R>(&self, read: impl FnOnce(&Overview) -> R) -> R {
        read(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn push(&self, peak: f32, frames: u64) {
        let mut overview = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        overview.push(peak, frames);
    }
}

/// A WAV file of 32-bit float samples under `assets/audio/`, written as the take records.
///
/// Every second of audio the header is written for the length so far, so a take whose program
/// ends without finishing it is a valid file up to that second.
pub struct TakeFile {
    assets: Assets,
    asset: AudioAsset,
    writer: hound::WavWriter<BufWriter<File>>,
    channels: usize,
    sample_rate: u32,
    frames: u64,
    frames_at_header: u64,
    overview: TakeOverview,
    /// The loudest sample of the stretch of the overview that is not full yet, and its frames.
    stretch: (f32, u64),
}

impl TakeFile {
    /// Creates `assets/audio/<name>-1.wav`, or the next free number, and opens it for the
    /// take. A file that is there is never opened, so no take is written over another.
    /// `channels` is 1 or 2: a take is mono or stereo.
    pub fn create(
        assets: &Assets,
        name: &str,
        sample_rate: u32,
        channels: usize,
    ) -> Result<Self, MediaError> {
        let channels = channels.clamp(1, 2);
        let wanted = AssetName::new(AUDIO_FOLDER, &asset_part(name, "take"), "wav");
        let wanted = wanted.map_err(invalid)?;
        let io_error = |path: String, source| MediaError::Io { path, source };
        let created = assets.create(&wanted, &[]).map_err(|error| {
            io_error(
                AUDIO_FOLDER.to_string(),
                std::io::Error::other(error.to_string()),
            )
        })?;
        let asset = AudioAsset(created);
        let path = assets.path(asset.asset_name());
        let file = OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|source| io_error(asset.project_path(), source))?;
        let spec = hound::WavSpec {
            channels: channels as u16,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let writer = hound::WavWriter::new(BufWriter::new(file), spec)
            .map_err(|error| io_error(asset.project_path(), std::io::Error::other(error)))?;
        Ok(Self {
            assets: assets.clone(),
            asset,
            writer,
            channels,
            sample_rate,
            frames: 0,
            frames_at_header: 0,
            overview: TakeOverview(Arc::new(Mutex::new(Overview::growing(sample_rate)))),
            stretch: (0.0, 0),
        })
    }

    /// The name a clip gives the take.
    pub fn asset(&self) -> &AudioAsset {
        &self.asset
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// What the take holds so far, for its waveform while it records.
    pub fn overview(&self) -> TakeOverview {
        self.overview.clone()
    }

    /// Appends frames, interleaved in the channels of the take.
    pub fn write(&mut self, samples: &[f32]) -> Result<(), MediaError> {
        for frame in samples.chunks_exact(self.channels) {
            for sample in frame {
                self.writer
                    .write_sample(*sample)
                    .map_err(|error| self.error(error))?;
            }
            let loudest = frame.iter().fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            self.stretch = (self.stretch.0.max(loudest), self.stretch.1 + 1);
            if self.stretch.1 == FINEST_FRAMES {
                self.overview.push(self.stretch.0, self.stretch.1);
                self.stretch = (0.0, 0);
            }
            self.frames += 1;
        }
        if self.frames - self.frames_at_header >= u64::from(self.sample_rate) {
            self.frames_at_header = self.frames;
            self.writer.flush().map_err(|error| self.error(error))?;
        }
        Ok(())
    }

    /// Writes the header for the whole length and closes the file. Gives the take in memory,
    /// as an import does: hold it until its clip is added, and the track that plays the clip
    /// reads nothing.
    pub fn finish(self) -> Result<Imported, MediaError> {
        if self.stretch.1 > 0 {
            self.overview.push(self.stretch.0, self.stretch.1);
        }
        let (assets, asset) = (self.assets.clone(), self.asset.clone());
        self.writer.finalize().map_err(|error| MediaError::Io {
            path: asset.project_path(),
            source: std::io::Error::other(error),
        })?;
        let audio = load(&assets, &asset)?;
        Ok(Imported { asset, audio })
    }

    fn error(&self, error: hound::Error) -> MediaError {
        MediaError::Io {
            path: self.asset.project_path(),
            source: std::io::Error::other(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_take_is_a_new_wav_file_that_plays_what_was_written() {
        let folder = tempfile::tempdir().unwrap();
        let assets = Assets::new(folder.path());
        let mut mono = TakeFile::create(&assets, "Voice take", 48_000, 1).unwrap();
        let mut stereo = TakeFile::create(&assets, "voice-take", 44_100, 2).unwrap();
        assert_eq!(mono.asset().to_string(), "voice-take-1.wav");
        assert_eq!(stereo.asset().to_string(), "voice-take-2.wav");
        let samples: Vec<f32> = (0..1_000).map(|n| (n as f32 / 1_000.0) - 0.5).collect();
        mono.write(&samples).unwrap();
        stereo.write(&samples).unwrap();
        assert_eq!((mono.frames(), stereo.frames()), (1_000, 500));
        let overview = mono.overview();

        let mono = mono.finish().unwrap();
        assert_eq!(mono.audio.frames(), 1_000);
        assert_eq!(mono.audio.channels(), 1);
        let mut read = vec![[0.0; 2]; 1_000];
        mono.audio.read(0, &mut read);
        assert!(read.iter().zip(&samples).all(|(frame, sample)| frame[0] == *sample));
        // The waveform drawn while it recorded is the one of the file.
        let whole = Overview::of(&mono.audio);
        assert_eq!(overview.read(Overview::clone), whole);

        let stereo = stereo.finish().unwrap();
        assert_eq!(stereo.audio.sample_rate(), 44_100);
        let mut read = vec![[0.0; 2]; 500];
        stereo.audio.read(0, &mut read);
        assert_eq!(read[10], [samples[20], samples[21]]);
        // Any reader of WAV files reads it: 32-bit float.
        let path = assets.path(stereo.asset.asset_name());
        let spec = hound::WavReader::open(path).unwrap().spec();
        assert_eq!((spec.channels, spec.bits_per_sample), (2, 32));
        assert_eq!(spec.sample_format, hound::SampleFormat::Float);
    }

    /// A program that ends in the middle of a take leaves a file that plays up to the last
    /// whole second it wrote.
    #[test]
    fn a_take_that_is_never_finished_still_plays_up_to_its_last_second() {
        let folder = tempfile::tempdir().unwrap();
        let assets = Assets::new(folder.path());
        let mut take = TakeFile::create(&assets, "take", 8_000, 1).unwrap();
        for _ in 0..5 {
            take.write(&[0.25; 4_000]).unwrap();
        }
        let path = assets.path(take.asset().asset_name());
        // No finish and no drop: as if the process ended here.
        std::mem::forget(take);
        let audio = crate::Audio::parse(std::fs::read(path).unwrap()).unwrap();
        assert_eq!(audio.frames(), 16_000);
    }
}
