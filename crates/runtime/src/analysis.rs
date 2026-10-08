//! What `--analyze` measures in a render or an audio file: loudness, peaks, the level of each
//! frequency band, the stereo width and the pitch, of the whole and of each row of the report.
//! An agent cannot listen, so these numbers are how it checks that what it wrote sounds as it
//! meant.
//!
//! Loudness is ITU-R BS.1770 (LUFS): the sound weighted like hearing (K-weighting), in blocks of
//! 400 ms every 100 ms, and the two gates of the loudness of the whole. A plain level (RMS)
//! reads short bright hits as much quieter than they sound; the weighting and the loudest block
//! of each row are what catch that. The true peak is the highest sample at four times the rate,
//! the peak a converter or an encoder meets between the samples.

mod pitch;
pub mod report;

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use sound_core::{Oversampler, OversamplingFilters};

pub use pitch::MeasuredPitch;
use pitch::{Pitch, PitchFinder, Pitches};

/// Every render is stereo, and a file is read as stereo.
const CHANNELS: usize = 2;

/// The bands, by name and lowest frequency in Hz. Each runs up to where the next starts.
pub const BANDS: [(&str, f64); 6] = [
    ("sub", 0.0),
    ("bass", 60.0),
    ("lowmid", 250.0),
    ("mid", 500.0),
    ("highmid", 2_000.0),
    ("high", 6_000.0),
];

/// The length of one spectrum, in frames: 12 Hz apart at 48 kHz, so the sub band has bins of
/// its own.
const WINDOW: usize = 4096;
/// Spectra overlap by three quarters: under the Hann window every frame then counts the same.
const HOP: usize = WINDOW / 4;

/// BS.1770 measures in blocks of 400 ms, four of 100 ms.
const BLOCKS_PER_WINDOW: usize = 4;
/// Under this a block is silence and does not count towards the loudness of the whole.
const ABSOLUTE_GATE: f64 = -70.0;
/// A block this far under the loudness of the louder blocks does not count either: the quiet
/// stretches of a piece do not pull its loudness down.
const RELATIVE_GATE: f64 = 10.0;
/// How many frames late the oversampled sound comes out: half of the way up and down again.
const PEAK_DELAY: u64 = Oversampler::DELAY_FRAMES as u64 / 2;
/// Under this a level is shown as silence: -100 dB.
const SILENT_POWER: f64 = 1e-10;

