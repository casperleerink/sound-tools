//! What the card shows, worked out on the interface thread from the frames the analyzer wrote
//! into its scope: the spectrum, the peaks, the momentary loudness and the note. The measuring
//! is that of `--analyze` ([`sound_media::analysis`]), so the card and an agent's report agree.

use std::ops::Range;

use sound_media::analysis::{Momentary, Pitch, PitchFinder, PowerSpectrum, WINDOW, lufs};
use sound_ui::components::curves::RESPONSE_ACROSS;

/// Columns of the spectrum across the display, from 20 Hz to 20 kHz as a filter's response.
pub(crate) const COLUMNS: usize = 96;
/// The bottom of the spectrum, in dB. Under it is silence.
pub(crate) const FLOOR_DB: f32 = -90.0;
/// A column falls this fast once the sound in it is gone, so the spectrum moves calmly.
const FALL_DB_PER_SECOND: f32 = 30.0;
/// How long a note stays once the sound has none, so it does not blink between notes.
const NOTE_HOLD_SECONDS: f32 = 0.3;
/// Under a Hann window a sine of amplitude `a` puts `a * a / 3` in the bin it is on, its mirror
/// image included. Times this, a sine reads its peak level in dBFS.
const SINE_IN_A_BIN: f64 = 3.0;

/// What the card shows, besides the peaks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reading {
    /// The level of each column of the spectrum in dB, from [`FLOOR_DB`] up.
    pub(crate) spectrum: [f32; COLUMNS],
    /// The loudness of the last 400 ms, in LUFS. `None` for silence.
    pub(crate) loudness: Option<f32>,
    pub(crate) tuning: Option<Tuning>,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            spectrum: [FLOOR_DB; COLUMNS],
            loudness: None,
            tuning: None,
        }
    }
}

/// The pitch the sound plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tuning {
    /// A MIDI note number with its fraction: 69 is A4 at 440 Hz.
    pub(crate) note: f64,
}

impl Tuning {
    /// The nearest note: `A4`.
    pub(crate) fn name(self) -> String {
        // The pitch finder looks from 40 Hz to 4 kHz, well inside the notes MIDI has.
        sound_notes::Pitch::nearest(self.note.round() as i64).name()
    }

    /// How far off the nearest note, from -50 to 50.
    pub(crate) fn cents(self) -> i32 {
        ((self.note - self.note.round()) * 100.0).round() as i32
    }
}

pub(crate) struct Analysis {
    /// The last [`WINDOW`] frames of each channel, oldest first.
    history: [Vec<f32>; 2],
    /// Both channels of the history mixed, and the power of each bin of it.
    mid: Vec<f32>,
    bins: Vec<f64>,
    /// The bins of each column of the spectrum.
    columns: Vec<Range<usize>>,
    power: PowerSpectrum,
    pitch: PitchFinder,
    momentary: Momentary,
    /// The loudest sample of each channel since the peaks were last taken.
    peaks: [f32; 2],
    reading: Reading,
    /// Seconds since a look found a clear note.
    unclear: f32,
}

