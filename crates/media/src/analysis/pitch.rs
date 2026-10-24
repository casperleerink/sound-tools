//! The pitch of a sound as it goes on. Every few milliseconds the tracker looks at the last
//! two periods of the lowest note, as YIN does (de Cheveigné and Kawahara, 2002): the period is
//! the shortest lag at which the sound clearly repeats itself. That finds the fundamental even
//! when its octave is up to four times as loud, and it says when nothing repeats, as in noise
//! or most chords.
//!
//! A look is not judged on its own. Once a note is found, the next looks follow its period
//! near where it was, and keep it while it repeats less clearly than it had to be found. So a
//! loud octave or a breath inside a note does not make the reading jump. A new note does: its
//! sound stops repeating at the old period, or, an octave up, still repeats there but has no
//! power at its frequency.
//!
//! A chord can repeat too: C, E and G together repeat at the period of a C two octaves under
//! them, a note nobody plays. So a period counts only when the sound has power at its
//! frequency or the octave above: a voice through a small microphone has little at its own. The
//! octave counts only while the sound does not also repeat clearly at half the period, or a
//! note followed would never let go of a new note an octave up.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// Under this mean square, -60 dB, the sound is too quiet to tell a pitch in.
const QUIET: f64 = 1e-6;
/// The least share of the power of the sound at the frequency of its period, or at twice it. A
/// sawtooth has 61% there, a bright tone a few; a chord read as its root has almost nothing.
const FUNDAMENTAL: f64 = 0.02;
/// How often the tracker looks, in seconds.
const LOOK_SECONDS: f64 = 0.005;
/// How far a followed period may move from one look to the next, in semitones. A faster move
/// is found anew.
const FOLLOW_SEMITONES: f64 = 1.0;
/// How long a note whose period stopped repeating clearly is still followed, in seconds, as
/// through a consonant. It reads as no clear pitch meanwhile.
const HOLD_SECONDS: f64 = 0.05;

/// What the sound is at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
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

/// What the tracker looks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchSettings {
    /// The lowest and the highest fundamental looked for, in Hz.
    pub lowest: f64,
    pub highest: f64,
    /// How clearly a sound must repeat to read as a note, from 0, strict, to 1, tolerant. A
    /// tolerant tracker holds a breathy note but also hears notes in more noise, and above
    /// 0.5 a tone whose octave is four times as loud can be found an octave high.
    pub tolerance: f64,
}

impl PitchSettings {
    /// Every note an instrument plays: about E1, the lowest string of a bass, to about B7.
    pub const FULL: Self = Self {
        lowest: 40.0,
        highest: 4_000.0,
        tolerance: 0.5,
    };

    /// A lag whose normalized difference is under this repeats clearly enough to find a note.
    fn finding(&self) -> f64 {
        0.05 + 0.1 * self.tolerance.clamp(0.0, 1.0)
    }
}

/// Follows the pitch of one channel of sound. Everything it needs is made in [`Self::new`], so
/// [`Self::push`] may run on the audio thread.
pub struct PitchTracker {
    rate: f64,
    /// The lowest and the highest fundamental read, in Hz.
    lowest: f64,
    highest: f64,
    /// The lags looked at, in frames: the periods of the highest, with one more for the
    /// parabola, to the lowest note. The difference at each lag is summed over `longest`
    /// frames, so a look is twice as long.
    shortest: usize,
    longest: usize,
    finding: f64,
    /// A followed period is kept while its normalized difference is under this.
    keeping: f64,
    /// Frames from one look to the next, and looks a lost note is held for.
    look_frames: usize,
    hold_looks: usize,
    forward: Arc<dyn RealToComplex<f64>>,
    inverse: Arc<dyn ComplexToReal<f64>>,
    scratch: Vec<Complex<f64>>,
    /// The last `2 * longest` frames, and where the next one goes, which is the oldest.
    ring: Vec<f32>,
    next: usize,
    since_look: usize,
    /// The ring in order, oldest first.
    sound: Vec<f64>,
    input: Vec<f64>,
    head: Vec<Complex<f64>>,
    whole: Vec<Complex<f64>>,
    correlation: Vec<f64>,
    /// The sum of the squares of the first `n` frames of the sound, at `n`.
    energy: Vec<f64>,
    /// The difference of the sound with itself at each lag, and that difference divided by
    /// its mean over the shorter lags, which is how YIN tells a clear period from a weak one.
    difference: Vec<f64>,
    normalized: Vec<f64>,
    /// The period followed, in frames, and for how many looks it has not been found.
    followed: Option<f64>,
    lost_looks: usize,
    pitch: Pitch,
    confidence: f64,
}