/// What the meter measured: of the whole, and of each row. `None` is silence.
#[derive(Debug, Clone, PartialEq)]
pub struct Measures {
    /// The loudness of the whole, gated, in LUFS. `None` for silence or a sound shorter than
    /// one block of 400 ms.
    pub integrated: Option<f64>,
    /// The loudest block of 400 ms, in LUFS.
    pub max_momentary: Option<f64>,
    /// In dBTP, with the frame it is at.
    pub true_peak: Option<(f64, u64)>,
    /// The first frame with a sample that is not a number or is infinite. Such samples are
    /// measured as silence.
    pub not_a_number: Option<u64>,
    pub pitch: Option<MeasuredPitch>,
    pub frames: u64,
    pub rows: Vec<RowMeasures>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowMeasures {
    /// The loudness of the whole row, not gated, in LUFS.
    pub loudness: Option<f64>,
    /// The loudest block of 400 ms that ends in this row, in LUFS.
    pub max_momentary: Option<f64>,
    /// In dBTP.
    pub true_peak: Option<f64>,
    /// The level of each band of [`BANDS`], RMS in dB, both channels together.
    pub bands: [Option<f64>; BANDS.len()],
    /// The share of the sound that is in the side, in percent: 0 is mono, 50 is as wide as
    /// two unrelated channels, above 50 the channels cancel when summed to mono.
    pub width: Option<f64>,
    pub pitch: Option<MeasuredPitch>,
}

/// Measures a stereo sound pushed through it in pieces of any size, row by row.
pub struct Meter {
    /// Where each row starts, in frames from the start, the first at 0.
    row_starts: Vec<u64>,
    rows: Vec<RowSums>,
    row: usize,
    frame: u64,
    weighting: [KWeighting; CHANNELS],
    /// The weighted power of the block of 100 ms being filled, summed over its frames.
    block_sum: f64,
    block_frames: u64,
    frames_per_block: u64,
    /// The mean weighted power of the last four blocks, oldest first. What [`Self::warm_up`]
    /// heard, or silence before the start.
    recent: [f64; BLOCKS_PER_WINDOW],
    /// Whether the block being filled started after the start, and how many such blocks
    /// ended. From the fourth on, a block of 400 ms holds no warm-up, and counts for the whole.
    block_measured: bool,
    measured_blocks: usize,
    /// The power of every whole block of 400 ms, for the gates.
    windows: Vec<f64>,
    max_momentary: f64,
    /// The first row in which no block of 400 ms has ended yet, and the last such block.
    waiting_row: usize,
    last_momentary: Option<f64>,
    oversampling: OversamplingFilters,
    oversamplers: [Oversampler; CHANNELS],
    true_peak: (f32, u64),
    not_a_number: Option<u64>,
    spectrum: Spectrum,
    pitches: Pitches,
}

/// What a row adds up while the sound passes.
#[derive(Default, Clone)]
struct RowSums {
    frames: u64,
    weighted: f64,
    max_momentary: Option<f64>,
    true_peak: f32,
    bands: [f64; BANDS.len()],
    spectra: u32,
    mid: f64,
    side: f64,
    pitches: Pitches,
}

impl Meter {
    /// A meter for a sound at `sample_rate` whose rows start at `row_starts`, in frames from its
    /// start, in order. A first row at 0 is added when it is missing.
    pub fn new(sample_rate: u32, mut row_starts: Vec<u64>) -> Self {
        if row_starts.first() != Some(&0) {
            row_starts.insert(0, 0);
        }
        let rate = f64::from(sample_rate);
        Self {
            rows: vec![RowSums::default(); row_starts.len()],
            row_starts,
            row: 0,
            frame: 0,
            weighting: [KWeighting::new(rate); CHANNELS],
            block_sum: 0.0,
            block_frames: 0,
            frames_per_block: (rate / 10.0).round() as u64,
            recent: [0.0; BLOCKS_PER_WINDOW],
            block_measured: false,
            measured_blocks: 0,
            windows: Vec::new(),
            max_momentary: 0.0,
            waiting_row: 0,
            last_momentary: None,
            oversampling: OversamplingFilters::new(),
            oversamplers: [Oversampler::new(); CHANNELS],
            true_peak: (0.0, 0),
            not_a_number: None,
            spectrum: Spectrum::new(rate),
            pitches: Pitches::default(),
        }
    }

    /// Measures `samples`, interleaved by channel, after what came before.
    pub fn push(&mut self, samples: &[f32]) {
        for frame in samples.as_chunks::<CHANNELS>().0 {
            let mut values = *frame;
            for value in &mut values {
                if !value.is_finite() {
                    self.not_a_number.get_or_insert(self.frame);
                    *value = 0.0;
                }
            }
            self.measure(values);
        }
    }

    /// Hears `samples` from before the start, interleaved by channel, and measures none of them:
    /// the filters, the blocks of 400 ms and the spectrum then start from what was sounding, as
    /// they would in the middle of the piece.
    pub fn warm_up(&mut self, samples: &[f32]) {
        for frame in samples.as_chunks::<CHANNELS>().0 {
            let values = frame.map(|value| match value.is_finite() {
                true => value,
                false => 0.0,
            });
            if self.block_frames == 0 {
                self.block_measured = false;
            }
            self.hear(values);
            self.spectrum.push(values);
        }
    }

    /// The weighting, the blocks and the oversampling, which run before the start too. Gives the
    /// weighted power of the frame, the highest sample at four times the rate of the frame
    /// [`PEAK_DELAY`] before it, and the momentary loudness of the block it ends, if it ends one.
    fn hear(&mut self, values: [f32; CHANNELS]) -> (f64, f32, Option<f64>) {
        let mut power = 0.0;
        for (value, weighting) in values.iter().zip(&mut self.weighting) {
            let weighted = weighting.process(f64::from(*value));
            power += weighted * weighted;
        }
        let oversampled = self.oversampled_peak(values);
        self.block_sum += power;
        self.block_frames += 1;
        if self.block_frames < self.frames_per_block {
            return (power, oversampled, None);
        }
        // A block of 100 ms is full: the block of 400 ms that ends with it is the momentary
        // loudness of this moment.
        self.recent.rotate_left(1);
        self.recent[BLOCKS_PER_WINDOW - 1] = self.block_sum / self.block_frames as f64;
        (self.block_sum, self.block_frames) = (0.0, 0);
        let momentary = self.recent.iter().sum::<f64>() / BLOCKS_PER_WINDOW as f64;
        (power, oversampled, Some(momentary))
    }

