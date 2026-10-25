//! What a sound is at each moment, a hundred times a second: its level, its pitch and how much
//! new sound comes in. [`super::notes`] finds the notes in them.

use super::pitch::{Pitch, PitchSettings, PitchTracker};
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
    /// Hears both channels mixed, and what it read after each frame of the history, newest at
    /// `read`, with how far back each reading describes.
    pitch: PitchTracker,
    readings: Vec<(Pitch, f64)>,
    read: usize,
    spectrum: PowerSpectrum,
    onset_length: usize,
    /// The power of each bin, both channels together, so channels of opposite polarity do not
    /// cancel.
    powers: Vec<f64>,
    /// The compressed spectrum of this moment and of the two before it, oldest first.
    weighed: Vec<f64>,
    earlier: [Vec<f64>; 2],
}

impl MomentFinder {
    pub(super) fn new(rate: f64) -> Self {
        let pitch = PitchTracker::new(rate, PitchSettings::FULL);
        let onset_length = (rate * ONSET_SECONDS) as usize;
        let length = pitch.length().max(onset_length);
        let bins = onset_length / 2 + 1;
        Self {
            hop: (rate / f64::from(PER_SECOND)).round() as usize,
            history: [vec![0.0; length], vec![0.0; length]],
            since: 0,
            measured: 0,
            pitch,
            readings: vec![(Pitch::Quiet, 0.0); length],
            read: 0,
            spectrum: PowerSpectrum::new(onset_length),
            onset_length,
            powers: vec![0.0; bins],
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
        self.pitch
            .push([values.iter().sum::<f32>() / CHANNELS as f32]);
        self.read = (self.read + 1) % length;
        if let Some(slot) = self.readings.get_mut(self.read) {
            *slot = (self.pitch.pitch(), self.pitch.age());
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

    /// The reading that describes the middle of the history: the one made as many frames after
    /// it as that reading is old.
    fn middle_pitch(&self) -> Pitch {
        let length = self.readings.len();
        let (_, age) = self
            .readings
            .get(self.read)
            .copied()
            .unwrap_or((Pitch::Quiet, 0.0));
        let back = (length / 2).saturating_sub(age.round() as usize);
        let at = (self.read + length - back) % length;
        self.readings
            .get(at)
            .map_or(Pitch::Quiet, |(pitch, _)| *pitch)
    }

    fn moment(&mut self) -> Moment {
        let length = self.history[0].len();
        let middle = |part: usize| (length - part) / 2..(length + part) / 2;
        let [left, right] = &self.history;
        let power: f64 = [left, right]
            .iter()
            .filter_map(|history| history.get(middle(self.hop)))
            .flatten()
            .map(|value| f64::from(*value).powi(2))
            .sum();
        let level = decibels(power / (CHANNELS * self.hop) as f64);
        self.powers.fill(0.0);
        for history in [left, right] {
            let samples = history.get(middle(self.onset_length)).unwrap_or_default();
            for (sum, power) in self.powers.iter_mut().zip(self.spectrum.powers(samples)) {
                *sum += power / CHANNELS as f64;
            }
        }
        for (weighed, power) in self.weighed.iter_mut().zip(&self.powers) {
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
            pitch: self.middle_pitch(),
            rise,
        }
    }
}