impl PitchTracker {
    /// A tracker for one channel at `rate`. It reads a note about one period of the lowest note
    /// after it starts.
    pub fn new(rate: f64, settings: PitchSettings) -> Self {
        // Under 1 Hz a look would grow without bound.
        let lowest = settings.lowest.max(1.0);
        let longest = ((rate / lowest).ceil() as usize).max(4);
        let shortest = ((rate / settings.highest).floor() as usize).max(3) - 1;
        let mut planner = RealFftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(2 * longest);
        let inverse = planner.plan_fft_inverse(2 * longest);
        let scratch = forward.get_scratch_len().max(inverse.get_scratch_len());
        let finding = settings.finding();
        Self {
            rate,
            lowest,
            highest: settings.highest,
            shortest,
            longest,
            finding,
            keeping: 2.0 * finding,
            look_frames: ((rate * LOOK_SECONDS).round() as usize).max(1),
            hold_looks: (HOLD_SECONDS / LOOK_SECONDS).round() as usize,
            ring: vec![0.0; 2 * longest],
            next: 0,
            since_look: 0,
            sound: vec![0.0; 2 * longest],
            input: forward.make_input_vec(),
            head: forward.make_output_vec(),
            whole: forward.make_output_vec(),
            correlation: inverse.make_output_vec(),
            scratch: vec![Complex::default(); scratch],
            energy: vec![0.0; 2 * longest + 1],
            difference: vec![0.0; longest + 1],
            normalized: vec![0.0; longest + 1],
            forward,
            inverse,
            followed: None,
            lost_looks: 0,
            pitch: Pitch::Quiet,
            confidence: 0.0,
        }
    }

    /// Hears the next frames of the channel.
    pub fn push(&mut self, samples: impl IntoIterator<Item = f32>) {
        for sample in samples {
            if let Some(slot) = self.ring.get_mut(self.next) {
                *slot = sample;
            }
            self.next = (self.next + 1) % self.ring.len();
            self.since_look += 1;
            if self.since_look == self.look_frames {
                self.since_look = 0;
                self.look();
            }
        }
    }

    /// How many frames a look hears. Its pitch is that of about their middle, half of them back.
    pub fn length(&self) -> usize {
        self.ring.len()
    }

    /// The pitch of the last [`Self::length`] frames, looked at in the last few milliseconds.
    pub fn pitch(&self) -> Pitch {
        self.pitch
    }

    /// How far [`Self::pitch`] can be trusted, from 0, a note only just told from noise or no
    /// note at all, to 1, a sound that repeats exactly.
    pub fn confidence(&self) -> f64 {
        self.confidence
    }

    fn look(&mut self) {
        let (older, newer) = self.ring.split_at(self.next.min(self.ring.len()));
        for (sound, sample) in self.sound.iter_mut().zip(newer.iter().chain(older)) {
            *sound = f64::from(*sample);
        }
        let length = self.sound.len() as f64;
        let mut sum = 0.0;
        for (energy, sound) in self.energy.iter_mut().skip(1).zip(&self.sound) {
            sum += sound * sound;
            *energy = sum;
        }
        let power = sum / length;
        if power < QUIET {
            self.followed = None;
            (self.pitch, self.confidence) = (Pitch::Quiet, 0.0);
            return;
        }
        let found = match self.correlate() {
            Ok(()) => {
                self.normalize();
                let followed = self.followed.and_then(|period| self.follow(period, power));
                followed.or_else(|| self.find(power))
            }
            Err(_) => None,
        };
        let Some((period, difference)) = found else {
            self.lost_looks += 1;
            if self.lost_looks > self.hold_looks {
                self.followed = None;
            }
            (self.pitch, self.confidence) = (Pitch::Unclear, 0.0);
            return;
        };
        (self.followed, self.lost_looks) = (Some(period), 0);
        let hz = self.rate / period;
        self.pitch = Pitch::Note(69.0 + 12.0 * (hz / 440.0).log2());
        self.confidence = (1.0 - difference / self.keeping).clamp(0.0, 1.0);
    }