    /// The highest sample at four times the rate, of the frame [`PEAK_DELAY`] before `values`.
    fn oversampled_peak(&mut self, values: [f32; CHANNELS]) -> f32 {
        let mut peak = 0.0_f32;
        for (value, oversampler) in values.iter().zip(&mut self.oversamplers) {
            let mut four = [0.0; Oversampler::FACTOR];
            oversampler.up(&self.oversampling, &[*value], &mut four);
            peak = four
                .iter()
                .fold(peak, |peak, sample| peak.max(sample.abs()));
        }
        peak
    }

    /// A peak of `frame`, from the start: the row it is in and the whole.
    fn add_peak(&mut self, frame: u64, peak: f32) {
        let row = self.row_starts.partition_point(|start| *start <= frame);
        if let Some(row) = self.rows.get_mut(row.saturating_sub(1)) {
            row.true_peak = row.true_peak.max(peak);
        }
        if peak > self.true_peak.0 {
            self.true_peak = (peak, frame);
        }
    }

    fn measure(&mut self, values: [f32; CHANNELS]) {
        while self
            .row_starts
            .get(self.row + 1)
            .is_some_and(|start| *start <= self.frame)
        {
            self.row += 1;
        }
        if self.block_frames == 0 {
            self.block_measured = true;
        }
        let (power, oversampled, momentary) = self.hear(values);
        let [left, right] = values.map(f64::from);
        if let Some(row) = self.rows.get_mut(self.row) {
            row.frames += 1;
            row.weighted += power;
            row.mid += (left + right) * (left + right) / 4.0;
            row.side += (left - right) * (left - right) / 4.0;
        }
        let sample_peak = values
            .iter()
            .fold(0.0_f32, |peak, value| peak.max(value.abs()));
        self.add_peak(self.frame, sample_peak);
        // The oversampled sound comes out late: before the start, it is the warm-up's.
        if let Some(frame) = self.frame.checked_sub(PEAK_DELAY) {
            self.add_peak(frame, oversampled);
        }
        if let Some(momentary) = momentary {
            self.add_momentary(momentary);
        }
        self.frame += 1;
        if let Some(heard) = self.spectrum.push(values) {
            self.add_spectrum(self.frame, heard);
        }
    }

    /// The momentary loudness of a block of 400 ms that ends in the current row.
    fn add_momentary(&mut self, momentary: f64) {
        if self.block_measured {
            self.measured_blocks += 1;
        }
        if self.measured_blocks >= BLOCKS_PER_WINDOW {
            self.windows.push(momentary);
            self.max_momentary = self.max_momentary.max(momentary);
        }
        // A row shorter than a block of 100 ms may end none: the block of 400 ms that ends
        // next covers it, so it is the loudest of that row.
        let waiting = self.rows.iter_mut().take(self.row).skip(self.waiting_row);
        for row in waiting {
            row.max_momentary.get_or_insert(momentary);
        }
        if let Some(row) = self.rows.get_mut(self.row) {
            let max = row
                .max_momentary
                .map_or(momentary, |max| max.max(momentary));
            row.max_momentary = Some(max);
        }
        self.waiting_row = self.row + 1;
        self.last_momentary = Some(momentary);
    }

    /// The spectrum and the pitch of the last [`WINDOW`] frames, when `pushed` frames have been
    /// pushed, go to the row its middle is in. The first ones have their middle before the start.
    fn add_spectrum(&mut self, pushed: u64, heard: Heard) {
        let Some(middle) = pushed.checked_sub(WINDOW as u64 / 2) else {
            return;
        };
        let row = self.row_starts.partition_point(|start| *start <= middle);
        let Some(row) = self.rows.get_mut(row.saturating_sub(1)) else {
            return;
        };
        for (sum, power) in row.bands.iter_mut().zip(heard.bands) {
            *sum += power;
        }
        row.spectra += 1;
        row.pitches.add(heard.pitch);
        self.pitches.add(heard.pitch);
    }

