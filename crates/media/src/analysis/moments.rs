//! What a sound is at each moment, a hundred times a second: its level, its pitch and how much
//! new sound comes in. [`super::notes`] finds the notes in them.

use super::pitch::{Pitch, PitchFinder};
use super::{CHANNELS, PowerSpectrum, decibels};

/// Moments per second.
const PER_SECOND: u32 = 100;
/// The spectrum new sound is found in, in seconds: short enough to tell two notes 50 ms apart.
const ONSET_SECONDS: f64 = 0.04;
/// Each bin counts as `ln(1 + COMPRESSION * magnitude)`: a quiet note starting counts nearly as
/// much as a loud one, and a bin 60 dB down, such as the hiss of a recording, hardly at all.
const COMPRESSION: f64 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    /// The frame in its middle, from the start.
    pub frame: u64,
    /// RMS in dB of the frames around it, both channels together. `None` for silence.
    pub level: Option<f64>,
    pub pitch: Pitch,
    /// How much the spectrum rose since two moments before: high where new sound starts.
    pub(super) rise: f64,
}

/// Finds a [`Moment`] in a stereo sound pushed through it frame by frame.
pub(super) struct MomentFinder {
    hop: usize,
    /// The frames the moment looks at, oldest first: as long as the longest of what it measures,
    /// which all have the same middle.
    history: [Vec<f32>; CHANNELS],
    since: usize,
    /// The frames pushed after the start.
    measured: u64,
    pitch: PitchFinder,
    spectrum: PowerSpectrum,
    onset_length: usize,
    mixed: Vec<f32>,
    /// The compressed spectrum of this moment and of the two before it, oldest first.
    weighed: Vec<f64>,
    earlier: [Vec<f64>; 2],
}

impl MomentFinder {
    pub(super) fn new(rate: f64) -> Self {
        let pitch = PitchFinder::new(rate, usize::MAX);
        let onset_length = (rate * ONSET_SECONDS) as usize;
        let length = pitch.length().max(onset_length);
        let bins = onset_length / 2 + 1;
        Self {
            hop: (rate / f64::from(PER_SECOND)).round() as usize,
            history: [vec![0.0; length], vec![0.0; length]],
            since: 0,
            measured: 0,
            pitch,
            spectrum: PowerSpectrum::new(onset_length),
            onset_length,
            mixed: vec![0.0; onset_length],
            weighed: vec![0.0; bins],
            earlier: [vec![0.0; bins], vec![0.0; bins]],
        }
    }

    /// How many frames after the end a moment can still have its middle before it.
    pub(super) fn reach(&self) -> usize {
        self.history[0].len() / 2 + self.hop
    }

    /// Adds one frame, `measured` after the start or else from before it. Every 10 ms, the
    /// moment whose middle is half the history back, if that is after the start.
    pub(super) fn push(&mut self, values: [f32; CHANNELS], measured: bool) -> Option<Moment> {
        let length = self.history[0].len();
        for (history, value) in self.history.iter_mut().zip(values) {
            if let Some(slot) = history.get_mut(length - self.hop + self.since) {
                *slot = value;
            }
        }
        self.since += 1;
        if measured {
            self.measured += 1;
        }
        if self.since < self.hop {
            return None;
        }
        self.since = 0;
        let moment = self.moment();
        for history in &mut self.history {
            history.copy_within(self.hop.., 0);
        }
        let frame = self.measured.checked_sub(length as u64 / 2)?;
        Some(Moment { frame, ..moment })
    }

    fn moment(&mut self) -> Moment {
        let length = self.history[0].len();
        let middle = |part: usize| (length - part) / 2..(length + part) / 2;
        let [left, right] = &self.history;
        let pitch = match (
            left.get(middle(self.pitch.length())),
            right.get(middle(self.pitch.length())),
        ) {
            (Some(left), Some(right)) => self.pitch.find(left, right),
            _ => Pitch::Unclear,
        };
        let power: f64 = [left, right]
            .iter()
            .filter_map(|history| history.get(middle(self.hop)))
            .flatten()
            .map(|value| f64::from(*value).powi(2))
            .sum();
        let level = decibels(power / (CHANNELS * self.hop) as f64);
        let onset = middle(self.onset_length);
        let both = left.get(onset.clone()).into_iter().flatten();
        let both = both.zip(right.get(onset).into_iter().flatten());
        for (mixed, (left, right)) in self.mixed.iter_mut().zip(both) {
            *mixed = (left + right) / 2.0;
        }
        for (weighed, power) in self
            .weighed
            .iter_mut()
            .zip(self.spectrum.powers(&self.mixed))
        {
            *weighed = (1.0 + COMPRESSION * power.sqrt()).ln();
        }
        let rise = self
            .weighed
            .iter()
            .zip(&self.earlier[0])
            .map(|(now, before)| (now - before).max(0.0))
            .sum();
        let [older, newer] = &mut self.earlier;
        std::mem::swap(older, newer);
        std::mem::swap(newer, &mut self.weighed);
        Moment {
            frame: 0,
            level,
            pitch,
            rise,
        }
    }
}