    /// The difference of the first half with the sound `lag` frames on, summed, at every lag:
    /// the energy of both minus twice their correlation. Then normalized.
    fn normalize(&mut self) {
        let length = self.sound.len() as f64;
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
    }

    /// The first lag in range that repeats clearly enough to find a note.
    fn find(&self, power: f64) -> Option<(f64, f64)> {
        let lag = (self.shortest..self.longest).find(|lag| self.normalized(*lag) < self.finding)?;
        self.period(lag, power)
    }

    /// The clearest lag near `period`, while it still repeats clearly enough to keep.
    fn follow(&self, period: f64, power: f64) -> Option<(f64, f64)> {
        let reach = 2_f64.powf(FOLLOW_SEMITONES / 12.0);
        let lowest = ((period / reach).floor() as usize).max(self.shortest);
        let highest = ((period * reach).ceil() as usize).min(self.longest - 1);
        let lag = (lowest..=highest)
            .min_by(|a, b| self.normalized(*a).total_cmp(&self.normalized(*b)))?;
        if self.normalized(lag) >= self.keeping {
            return None;
        }
        // The bottom of the dip can be far off, at a period a chord shares.
        let found = self.period(lag, power)?;
        (period / reach..=period * reach)
            .contains(&found.0)
            .then_some(found)
    }

    fn normalized(&self, lag: usize) -> f64 {
        self.normalized.get(lag).copied().unwrap_or(f64::INFINITY)
    }

    /// The period of the dip `lag` is in, between the frames by a parabola through the
    /// difference, and its normalized difference. `None` when the sound has no power at its
    /// frequency, or it is out of range.
    fn period(&self, mut lag: usize, power: f64) -> Option<(f64, f64)> {
        loop {
            let here = self.normalized(lag);
            if self.normalized(lag + 1) < here {
                lag += 1;
            } else if self.normalized(lag - 1) < here {
                lag -= 1;
            } else {
                break;
            }
            if !(self.shortest..self.longest).contains(&lag) {
                return None;
            }
        }
        let difference = |lag: usize| self.difference.get(lag).copied();
        let (before, here, after) = (difference(lag - 1)?, difference(lag)?, difference(lag + 1)?);
        let curve = before - 2.0 * here + after;
        let offset = match curve > 0.0 {
            true => ((before - after) / (2.0 * curve)).clamp(-1.0, 1.0),
            false => 0.0,
        };
        let period = lag as f64 + offset;
        let hz = self.rate / period;
        if !(self.lowest..=self.highest).contains(&hz) {
            return None;
        }
        let least = FUNDAMENTAL * power;
        let heard = self.power_at(hz) >= least
            || (self.power_at(2.0 * hz) >= least && !self.repeats_at(period / 2.0));
        heard.then_some((period, self.normalized(lag)))
    }

    /// Whether the sound repeats clearly enough at `period` to find a note there.
    fn repeats_at(&self, period: f64) -> bool {
        let near = period.floor() as usize..=period.ceil() as usize;
        near.into_iter()
            .any(|lag| self.normalized(lag) < self.finding)
    }

    /// The mean square of the part of the sound that is a sine at `hz`.
    fn power_at(&self, hz: f64) -> f64 {
        let step = Complex::from_polar(1.0, std::f64::consts::TAU * hz / self.rate);
        let mut phase = Complex::new(1.0, 0.0);
        let mut sum = Complex::new(0.0, 0.0);
        for sound in &self.sound {
            sum += phase * sound;
            phase *= step;
        }
        // A sine of amplitude `a` sums to `a * length / 2` here, and has a mean square of
        // `a * a / 2`.
        let length = self.sound.len() as f64;
        2.0 * sum.norm_sqr() / (length * length)
    }