    pub fn finish(mut self) -> Measures {
        // The last spectra have their middle in the last half window: silence after the end
        // completes them. A spectrum whose middle is past the end belongs to no row.
        let frames = self.frame;
        for pushed in frames + 1..frames + WINDOW as u64 / 2 {
            if let Some(heard) = self.spectrum.push([0.0; CHANNELS]) {
                self.add_spectrum(pushed, heard);
            }
        }
        // The same for the oversampling: the peaks of the last frames are still in it.
        for late in frames..frames + PEAK_DELAY {
            let peak = self.oversampled_peak([0.0; CHANNELS]);
            if let Some(frame) = late.checked_sub(PEAK_DELAY) {
                self.add_peak(frame, peak);
            }
        }
        // Rows after the last whole block of 100 ms: the last block of 400 ms is the closest.
        let waiting = self.rows.iter_mut().skip(self.waiting_row);
        for row in waiting.filter(|row| row.frames > 0) {
            row.max_momentary = self.last_momentary;
        }
        let rows = self.rows.iter().map(RowSums::measures).collect();
        Measures {
            integrated: gated(&self.windows),
            max_momentary: lufs(self.max_momentary),
            true_peak: decibels(f64::from(self.true_peak.0).powi(2))
                .map(|peak| (peak, self.true_peak.1)),
            not_a_number: self.not_a_number,
            pitch: self.pitches.measure(),
            frames,
            rows,
        }
    }
}

impl RowSums {
    fn measures(&self) -> RowMeasures {
        let spectra = f64::from(self.spectra) * CHANNELS as f64;
        RowMeasures {
            loudness: (self.frames > 0)
                .then(|| lufs(self.weighted / self.frames as f64))
                .flatten(),
            max_momentary: self.max_momentary.and_then(lufs),
            true_peak: decibels(f64::from(self.true_peak).powi(2)),
            bands: self.bands.map(|power| {
                (self.spectra > 0)
                    .then(|| decibels(power / spectra))
                    .flatten()
            }),
            width: (self.mid + self.side > SILENT_POWER)
                .then(|| 100.0 * self.side / (self.mid + self.side)),
            pitch: self.pitches.measure(),
        }
    }
}

/// The loudness of the whole: the blocks above the absolute gate, and of those the ones above
/// the relative gate under their mean.
fn gated(windows: &[f64]) -> Option<f64> {
    let mean = |powers: &mut dyn Iterator<Item = f64>| {
        let (sum, count) = powers.fold((0.0, 0), |(sum, count), power| (sum + power, count + 1));
        (count > 0).then(|| sum / f64::from(count))
    };
    let loud = |floor: f64| move |power: &&f64| lufs(**power).is_some_and(|value| value > floor);
    let above_silence = mean(&mut windows.iter().filter(loud(ABSOLUTE_GATE)).copied())?;
    let relative = lufs(above_silence)? - RELATIVE_GATE;
    lufs(mean(&mut windows.iter().filter(loud(relative)).copied())?)
}

/// The loudness of a weighted power summed over the channels, or `None` for silence.
fn lufs(power: f64) -> Option<f64> {
    let loudness = -0.691 + 10.0 * power.log10();
    (power > 0.0 && loudness > ABSOLUTE_GATE).then_some(loudness)
}

/// A power in dB, or `None` for silence.
fn decibels(power: f64) -> Option<f64> {
    (power > SILENT_POWER).then(|| 10.0 * power.log10())
}

/// The two filters of BS.1770 that weight a sound as hearing does: a shelf that lifts what is
/// above about 1.5 kHz by 4 dB, and a high pass at about 38 Hz. The factors are worked out for
/// any rate, as libebur128 does, and are the ones of the standard at 48 kHz.
#[derive(Clone, Copy)]
struct KWeighting {
    shelf: Biquad,
    high_pass: Biquad,
}

impl KWeighting {
    fn new(rate: f64) -> Self {
        let (frequency, gain_db, q) = (
            1_681.974_450_955_533,
            3.999_843_853_973_347,
            0.707_175_236_955_419_6,
        );
        let k = (std::f64::consts::PI * frequency / rate).tan();
        let high = 10_f64.powf(gain_db / 20.0);
        let band = high.powf(0.499_666_774_154_541_6);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad::new(
            [
                (high + band * k / q + k * k) / a0,
                2.0 * (k * k - high) / a0,
                (high - band * k / q + k * k) / a0,
            ],
            [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        );
        let (frequency, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
        let k = (std::f64::consts::PI * frequency / rate).tan();
        let a0 = 1.0 + k / q + k * k;
        let high_pass = Biquad::new(
            [1.0, -2.0, 1.0],
            [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        );
        Self { shelf, high_pass }
    }

    fn process(&mut self, sample: f64) -> f64 {
        self.high_pass.process(self.shelf.process(sample))
    }
}

/// A second order filter, direct form one, in f64 so that the low high pass stays exact.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    inputs: [f64; 2],
    outputs: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self {
            b,
            a,
            inputs: [0.0; 2],
            outputs: [0.0; 2],
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let output = self.b[0] * input + self.b[1] * self.inputs[0] + self.b[2] * self.inputs[1]
            - self.a[0] * self.outputs[0]
            - self.a[1] * self.outputs[1];
        self.inputs = [input, self.inputs[0]];
        self.outputs = [output, self.outputs[0]];
        output
    }
}

/// What the last [`WINDOW`] frames hold: the power of each band, and the pitch of their
/// middle.
struct Heard {
    bands: [f64; BANDS.len()],
    pitch: Pitch,
}

/// The power of each band over the last [`WINDOW`] frames and their pitch, every [`HOP`] frames.
struct Spectrum {
    fft: Arc<dyn RealToComplex<f32>>,
    /// Hann, with the sum of its squares: a windowed spectrum is scaled back by it.
    window: Vec<f32>,
    window_power: f64,
    /// The last [`WINDOW`] frames of each channel, oldest first.
    history: [Vec<f32>; CHANNELS],
    /// Frames since the last spectrum.
    since: usize,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    /// The band of each bin and how much it counts: the bins between 0 and the top stand for
    /// their mirror image too. The bin at 0 is no band.
    bins: Vec<Option<(usize, f64)>>,
    pitch: PitchFinder,
}

impl Spectrum {
    fn new(rate: f64) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let window: Vec<f32> = (0..WINDOW)
            .map(|index| {
                let phase = std::f64::consts::TAU * index as f64 / WINDOW as f64;
                (0.5 - 0.5 * phase.cos()) as f32
            })
            .collect();
        let window_power = window.iter().map(|weight| f64::from(*weight).powi(2)).sum();
        let bins = (0..=WINDOW / 2)
            .map(|bin| {
                let frequency = bin as f64 * rate / WINDOW as f64;
                let band = BANDS.iter().rposition(|(_, lowest)| frequency >= *lowest)?;
                let mirrored = if bin == WINDOW / 2 { 1.0 } else { 2.0 };
                (bin > 0).then_some((band, mirrored))
            })
            .collect();
        Self {
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            fft,
            window,
            window_power,
            history: [vec![0.0; WINDOW], vec![0.0; WINDOW]],
            since: 0,
            bins,
            pitch: PitchFinder::new(rate, WINDOW),
        }
    }

    /// Adds one frame. Every [`HOP`] frames, the power of each band over the last [`WINDOW`]
    /// frames, summed over the channels, each as the mean square of the sound it stands for,
    /// and the pitch of their middle.
    fn push(&mut self, values: [f32; CHANNELS]) -> Option<Heard> {
        for (history, value) in self.history.iter_mut().zip(values) {
            if let Some(slot) = history.get_mut(WINDOW - HOP + self.since) {
                *slot = value;
            }
        }
        self.since += 1;
        if self.since < HOP {
            return None;
        }
        self.since = 0;
        let pitch = self.pitch();
        Some(Heard {
            bands: self.bands(),
            pitch,
        })
    }

    /// The pitch of the middle of the window, as long as the pitch finder looks at: shorter
    /// than the window, so that a vibrato is not smoothed away.
    fn pitch(&mut self) -> Pitch {
        let length = self.pitch.length();
        let middle = (WINDOW - length) / 2..(WINDOW + length) / 2;
        let [left, right] = &self.history;
        match (left.get(middle.clone()), right.get(middle)) {
            (Some(left), Some(right)) => self.pitch.find(left, right),
            _ => Pitch::Unclear,
        }
    }

    /// The bands of the last window. Then the window moves on by [`HOP`].
    fn bands(&mut self) -> [f64; BANDS.len()] {
        let mut bands = [0.0; BANDS.len()];
        for history in &self.history {
            for ((input, sample), weight) in self.input.iter_mut().zip(history).zip(&self.window) {
                *input = sample * weight;
            }
            // Only fails on buffers of the wrong length, which these are not.
            if self.fft.process(&mut self.input, &mut self.output).is_err() {
                continue;
            }
            let scale = WINDOW as f64 * self.window_power;
            for (bin, value) in self.bins.iter().zip(&self.output) {
                if let Some((band, mirrored)) = bin {
                    bands[*band] += mirrored * f64::from(value.norm_sqr()) / scale;
                }
            }
        }
        for history in &mut self.history {
            history.copy_within(HOP.., 0);
        }
        bands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// `seconds` of a sine on both channels, `right` times the left one on the right.
    fn sine(hz: f64, amplitude: f64, phase: f64, seconds: f64, right: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(RATE)) as usize;
        let mut samples = Vec::with_capacity(2 * frames);
        for frame in 0..frames {
            let time = frame as f64 / f64::from(RATE);
            let value = amplitude * (std::f64::consts::TAU * hz * time + phase).sin();
            samples.extend([value as f32, (right * value) as f32]);
        }
        samples
    }

    fn measure(samples: &[f32], rows: Vec<u64>) -> Measures {
        let mut meter = Meter::new(RATE, rows);
        // In uneven pieces, as a render gives them.
        for piece in samples.chunks(2 * 333) {
            meter.push(piece);
        }
        meter.finish()
    }

    fn close(value: Option<f64>, expected: f64, within: f64) {
        let value = value.unwrap();
        assert!(
            (value - expected).abs() <= within,
            "{value} is not {expected}"
        );
    }

    /// EBU Tech 3341, test 1: a 1 kHz sine at -23 dBFS on both channels reads -23 LUFS.
    #[test]
    fn a_sine_at_1_khz_reads_the_loudness_of_the_standard() {
        let amplitude = 10_f64.powf(-23.0 / 20.0);
        let measures = measure(&sine(1_000.0, amplitude, 0.0, 20.0, 1.0), vec![]);
        close(measures.integrated, -23.0, 0.1);
        close(measures.max_momentary, -23.0, 0.1);
        close(measures.rows[0].loudness, -23.0, 0.1);
    }

    /// EBU Tech 3341, test 3 in short: quiet stretches under the relative gate do not pull the
    /// loudness of the whole down, and silence does not count at all.
    #[test]
    fn the_gates_leave_out_silence_and_the_quiet_parts() {
        let loud = 10_f64.powf(-23.0 / 20.0);
        let quiet = 10_f64.powf(-40.0 / 20.0);
        let mut samples = sine(1_000.0, quiet, 0.0, 10.0, 1.0);
        samples.extend(vec![0.0; 2 * 48_000 * 10]);
        samples.extend(sine(1_000.0, loud, 0.0, 10.0, 1.0));
        let measures = measure(&samples, vec![]);
        close(measures.integrated, -23.0, 0.1);
        // The whole row, not gated, has the silence and the quiet part in it.
        assert!(measures.rows[0].loudness.unwrap() < -26.0);
    }

    /// A sine at a quarter of the rate, 45° off: every sample is at 0.707 of the top of the
    /// wave, which only the true peak sees.
    #[test]
    fn the_true_peak_finds_the_top_between_the_samples() {
        let samples = sine(12_000.0, 0.5, std::f64::consts::FRAC_PI_4, 1.0, 1.0);
        let sample_peak = samples.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
        assert!(20.0 * sample_peak.log10() < -8.9);
        let measures = measure(&samples, vec![]);
        close(measures.true_peak.map(|(peak, _)| peak), -6.02, 0.2);
    }

    /// 16 frames of the sine above: in the row they are in, also at the very end.
    #[test]
    fn a_short_burst_has_its_true_peak_in_its_own_row() {
        let burst = &sine(12_000.0, 0.5, std::f64::consts::FRAC_PI_4, 1.0, 1.0)[..2 * 16];
        let mut samples = vec![0.0; 2 * (48_000 - 24)];
        samples.extend(burst);
        samples.extend(vec![0.0; 2 * 48_008]);
        let measures = measure(&samples, vec![0, 48_000]);
        close(measures.rows[0].true_peak, -6.02, 0.3);
        assert!(measures.rows[1].true_peak.is_none_or(|peak| peak < -20.0));
        let at = measures.true_peak.unwrap().1;
        assert!((47_976..47_992).contains(&at), "{at}");

        let mut samples = vec![0.0; 2 * 48_000];
        samples.extend(burst);
        close(
            measure(&samples, vec![]).true_peak.map(|(peak, _)| peak),
            -6.02,
            0.3,
        );
    }

    #[test]
    fn a_tone_is_in_its_band_at_its_level_and_the_other_bands_are_far_under_it() {
        // RMS of a sine of amplitude 0.5: -9.03 dB.
        for (hz, band) in [
            (30.0, 0),
            (100.0, 1),
            (350.0, 2),
            (1_000.0, 3),
            (3_000.0, 4),
            (9_000.0, 5),
        ] {
            let measures = measure(&sine(hz, 0.5, 0.0, 2.0, 1.0), vec![]);
            let bands = measures.rows[0].bands;
            close(bands[band], -9.03, 0.2);
            for (other, level) in bands.iter().enumerate() {
                if other != band {
                    assert!(
                        level.is_none_or(|level| level < -35.0),
                        "{hz} Hz: {bands:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn width_is_zero_for_mono_half_for_one_channel_and_all_for_opposite_channels() {
        let width = |right| measure(&sine(440.0, 0.5, 0.0, 1.0, right), vec![]).rows[0].width;
        close(width(1.0), 0.0, 1e-6);
        close(width(0.0), 50.0, 1e-6);
        close(width(-1.0), 100.0, 1e-6);
    }

    #[test]
    fn each_row_measures_its_own_part() {
        let mut samples = sine(1_000.0, 0.5, 0.0, 1.0, 1.0);
        samples.extend(vec![0.0; 2 * 48_000]);
        samples.extend(sine(100.0, 0.05, 0.0, 1.0, 1.0));
        let measures = measure(&samples, vec![0, 48_000, 96_000]);
        let [loud, silent, quiet] = &measures.rows[..] else {
            panic!("{:?}", measures.rows);
        };
        close(loud.true_peak, -6.02, 0.1);
        close(loud.bands[3], -9.03, 0.3);
        // What the filters still ring with after the tone stops, far under it.
        assert!(
            silent.loudness.is_none_or(|level| level < -50.0),
            "{silent:?}"
        );
        // The reverb of the spectrum: half a window of the tone before reaches into it.
        assert!(
            silent.bands[3].is_none_or(|level| level < -20.0),
            "{silent:?}"
        );
        // The loudest block that ends in the silent row still holds some of the tone.
        assert!(silent.max_momentary.is_some());
        close(quiet.bands[1], -29.03, 0.3);
        assert_eq!(measures.frames, 3 * 48_000);
    }

    /// A range in the middle of a sound measures what sounds there, not a start from silence.
    #[test]
    fn what_is_heard_before_the_start_warms_the_meter_and_is_not_measured() {
        let amplitude = 10_f64.powf(-23.0 / 20.0);
        let tone = sine(1_000.0, amplitude, 0.0, 2.0, 1.0);
        let (before, after) = tone.split_at(2 * 48_000);
        let mut meter = Meter::new(RATE, vec![0, 4_800]);
        meter.warm_up(before);
        meter.push(after);
        let measures = meter.finish();
        assert_eq!(measures.frames, 48_000);
        // The first 100 ms already have a whole block of 400 ms of the tone behind them.
        close(measures.rows[0].max_momentary, -23.0, 0.1);
        close(measures.integrated, -23.0, 0.1);
        // Rows shorter than a block of 100 ms get the block that covers them.
        let mut meter = Meter::new(RATE, vec![0, 1_000, 2_000, 47_990]);
        meter.warm_up(before);
        meter.push(after);
        for row in meter.finish().rows {
            close(row.max_momentary, -23.0, 0.1);
        }

        // Silence after the tone: the whole is silent, whatever the warm-up heard, but for
        // what the filters ring with in the first milliseconds.
        let mut meter = Meter::new(RATE, vec![]);
        meter.warm_up(&tone);
        meter.push(&vec![0.0; 2 * 48_000]);
        let measures = meter.finish();
        for loudness in [measures.integrated, measures.max_momentary] {
            assert!(
                loudness.is_none_or(|loudness| loudness < -60.0),
                "{loudness:?}"
            );
        }
        // The same when the warm-up ends in the middle of a block of 100 ms.
        let mut meter = Meter::new(RATE, vec![]);
        meter.warm_up(&tone[..2 * 2_400]);
        meter.push(&vec![0.0; 2 * 48_000]);
        let measures = meter.finish();
        for loudness in [measures.integrated, measures.max_momentary] {
            assert!(
                loudness.is_none_or(|loudness| loudness < -60.0),
                "{loudness:?}"
            );
        }
    }

    /// `seconds` of a sine on both channels whose frequency at each time is `hz` of it.
    fn gliding(hz: impl Fn(f64) -> f64, seconds: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(RATE)) as usize;
        let mut phase = 0.0_f64;
        let mut samples = Vec::with_capacity(2 * frames);
        for frame in 0..frames {
            let value = (0.5 * phase.sin()) as f32;
            samples.extend([value, value]);
            phase += std::f64::consts::TAU * hz(frame as f64 / f64::from(RATE)) / f64::from(RATE);
        }
        samples
    }

    #[test]
    fn a_steady_tone_reads_its_note_and_no_drift() {
        // E1, the lowest string of a bass, A4, and 30 cents above C6.
        for note in [28.0, 69.0, 84.3] {
            let hz = 440.0 * 2_f64.powf((note - 69.0) / 12.0);
            let measures = measure(&sine(hz, 0.5, 0.0, 1.0, 1.0), vec![0, 24_000]);
            for row in &measures.rows {
                let pitch = row.pitch.unwrap();
                assert!((pitch.note - note).abs() < 0.01, "{hz} Hz: {pitch:?}");
                assert!(pitch.drift < 1.0, "{hz} Hz: {pitch:?}");
            }
        }
    }

    #[test]
    fn a_vibrato_reads_as_the_drift_it_has() {
        // 20 cents up and down five times a second: 40 cents from low to high.
        let vibrato =
            |time: f64| 440.0 * 2_f64.powf(0.2 / 12.0 * (std::f64::consts::TAU * 5.0 * time).sin());
        let measures = measure(&gliding(vibrato, 2.0), vec![]);
        let pitch = measures.rows[0].pitch.unwrap();
        assert!((pitch.note - 69.0).abs() < 0.03, "{pitch:?}");
        assert!((34.0..=42.0).contains(&pitch.drift), "{pitch:?}");
        // A slow wobble of 5 cents, as of tape, is still told from a steady tone.
        let wobble = |time: f64| {
            440.0 * 2_f64.powf(0.05 / 12.0 * (std::f64::consts::TAU * 0.7 * time).sin())
        };
        let drift = measure(&gliding(wobble, 2.0), vec![]).rows[0]
            .pitch
            .unwrap()
            .drift;
        assert!((8.0..=10.5).contains(&drift), "{drift}");
    }

    #[test]
    fn silence_noise_and_a_chord_have_no_pitch() {
        let silence = measure(&vec![0.0; 2 * 48_000], vec![]);
        assert_eq!(silence.rows[0].pitch, None);
        // White noise from a fixed seed, by xorshift.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let noise: Vec<f32> = (0..2 * 48_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state as f64 / u64::MAX as f64 - 0.5) as f32
            })
            .collect();
        assert_eq!(measure(&noise, vec![]).rows[0].pitch, None);
        // C, E and G repeat together at the period of C2, which none of them is.
        let mut chord = vec![0.0_f32; 2 * 48_000];
        for hz in [261.63, 329.63, 392.0] {
            for (sample, note) in chord.iter_mut().zip(sine(hz, 0.2, 0.0, 1.0, 1.0)) {
                *sample += note;
            }
        }
        assert_eq!(measure(&chord, vec![]).rows[0].pitch, None);
    }

    #[test]
    fn a_sample_that_is_not_a_number_is_found_and_measured_as_silence() {
        let mut samples = vec![0.0; 2 * 1000];
        samples[2 * 700 + 1] = f32::NAN;
        let measures = measure(&samples, vec![]);
        assert_eq!(measures.not_a_number, Some(700));
        assert_eq!(measures.true_peak, None);
        assert_eq!(measures.integrated, None);
    }
}
