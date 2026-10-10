//! The pitch of a short window of sound, by YIN (de Cheveigné and Kawahara, 2002): the period
//! is the shortest lag at which the sound clearly repeats itself. It finds the fundamental even
//! when its octave is up to four times as loud, and it says when nothing repeats, as in noise or
//! most chords.
//!
//! A chord can repeat too: C, E and G together repeat at the period of a C two octaves under
//! them, a note nobody plays. So a period counts only when the sound has power at its
//! frequency.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// The lowest fundamental looked for, in Hz: about E1, the lowest string of a bass.
const LOWEST: f64 = 40.0;
/// The highest, in Hz: about B7. Above it a tone reads as no pitch.
const HIGHEST: f64 = 4_000.0;
/// A lag whose normalized difference is under this repeats clearly enough to be a period. At
/// 0.15 a tone whose octave is four times as loud reads an octave high.
const THRESHOLD: f64 = 0.1;
/// Under this mean square, -60 dB, a window is too quiet to tell a pitch in.
const QUIET: f64 = 1e-6;
/// The least share of the power of a window at the frequency of its period. A sawtooth has 61%
/// there, a bright tone a few; a chord read as its root has almost nothing.
const FUNDAMENTAL: f64 = 0.02;

/// What one window sounds like.
#[derive(Clone, Copy)]
pub enum Pitch {
    Quiet,
    /// Sound with no clear period: noise, drums, a chord.
    Unclear,
    /// A MIDI note number with its fraction: 69 is A4 at 440 Hz, 69.5 is 50 cents above it.
    Note(f64),
}

/// The pitch of a stretch of windows: the middle one, and how far it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeasuredPitch {
    /// A MIDI note number with its fraction, as [`Pitch::Note`].
    pub note: f64,
    /// From low to high, in cents, leaving out the lowest and the highest twentieth, where a
    /// note starts or ends.
    pub drift: f64,
}

/// The nearest MIDI note to `note`, a note number with its fraction, and how far off it `note`
/// is in cents, from -50 to 50.
pub fn nearest_note(note: f64) -> (i64, i64) {
    let nearest = note.round();
    (nearest as i64, ((note - nearest) * 100.0).round() as i64)
}

/// The note of each window with a clear pitch, and how many windows were not quiet.
#[derive(Default, Clone)]
pub(super) struct Pitches {
    notes: Vec<f64>,
    sounding: usize,
}

impl Pitches {
    pub(super) fn add(&mut self, pitch: Pitch) {
        match pitch {
            Pitch::Quiet => {}
            Pitch::Unclear => self.sounding += 1,
            Pitch::Note(note) => {
                self.sounding += 1;
                self.notes.push(note);
            }
        }
    }

    /// `None` when most of the sound has no clear pitch, even if a moment of it did.
    pub(super) fn measure(&self) -> Option<MeasuredPitch> {
        if self.notes.is_empty() || 2 * self.notes.len() < self.sounding {
            return None;
        }
        let mut notes = self.notes.clone();
        notes.sort_by(f64::total_cmp);
        let at = |share: f64| {
            let index = ((notes.len() - 1) as f64 * share).round() as usize;
            notes.get(index).copied()
        };
        Some(MeasuredPitch {
            note: at(0.5)?,
            drift: 100.0 * (at(0.95)? - at(0.05)?),
        })
    }
}

pub struct PitchFinder {
    rate: f64,
    /// The lags looked at, in frames: the periods of [`HIGHEST`] to [`LOWEST`]. The difference
    /// at each lag is summed over `longest` frames, so a window is twice as long.
    shortest: usize,
    longest: usize,
    forward: Arc<dyn RealToComplex<f64>>,
    inverse: Arc<dyn ComplexToReal<f64>>,
    /// The window, both channels mixed.
    sound: Vec<f64>,
    input: Vec<f64>,
    head: Vec<Complex<f64>>,
    whole: Vec<Complex<f64>>,
    correlation: Vec<f64>,
    /// The sum of the squares of the first `n` frames of the window, at `n`.
    energy: Vec<f64>,
    /// The difference of the window with itself at each lag, and that difference divided by
    /// its mean over the shorter lags, which is how YIN tells a clear period from a weak one.
    difference: Vec<f64>,
    normalized: Vec<f64>,
}

impl PitchFinder {
    /// A finder for a sound at `rate` that looks at no more than `window` frames. When two
    /// periods of [`LOWEST`] do not fit, the lowest note found is higher: 47 Hz at 96 kHz in
    /// 4096 frames.
    pub fn new(rate: f64, window: usize) -> Self {
        let longest = ((rate / LOWEST).ceil() as usize).min(window / 2);
        let shortest = ((rate / HIGHEST).floor() as usize).max(2);
        let mut planner = RealFftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(2 * longest);
        let inverse = planner.plan_fft_inverse(2 * longest);
        Self {
            rate,
            shortest,
            longest,
            sound: vec![0.0; 2 * longest],
            input: forward.make_input_vec(),
            head: forward.make_output_vec(),
            whole: forward.make_output_vec(),
            correlation: inverse.make_output_vec(),
            energy: vec![0.0; 2 * longest + 1],
            difference: vec![0.0; longest + 1],
            normalized: vec![0.0; longest + 1],
            forward,
            inverse,
        }
    }

