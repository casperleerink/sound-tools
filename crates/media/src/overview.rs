//! The overview of a file: the loudest sample of each short stretch of it, at a few
//! resolutions, which is what a waveform draws. It is made from the file and kept in memory by
//! whoever draws it, never written into the project.

use crate::file::Audio;

/// Frames per peak at the finest resolution.
pub const FINEST_FRAMES: u64 = 64;
/// Each coarser resolution has one peak per this many peaks of the one under it.
const COARSER: usize = 8;
/// Frames read at a time. A whole number of the finest stretches.
const BLOCK: usize = 64 * FINEST_FRAMES as usize;

/// The loudest sample of every stretch of a file, left and right together, from 0 to 1.
///
/// A column of a waveform of any width asks for its stretch with [`Overview::peak`], which
/// looks at a few peaks of the resolution that fits: a 10-minute file at 48 kHz is 450,000
/// peaks at the finest, 1.8 MB, and one column costs the same whatever its width.
#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    frames: u64,
    sample_rate: u32,
    /// `levels[0]` has one peak per [`FINEST_FRAMES`] frames, each next one per [`COARSER`]
    /// times as many.
    levels: Vec<Vec<f32>>,
}

impl Overview {
    /// Reads the whole file once. It takes about as long as reading the file: do it away from
    /// the thread that draws.
    pub fn of(audio: &Audio) -> Self {
        let frames = audio.frames();
        let mut finest = Vec::with_capacity(frames.div_ceil(FINEST_FRAMES) as usize);
        let mut block = vec![[0.0_f32; 2]; BLOCK];
        let mut start = 0_u64;
        while start < frames {
            let count = (frames - start).min(BLOCK as u64) as usize;
            let read = &mut block[..count];
            audio.read(start as i64, read);
            for stretch in read.chunks(FINEST_FRAMES as usize) {
                let peak = stretch.iter().fold(0.0_f32, |peak, [left, right]| {
                    peak.max(left.abs()).max(right.abs())
                });
                // A float file may go over full scale, and a waveform stops at its edge.
                finest.push(peak.min(1.0));
            }
            start += count as u64;
        }
        let mut levels = vec![finest];
        while let Some(last) = levels.last()
            && last.len() > 1
        {
            let coarser = last
                .chunks(COARSER)
                .map(|peaks| peaks.iter().copied().fold(0.0, f32::max))
                .collect();
            levels.push(coarser);
        }
        Self {
            frames,
            sample_rate: audio.sample_rate(),
            levels,
        }
    }

