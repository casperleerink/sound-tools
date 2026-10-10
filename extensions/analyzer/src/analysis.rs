//! What the card shows, worked out on the interface thread from the frames the analyzer wrote
//! into its scope: the spectrum, the momentary loudness and the note. The measuring is that of
//! `--analyze` ([`sound_media::analysis`]), so the card and an agent's report agree. The peaks
//! of the meter come from the processor's `Peaks`, which miss no sample.

use std::ops::Range;

use sound_media::analysis::{
    Momentary, Pitch, PitchFinder, PowerSpectrum, WINDOW, lufs, nearest_note,
};
use sound_ui::components::curves::RESPONSE_ACROSS;

/// Columns of the spectrum across the display, from 20 Hz to 20 kHz as a filter's response.
pub(crate) const COLUMNS: usize = 96;
/// The bottom of the spectrum, in dB. Under it is silence.
pub(crate) const FLOOR_DB: f32 = -90.0;
/// A column falls this fast once the sound in it is gone, so the spectrum moves calmly.
const FALL_DB_PER_SECOND: f32 = 30.0;
/// How long a note stays once the sound has none, so it does not blink between notes.
const NOTE_HOLD_SECONDS: f32 = 0.3;
/// Each column is the power of a band a sixth of an octave wide around its frequency, so the
/// harmonics of a high note read as one calm line and not as a comb.
const BAND_OCTAVES: f64 = 1.0 / 6.0;
/// A sine of amplitude `a` has a mean square of `a * a / 2`. Times this, it reads its peak level
/// in dBFS.
const SINE: f64 = 2.0;
/// Longer than any gap between two device buffers, see [`Analysis::hear_silence`].
const SILENT_AFTER_SECONDS: f32 = 0.1;
/// Silence to hear, a piece at a time.
const SILENCE: [[f32; 2]; 1024] = [[0.0; 2]; 1024];

/// What the card shows, besides the peaks.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reading {
    /// The level of each column of the spectrum in dB, from [`FLOOR_DB`] up.
    pub(crate) spectrum: [f32; COLUMNS],
    /// The loudness of the last 400 ms, in LUFS. `None` for silence.
    pub(crate) loudness: Option<f32>,
    /// The note the sound plays, a MIDI note number with its fraction: 69 is A4 at 440 Hz.
    pub(crate) note: Option<f64>,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            spectrum: [FLOOR_DB; COLUMNS],
            loudness: None,
            note: None,
        }
    }
}

/// The name of the nearest note and how far off it a note is, in cents: `("A4", 3)`.
pub(crate) fn tuning(note: f64) -> (String, i64) {
    let (nearest, cents) = nearest_note(note);
    (sound_notes::Pitch::nearest(nearest).name(), cents)
}

pub(crate) struct Analysis {
    rate: f32,
    /// The last [`WINDOW`] frames of each channel, oldest first.
    history: [Vec<f32>; 2],
    /// Whether frames came since the last look: else the history is as it was.
    heard: bool,
    /// Seconds since frames last came, and the frames of silence heard since.
    waiting: f32,
    silence: usize,
    /// The power of each bin of the history, and the bins of the band of each column.
    bins: Vec<f64>,
    columns: Vec<Range<usize>>,
    power: PowerSpectrum,
    pitch: PitchFinder,
    /// What the pitch finder found in the history.
    found: Pitch,
    momentary: Momentary,
    reading: Reading,
    /// Seconds since a look found a clear note.
    unclear: f32,
}