    /// The correlation of the first half of the sound with the whole, at every lag, through
    /// the spectra of both: the sum of `sound[j] * sound[j + lag]` over the first half.
    fn correlate(&mut self) -> Result<(), realfft::FftError> {
        self.input.copy_from_slice(&self.sound);
        self.forward
            .process_with_scratch(&mut self.input, &mut self.whole, &mut self.scratch)?;
        let head = self.sound.iter().take(self.longest);
        for (input, sound) in self
            .input
            .iter_mut()
            .zip(head.chain(std::iter::repeat(&0.0)))
        {
            *input = *sound;
        }
        self.forward
            .process_with_scratch(&mut self.input, &mut self.head, &mut self.scratch)?;
        for (whole, head) in self.whole.iter_mut().zip(&self.head) {
            *whole *= head.conj();
        }
        self.inverse
            .process_with_scratch(&mut self.whole, &mut self.correlation, &mut self.scratch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;
    const VOICE: PitchSettings = PitchSettings {
        lowest: 80.0,
        highest: 1_000.0,
        tolerance: 0.5,
    };

    fn hz(note: f64) -> f64 {
        440.0 * 2_f64.powf((note - 69.0) / 12.0)
    }

    /// `seconds` of a tone whose note and the amplitude of each harmonic go with the time.
    fn tone(
        seconds: f64,
        note: impl Fn(f64) -> f64,
        harmonics: impl Fn(f64) -> Vec<f64>,
    ) -> Vec<f32> {
        let mut phase = 0.0;
        (0..(seconds * RATE) as usize)
            .map(|frame| {
                let time = frame as f64 / RATE;
                phase += std::f64::consts::TAU * hz(note(time)) / RATE;
                let sample: f64 = harmonics(time)
                    .iter()
                    .enumerate()
                    .map(|(index, amplitude)| amplitude * (phase * (index + 1) as f64).sin())
                    .sum();
                sample as f32
            })
            .collect()
    }

    fn bright(_: f64) -> Vec<f64> {
        vec![0.4, 0.2, 0.13, 0.1, 0.08]
    }

    /// White noise from a fixed seed, by xorshift.
    fn noise(seconds: f64) -> Vec<f32> {
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        (0..(seconds * RATE) as usize)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state as f64 / u64::MAX as f64 - 0.5) as f32
            })
            .collect()
    }

    /// What the tracker reads after each look, with the time of the middle of what it heard.
    fn track(settings: PitchSettings, sound: &[f32]) -> Vec<(f64, Pitch)> {
        let mut tracker = PitchTracker::new(RATE, settings);
        let mut heard = 0;
        sound
            .chunks(tracker.look_frames)
            .map(|frames| {
                tracker.push(frames.iter().copied());
                heard += frames.len();
                let middle = heard as f64 - tracker.length() as f64 / 2.0;
                (middle / RATE, tracker.pitch())
            })
            .collect()
    }

    /// The notes read after `after` seconds, with their times.
    fn notes(readings: &[(f64, Pitch)], after: f64) -> Vec<(f64, f64)> {
        let readings = readings.iter().filter(|(time, _)| *time > after);
        readings
            .filter_map(|(time, pitch)| match pitch {
                Pitch::Note(note) => Some((*time, *note)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_note_whose_fundamental_fades_under_a_loud_octave_stays_on_its_note() {
        // A3 whose fundamental fades to a fifth of its octave, as a voice's does when it opens.
        let fading = |time: f64| vec![(0.3 - 0.4 * (time - 0.3).clamp(0.0, 0.6)), 0.3];
        let sound = tone(1.5, |_| 57.0, fading);
        let readings = track(PitchSettings::FULL, &sound);
        // From when the tracker has heard only the note.
        for (time, pitch) in readings.iter().filter(|(time, _)| *time >= 0.025) {
            let Pitch::Note(note) = pitch else {
                panic!("{pitch:?} at {time}");
            };
            assert!((note - 57.0).abs() < 0.05, "{note} at {time}");
        }
        // Heard on its own, the faded part reads as the octave: following is what keeps it.
        let faded = sound.get((0.9 * RATE) as usize..).unwrap();
        let (_, last) = *track(PitchSettings::FULL, faded).last().unwrap();
        assert!(
            matches!(last, Pitch::Note(note) if (note - 69.0).abs() < 0.05),
            "{last:?}"
        );
    }

    #[test]
    fn a_note_an_octave_up_is_followed_and_not_held_at_the_old_one() {
        let sound = tone(1.0, |time| if time < 0.5 { 57.0 } else { 69.0 }, bright);
        let readings = track(VOICE, &sound);
        let (_, after) = notes(&readings, 0.55)[0];
        assert!((after - 69.0).abs() < 0.05, "{after}");
    }

    #[test]
    fn a_glide_and_a_vibrato_are_followed() {
        // An octave up in half a second, then 50 cents up and down six times a second.
        let note = |time: f64| match time < 0.5 {
            true => 57.0 + 24.0 * time,
            false => 69.0 + 0.5 * (std::f64::consts::TAU * 6.0 * (time - 0.5)).sin(),
        };
        let readings = track(VOICE, &tone(1.5, note, bright));
        let sounding = readings.iter().filter(|(time, _)| *time > 0.05);
        let followed = notes(&readings, 0.05);
        assert_eq!(sounding.count(), followed.len());
        for (time, found) in &followed {
            assert!((found - note(*time)).abs() < 0.15, "{found} at {time}");
        }
        let vibrato = notes(&readings, 0.6).into_iter().map(|(_, note)| note);
        let (low, high) = vibrato.fold((f64::MAX, f64::MIN), |(low, high), note| {
            (low.min(note), high.max(note))
        });
        assert!(high - low > 0.9, "{low} to {high}");
    }

    #[test]
    fn noise_and_a_chord_have_no_pitch_even_after_a_note() {
        let readings = track(PitchSettings::FULL, &noise(1.0));
        assert_eq!(notes(&readings, 0.0), []);
        // C4 alone, then E4 and G4 join it; A2, then C#3.
        for (root, joining) in [(60.0, vec![64.0, 67.0]), (45.0, vec![49.0])] {
            let mut sound = tone(1.0, |_| root, |_| vec![0.3]);
            for note in joining {
                let joining = tone(
                    1.0,
                    |_| note,
                    |time| vec![if time < 0.5 { 0.0 } else { 0.3 }],
                );
                for (sample, joining) in sound.iter_mut().zip(joining) {
                    *sample += joining;
                }
            }
            let readings = track(PitchSettings::FULL, &sound);
            assert!((notes(&readings, 0.0)[0].1 - root).abs() < 0.05);
            // Past the hold and the length of a look.
            assert_eq!(notes(&readings, 0.6), [], "{root}");
        }
    }

    #[test]
    fn a_note_reads_within_one_look_of_filling_what_the_tracker_hears() {
        for settings in [VOICE, PitchSettings::FULL] {
            let mut sound = vec![0.0; (0.2 * RATE) as usize];
            sound.extend(tone(0.5, |_| 57.0, bright));
            let mut tracker = PitchTracker::new(RATE, settings);
            tracker.push(sound.iter().copied().take((0.2 * RATE) as usize));
            assert_eq!(tracker.pitch(), Pitch::Quiet);
            let start = (0.2 * RATE) as usize;
            let delay = sound
                .iter()
                .skip(start)
                .position(|sample| {
                    tracker.push([*sample]);
                    matches!(tracker.pitch(), Pitch::Note(_))
                })
                .unwrap();
            assert!(delay <= tracker.length() + tracker.look_frames, "{delay}");
            assert!(tracker.confidence() > 0.5, "{}", tracker.confidence());
        }
    }
}