    /// How many frames the file has.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// How long the file plays at its own rate.
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate.max(1))
    }

    /// The loudest sample from frame `from` up to `to`, 0 to 1. A stretch shorter than the finest
    /// resolution gives the peak of the stretch it falls in, so a waveform zoomed far in is not
    /// empty between its peaks. Outside the file is silence.
    pub fn peak(&self, from: u64, to: u64) -> f32 {
        let to = to.min(self.frames);
        if from >= to {
            return 0.0;
        }
        let length = to - from;
        // The coarsest resolution with at least eight of its peaks in the stretch, so a column
        // reads 8 to 64 peaks whatever its width.
        let mut level = 0;
        let mut size = FINEST_FRAMES;
        while level + 1 < self.levels.len() && size * (COARSER * COARSER) as u64 <= length {
            level += 1;
            size *= COARSER as u64;
        }
        // Each peak counts for the stretch its first frame is in, so two columns next to each
        // other never both show one loud sample.
        let (first, last) = (from.div_ceil(size), to.div_ceil(size));
        if first >= last {
            let finest = self.levels.first().and_then(|peaks| {
                let index = usize::try_from(from / FINEST_FRAMES).ok()?;
                peaks.get(index).copied()
            });
            return finest.unwrap_or(0.0);
        }
        let Some(peaks) = self.levels.get(level) else {
            return 0.0;
        };
        let range = usize::try_from(first).unwrap_or(usize::MAX)
            ..usize::try_from(last).unwrap_or(usize::MAX).min(peaks.len());
        peaks
            .get(range)
            .unwrap_or_default()
            .iter()
            .copied()
            .fold(0.0, f32::max)
    }

    /// The peaks of `count` columns of equal width over the file, from second `from` to second
    /// `to`: what a waveform of that many columns across that part of the file shows.
    pub fn columns(&self, from: f64, to: f64, count: usize) -> Vec<f32> {
        let rate = f64::from(self.sample_rate);
        let step = (to - from) / count.max(1) as f64;
        (0..count)
            .map(|column| {
                let start = from + step * column as f64;
                let frame = |seconds: f64| (seconds * rate).max(0.0) as u64;
                self.peak(frame(start), frame(start + step).max(frame(start) + 1))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV file of 16-bit mono samples at 48 kHz.
    fn wav(samples: &[i16]) -> Audio {
        let mut bytes = Vec::new();
        let data = samples.len() * 2;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&96_000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data as u32).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        Audio::parse(bytes).unwrap()
    }

    #[test]
    fn a_peak_is_the_loudest_sample_of_its_stretch_at_any_width() {
        // Silence with one loud sample at frame 15,000 and a quieter one at 105,000.
        let mut samples = vec![0_i16; 200_000];
        samples[15_000] = -16_384;
        samples[105_000] = 8_192;
        let overview = Overview::of(&wav(&samples));
        assert_eq!(overview.frames(), 200_000);
        assert_eq!(overview.peak(0, 200_000), 0.5);
        assert_eq!(overview.peak(14_000, 16_000), 0.5);
        assert_eq!(overview.peak(20_000, 100_000), 0.0);
        assert_eq!(overview.peak(104_990, 105_010), 0.25);
        assert_eq!(overview.peak(20_000, 200_000), 0.25);
        // Past the end, and nothing asked for.
        assert_eq!(overview.peak(200_000, 300_000), 0.0);
        assert_eq!(overview.peak(5, 5), 0.0);
        let columns = overview.columns(0.0, 200_000.0 / 48_000.0, 20);
        assert_eq!(columns.len(), 20);
        assert_eq!(columns[1], 0.5);
        assert_eq!(columns[10], 0.25);
        assert_eq!(columns.iter().filter(|peak| **peak > 0.0).count(), 2);
    }

    #[test]
    fn a_stretch_inside_one_peak_gives_that_peak() {
        let mut samples = vec![0_i16; 1_000];
        samples[70] = 32_767;
        let overview = Overview::of(&wav(&samples));
        // Frames 64 to 127 are one peak of the finest resolution.
        assert!(overview.peak(100, 101) > 0.99);
        assert_eq!(overview.peak(10, 11), 0.0);
    }

    /// How long a waveform of a 10-minute file takes to be ready: reading the file and making
    /// its overview, on the background thread that does it in the window. Run by hand:
    /// `cargo nextest run -p sound-media --run-ignored only ten_minute --no-capture`.
    #[test]
    #[ignore]
    fn the_overview_of_a_ten_minute_file_is_ready_in_a_stated_time() {
        use std::time::Instant;

        // Stereo, 24-bit, 48 kHz: 173 MB, the kind of file a recording gives.
        let frames: u32 = 10 * 60 * 48_000;
        let data = frames * 6;
        let mut bytes = Vec::with_capacity(44 + data as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&(48_000_u32 * 6).to_le_bytes());
        bytes.extend_from_slice(&6_u16.to_le_bytes());
        bytes.extend_from_slice(&24_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for frame in 0..frames {
            let sample = ((frame as f32 * 0.01).sin() * 4_000_000.) as i32;
            let [a, b, c, _] = sample.to_le_bytes();
            bytes.extend_from_slice(&[a, b, c, a, b, c]);
        }
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("assets/audio/ten-minutes.wav");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let assets = sound_core::Assets::new(folder.path());
        let asset = crate::AudioAsset::new("ten-minutes.wav").unwrap();

        let started = Instant::now();
        let audio = crate::load(&assets, &asset).unwrap();
        let loaded = started.elapsed();
        let overview = Overview::of(&audio);
        let total = started.elapsed();
        println!(
            "10 minutes, stereo 24-bit 48 kHz: read {loaded:?}, overview {:?}, ready after {total:?}",
            total - loaded
        );
        assert_eq!(overview.frames(), u64::from(frames));
        // Once in memory, as it is in a project that plays it, only the overview is left to do.
        let started = Instant::now();
        let again = Overview::of(&audio);
        println!(
            "the overview alone, the file in memory: {:?}",
            started.elapsed()
        );
        assert_eq!(again, overview);
    }
}
