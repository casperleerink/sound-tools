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

    /// The resolution for stretches of `length` frames: the coarsest whose peaks are no longer
    /// than that, so a stretch reads 1 to 8 of them. Its size in frames with it.
    fn level_for(&self, length: u64) -> (usize, u64) {
        let (mut level, mut size) = (0, FINEST_FRAMES);
        while level + 1 < self.levels.len() && size * COARSER as u64 <= length {
            level += 1;
            size *= COARSER as u64;
        }
        (level, size)
    }

    /// The loudest sample from frame `from` up to `to` at one resolution. Each peak counts for
    /// the stretch its first frame is in, so stretches next to each other share none and miss
    /// none. A stretch that holds no first frame gives the peak it lies in, so a waveform zoomed
    /// far in has no gaps.
    fn peak_at(&self, (level, size): (usize, u64), from: u64, to: u64) -> f32 {
        let Some(peaks) = self.levels.get(level) else {
            return 0.0;
        };
        let to = to.min(self.frames);
        if from >= to {
            return 0.0;
        }
        let index = |frame: u64| usize::try_from(frame).unwrap_or(usize::MAX);
        let (first, last) = (from.div_ceil(size), to.div_ceil(size));
        if first >= last {
            return peaks.get(index(from / size)).copied().unwrap_or(0.0);
        }
        let range = index(first)..index(last).min(peaks.len());
        let peaks = peaks.get(range).unwrap_or_default();
        peaks.iter().copied().fold(0.0, f32::max)
    }

    /// The loudest sample from frame `from` up to `to`, 0 to 1. Outside the file is silence.
    pub fn peak(&self, from: u64, to: u64) -> f32 {
        let level = self.level_for(to.saturating_sub(from));
        self.peak_at(level, from, to)
    }

    /// The peaks of the columns between `edges`, frames of the file from left to right, each
    /// column from one edge up to the next. All at one resolution, the one for their average
    /// width, so together they cover every frame and a single loud sample shows in a column at
    /// every zoom.
    pub fn peaks(&self, edges: &[u64]) -> Vec<f32> {
        let (Some(first), Some(last)) = (edges.first(), edges.last()) else {
            return Vec::new();
        };
        let columns = edges.len().saturating_sub(1).max(1) as u64;
        let level = self.level_for(last.saturating_sub(*first) / columns);
        let peaks = edges
            .windows(2)
            .map(|edge| self.peak_at(level, edge[0], edge[1]));
        peaks.collect()
    }

    /// The peaks of `count` columns of equal width over the file, from second `from` to second
    /// `to`: what a waveform of that many columns across that part of the file shows.
    pub fn columns(&self, from: f64, to: f64, count: usize) -> Vec<f32> {
        let rate = f64::from(self.sample_rate);
        let step = (to - from) / count.max(1) as f64;
        let edges: Vec<u64> = (0..=count)
            .map(|edge| ((from + step * edge as f64) * rate).max(0.0) as u64)
            .collect();
        self.peaks(&edges)
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
        assert_eq!(overview.peak(20_000, 90_000), 0.0);
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

    /// One loud sample shows in a column at every width of column, from a frame to the whole
    /// file, wherever the columns start.
    #[test]
    fn a_single_sample_click_shows_at_every_zoom() {
        let frames = 1_000_000_u64;
        let click = 654_321_u64;
        let mut samples = vec![0_i16; frames as usize];
        samples[click as usize] = 32_000;
        let overview = Overview::of(&wav(&samples));
        let mut width = 1_u64;
        while width <= frames {
            for offset in [0, width / 3, width / 2] {
                let edges: Vec<u64> = (0..)
                    .map(|column| offset + column * width)
                    .take_while(|edge| *edge <= frames + width)
                    .collect();
                let peaks = overview.peaks(&edges);
                let shown = peaks.iter().filter(|peak| **peak > 0.9).count();
                assert!(
                    shown >= 1,
                    "width {width}, offset {offset}: the click is lost"
                );
                // Wider than a finest peak, it shows in one column only.
                if width >= FINEST_FRAMES * 2 {
                    assert_eq!(shown, 1, "width {width}, offset {offset}");
                }
            }
            width = width * 3 + 1;
        }
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