impl Analysis {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate);
        let bin_hz = rate / WINDOW as f64;
        let bins = WINDOW / 2 + 1;
        let edge = 2_f64.powf(BAND_OCTAVES / 2.0);
        let columns = (0..COLUMNS)
            .map(|column| {
                let middle = RESPONSE_ACROSS.value((column as f32 + 0.5) / COLUMNS as f32);
                let middle = f64::from(middle) / bin_hz;
                let start = ((middle / edge).round() as usize).min(bins - 1);
                let end = ((middle * edge).round() as usize + 1).clamp(start + 1, bins);
                start..end
            })
            .collect();
        Self {
            rate: rate as f32,
            history: [vec![0.0; WINDOW], vec![0.0; WINDOW]],
            heard: false,
            waiting: 0.0,
            silence: 0,
            bins: vec![0.0; bins],
            columns,
            power: PowerSpectrum::new(),
            pitch: PitchFinder::new(rate, WINDOW),
            found: Pitch::Quiet,
            momentary: Momentary::new(sample_rate),
            reading: Reading::default(),
            unclear: f32::INFINITY,
        }
    }

    pub(crate) fn reading(&self) -> &Reading {
        &self.reading
    }

    /// Hears the frames that came since the last call.
    pub(crate) fn hear(&mut self, frames: &[[f32; 2]]) {
        if !frames.is_empty() {
            (self.waiting, self.silence) = (0.0, 0);
            self.take(frames);
        }
    }

    fn take(&mut self, frames: &[[f32; 2]]) {
        for frame in frames {
            if let (_, Some(power)) = self.momentary.push(*frame) {
                self.reading.loudness = lufs(power).map(|loudness| loudness as f32);
            }
        }
        let new = frames.len().min(WINDOW);
        let frames = frames.iter().skip(frames.len() - new);
        for (channel, history) in self.history.iter_mut().enumerate() {
            history.copy_within(new.., 0);
            let samples = frames.clone().filter_map(|frame| frame.get(channel));
            for (slot, sample) in history.iter_mut().skip(WINDOW - new).zip(samples) {
                *slot = *sample;
            }
        }
        self.heard = true;
    }

    /// The analyzer writes no silence, so frames that stop coming are silence: once none came
    /// for [`SILENT_AFTER_SECONDS`], the card hears silence for the time that passes, until a
    /// second of it has emptied the history and the loudness.
    fn hear_silence(&mut self, seconds: f32) {
        self.waiting += seconds;
        if self.waiting < SILENT_AFTER_SECONDS || self.silence >= self.rate as usize {
            return;
        }
        let mut frames = (seconds * self.rate) as usize;
        self.silence += frames;
        while frames > 0 {
            let silence = SILENCE.get(..frames.min(SILENCE.len())).unwrap_or_default();
            frames -= silence.len();
            self.take(silence);
        }
    }

    /// The spectrum and the note of the last frames heard, `seconds` after the last look.
    /// Whether that changed the reading.
    pub(crate) fn look(&mut self, seconds: f32) -> bool {
        let before = self.reading.clone();
        if !self.heard {
            self.hear_silence(seconds);
        }
        // A history that did not change has the bins and the pitch it had.
        if std::mem::take(&mut self.heard) {
            self.bins.fill(0.0);
            // The power of both channels, so sounds that cancel in a mix still show.
            for history in &self.history {
                for (bin, power) in self.bins.iter_mut().zip(self.power.powers(history)) {
                    *bin += power / 2.0;
                }
            }
            let [left, right] = self.history.each_ref().map(|history| {
                let last = history.get(WINDOW - self.pitch.length()..);
                last.unwrap_or_default()
            });
            self.found = self.pitch.find(left, right);
        }
        let fall = FALL_DB_PER_SECOND * seconds;
        for (shown, bins) in self.reading.spectrum.iter_mut().zip(&self.columns) {
            let power: f64 = self.bins.get(bins.clone()).unwrap_or_default().iter().sum();
            let db = (10.0 * (SINE * power).log10()) as f32;
            *shown = db.max(*shown - fall).max(FLOOR_DB);
        }
        match self.found {
            Pitch::Note(note) => {
                self.reading.note = Some(note);
                self.unclear = 0.0;
            }
            Pitch::Quiet | Pitch::Unclear => {
                self.unclear += seconds;
                if self.unclear > NOTE_HOLD_SECONDS {
                    self.reading.note = None;
                }
            }
        }
        self.reading != before
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

    fn loudest_column(analysis: &Analysis) -> (usize, f32) {
        let spectrum = analysis.reading.spectrum.iter().copied().enumerate();
        spectrum.max_by(|(_, a), (_, b)| a.total_cmp(b)).unwrap()
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
        let note = analyse(&sine(440.0, 0.5, 0.5)).reading.note.unwrap();
        assert_eq!(tuning(note), ("A4".to_string(), 0));
        // 30 cents above C3.
        let hz = 440.0 * 2_f64.powf((48.3 - 69.0) / 12.0);
        let note = analyse(&sine(hz, 0.5, 0.5)).reading.note.unwrap();
        assert_eq!(tuning(note), ("C3".to_string(), 30));
    }

    /// EBU Tech 3341: a 1 kHz sine at -23 dBFS in both channels is -23 LUFS.
    #[test]
    fn the_loudness_of_the_standard_sine_is_minus_23_lufs() {
        let amplitude = 10_f64.powf(-23.0 / 20.0);
        let loudness = analyse(&sine(1_000.0, amplitude, 2.0)).reading.loudness;
        assert!((loudness.unwrap() + 23.0).abs() < 0.1, "{loudness:?}");
    }

    #[test]
    fn a_sine_peaks_at_its_level_in_its_column_and_the_spectrum_falls_to_rest() {
        let mut analysis = analyse(&sine(1_000.0, 0.5, 0.5));
        let (loudest, db) = loudest_column(&analysis);
        // -6 dBFS.
        assert!((-6.3..=-5.9).contains(&db), "{db}");
        let column = (RESPONSE_ACROSS.position(1_000.0) * COLUMNS as f32) as usize;
        assert!(loudest.abs_diff(column) <= 1, "{loudest} is not {column}");
        assert!(analysis.reading.spectrum[column / 2] < -60.0);
        // The analyzer writes no silence: frames that stop are silence. The note and the
        // loudness go, the columns fall to the floor, and the card comes to rest.
        for _ in 0..240 {
            analysis.look(1.0 / 60.0);
        }
        assert_eq!(analysis.reading, Reading::default());
        assert!(!analysis.look(1.0 / 60.0));
    }

    /// The two channels upside down to each other cancel in a mix, and still show.
    #[test]
    fn a_sound_whose_channels_cancel_shows_at_its_level() {
        let mut opposite = sine(1_000.0, 0.5, 0.5);
        for frame in &mut opposite {
            frame[1] = -frame[0];
        }
        let (_, db) = loudest_column(&analyse(&opposite));
        assert!((-6.3..=-5.9).contains(&db), "{db}");
    }
}