impl Analysis {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate);
        let bin_hz = rate / WINDOW as f64;
        let bins = WINDOW / 2 + 1;
        let edge = |column: usize| {
            let hz = RESPONSE_ACROSS.value(column as f32 / COLUMNS as f32);
            f64::from(hz) / bin_hz
        };
        let columns = (0..COLUMNS)
            .map(|column| {
                let start = (edge(column).floor() as usize).min(bins - 1);
                let end = (edge(column + 1).ceil() as usize).clamp(start + 1, bins);
                start..end
            })
            .collect();
        Self {
            history: [vec![0.0; WINDOW], vec![0.0; WINDOW]],
            mid: vec![0.0; WINDOW],
            bins: vec![0.0; bins],
            columns,
            power: PowerSpectrum::new(),
            pitch: PitchFinder::new(rate, WINDOW),
            momentary: Momentary::new(sample_rate),
            peaks: [0.0; 2],
            reading: Reading::default(),
            unclear: f32::INFINITY,
        }
    }

    /// Hears the frames that came since the last call.
    pub(crate) fn hear(&mut self, frames: &[[f32; 2]]) {
        for frame in frames {
            if let (_, Some(power)) = self.momentary.push(*frame) {
                self.reading.loudness = lufs(power).map(|loudness| loudness as f32);
            }
            for (peak, sample) in self.peaks.iter_mut().zip(frame) {
                *peak = peak.max(sample.abs());
            }
        }
        let new = frames.len().min(WINDOW);
        for (channel, history) in self.history.iter_mut().enumerate() {
            history.copy_within(new.., 0);
            let samples = frames.iter().skip(frames.len() - new);
            let samples = samples.filter_map(|frame| frame.get(channel));
            for (slot, sample) in history.iter_mut().skip(WINDOW - new).zip(samples) {
                *slot = *sample;
            }
        }
    }

    /// The loudest sample of each channel since the last take.
    pub(crate) fn take_peaks(&mut self) -> [f32; 2] {
        std::mem::take(&mut self.peaks)
    }

    /// The spectrum and the note of the last frames heard, `seconds` after the last look.
    pub(crate) fn look(&mut self, seconds: f32) -> &Reading {
        let [left, right] = &self.history;
        for (mid, (left, right)) in self.mid.iter_mut().zip(left.iter().zip(right)) {
            *mid = (left + right) / 2.0;
        }
        for (bin, power) in self.bins.iter_mut().zip(self.power.powers(&self.mid)) {
            *bin = power;
        }
        let fall = FALL_DB_PER_SECOND * seconds;
        for (shown, bins) in self.reading.spectrum.iter_mut().zip(&self.columns) {
            let bins = self.bins.get(bins.clone()).unwrap_or_default();
            let power = bins
                .iter()
                .fold(0.0_f64, |loudest, power| loudest.max(*power));
            let db = (10.0 * (SINE_IN_A_BIN * power).log10()) as f32;
            *shown = db.max(*shown - fall).max(FLOOR_DB);
        }
        let length = self.pitch.length();
        let [left, right] = [left, right].map(|history| {
            let last = history.get(WINDOW - length..);
            last.unwrap_or_default()
        });
        match self.pitch.find(left, right) {
            Pitch::Note(note) => {
                self.reading.tuning = Some(Tuning { note });
                self.unclear = 0.0;
            }
            Pitch::Quiet | Pitch::Unclear => {
                self.unclear += seconds;
                if self.unclear > NOTE_HOLD_SECONDS {
                    self.reading.tuning = None;
                }
            }
        }
        &self.reading
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// `seconds` of a sine of `hz` at `amplitude` in both channels.
    fn sine(hz: f64, amplitude: f64, seconds: f64) -> Vec<[f32; 2]> {
        let frames = (seconds * f64::from(RATE)) as usize;
        let value = |frame: usize| {
            let phase = std::f64::consts::TAU * hz * frame as f64 / f64::from(RATE);
            (amplitude * phase.sin()) as f32
        };
        (0..frames).map(|frame| [value(frame); 2]).collect()
    }

    /// Hears `frames` in pieces of one poll and looks after each, as the card does.
    fn analyse(frames: &[[f32; 2]]) -> Analysis {
        let mut analysis = Analysis::new(RATE);
        for piece in frames.chunks(800) {
            analysis.hear(piece);
            analysis.look(1.0 / 60.0);
        }
        analysis
    }

    #[test]
    fn the_tuner_reads_440_hz_as_a4_and_a_sharp_note_in_cents() {
        let tuning = analyse(&sine(440.0, 0.5, 0.5)).reading.tuning.unwrap();
        assert_eq!((tuning.name().as_str(), tuning.cents()), ("A4", 0));
        // 30 cents above C3.
        let hz = 440.0 * 2_f64.powf((48.3 - 69.0) / 12.0);
        let tuning = analyse(&sine(hz, 0.5, 0.5)).reading.tuning.unwrap();
        assert_eq!((tuning.name().as_str(), tuning.cents()), ("C3", 30));
    }

    #[test]
    fn the_meter_reads_the_peak_and_the_loudness_of_the_standard() {
        // EBU Tech 3341: a 1 kHz sine at -23 dBFS in both channels is -23 LUFS.
        let amplitude = 10_f64.powf(-23.0 / 20.0);
        let mut analysis = analyse(&sine(1_000.0, amplitude, 2.0));
        let loudness = analysis.reading.loudness.unwrap();
        assert!((loudness + 23.0).abs() < 0.1, "{loudness}");
        let [left, right] = analysis.take_peaks();
        assert!((left - amplitude as f32).abs() < 1e-4 && left == right);
        assert_eq!(analysis.take_peaks(), [0.0; 2]);
    }

    #[test]
    fn a_sine_peaks_at_its_level_in_its_column_and_the_spectrum_falls_to_rest() {
        let mut analysis = analyse(&sine(1_000.0, 0.5, 0.5));
        let spectrum = analysis.reading.spectrum;
        let (loudest, db) = (spectrum.iter().enumerate())
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap();
        // -6 dBFS, less what the window loses between two bins.
        assert!((-7.5..=-5.9).contains(db), "{db}");
        let column = (RESPONSE_ACROSS.position(1_000.0) * COLUMNS as f32) as usize;
        assert!(loudest.abs_diff(column) <= 1, "{loudest} is not {column}");
        assert!(spectrum[column / 2] < -60.0, "{spectrum:?}");
        // Silence: the note goes, the columns fall and come to rest at the floor.
        let silence = vec![[0.0; 2]; RATE as usize / 2];
        analysis.hear(&silence);
        for _ in 0..240 {
            analysis.look(1.0 / 60.0);
        }
        assert_eq!(analysis.reading, Reading::default());
    }
}