    /// How many frames a window is.
    pub fn length(&self) -> usize {
        self.sound.len()
    }

    /// The pitch of a window of [`Self::length`] frames of each channel.
    pub fn find(&mut self, left: &[f32], right: &[f32]) -> Pitch {
        for ((sound, left), right) in self.sound.iter_mut().zip(left).zip(right) {
            *sound = (f64::from(*left) + f64::from(*right)) / 2.0;
        }
        let length = self.sound.len() as f64;
        let mut sum = 0.0;
        for (energy, sound) in self.energy.iter_mut().skip(1).zip(&self.sound) {
            sum += sound * sound;
            *energy = sum;
        }
        let power = sum / length;
        if power < QUIET {
            return Pitch::Quiet;
        }
        if self.correlate().is_err() {
            return Pitch::Unclear;
        }
        // The squared difference of the first half with the window `lag` frames on, summed:
        // the energy of both minus twice their correlation.
        let half = self.longest;
        let head = self.energy.get(half).copied().unwrap_or_default();
        let shifted = self.energy.iter().zip(self.energy.iter().skip(half));
        for ((difference, correlation), (start, end)) in self
            .difference
            .iter_mut()
            .zip(&self.correlation)
            .zip(shifted)
        {
            // The inverse transform is not scaled down by its length.
            *difference = (head + end - start - 2.0 * correlation / length).max(0.0);
        }
        let mut sum = 0.0;
        for (lag, (normalized, difference)) in
            self.normalized.iter_mut().zip(&self.difference).enumerate()
        {
            sum += difference;
            *normalized = match lag {
                0 => 1.0,
                _ if sum <= 0.0 => 1.0,
                _ => difference * lag as f64 / sum,
            };
        }
        let Some(period) = self.period() else {
            return Pitch::Unclear;
        };
        let hz = self.rate / period;
        match self.power_at(hz) >= FUNDAMENTAL * power {
            true => Pitch::Note(69.0 + 12.0 * (hz / 440.0).log2()),
            false => Pitch::Unclear,
        }
    }

    /// The mean square of the part of the window that is a sine at `hz`.
    fn power_at(&self, hz: f64) -> f64 {
        let step = std::f64::consts::TAU * hz / self.rate;
        let (mut cosine, mut sine) = (0.0, 0.0);
        for (index, sound) in self.sound.iter().enumerate() {
            let phase = step * index as f64;
            cosine += sound * phase.cos();
            sine += sound * phase.sin();
        }
        // A sine of amplitude `a` sums to `a * length / 2` here, and has a mean square of
        // `a * a / 2`.
        let length = self.sound.len() as f64;
        2.0 * (cosine * cosine + sine * sine) / (length * length)
    }

    /// The correlation of the first half of the window with the whole, at every lag, through
    /// the spectra of both: the sum of `sound[j] * sound[j + lag]` over the first half.
    fn correlate(&mut self) -> Result<(), realfft::FftError> {
        self.input.copy_from_slice(&self.sound);
        self.forward.process(&mut self.input, &mut self.whole)?;
        let head = self.sound.iter().take(self.longest);
        for (input, sound) in self
            .input
            .iter_mut()
            .zip(head.chain(std::iter::repeat(&0.0)))
        {
            *input = *sound;
        }
        self.forward.process(&mut self.input, &mut self.head)?;
        for (whole, head) in self.whole.iter_mut().zip(&self.head) {
            *whole *= head.conj();
        }
        self.inverse.process(&mut self.whole, &mut self.correlation)
    }

    /// The first lag in range whose normalized difference is under [`THRESHOLD`], moved on to
    /// the bottom of its dip and between the frames by a parabola through the difference.
    fn period(&self) -> Option<f64> {
        let normalized = |lag: usize| self.normalized.get(lag).copied();
        let mut lag = (self.shortest..=self.longest)
            .find(|lag| normalized(*lag).is_some_and(|value| value < THRESHOLD))?;
        while let (Some(next), Some(here)) = (normalized(lag + 1), normalized(lag))
            && next < here
        {
            lag += 1;
        }
        // A dip still falling at the longest lag has its bottom past it: a note under the
        // lowest, which would read as the lowest.
        let difference = |lag: usize| self.difference.get(lag).copied();
        let (Some(before), Some(here), Some(after)) =
            (difference(lag - 1), difference(lag), difference(lag + 1))
        else {
            return None;
        };
        let curve = before - 2.0 * here + after;
        let offset = match curve > 0.0 {
            true => ((before - after) / (2.0 * curve)).clamp(-1.0, 1.0),
            false => 0.0,
        };
        Some(lag as f64 + offset)
    }
}
