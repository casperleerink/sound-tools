//! The pitch of one melodic voice as it goes on: whether it has one, and its period.
//!
//! The tracker works on a detection signal at up to 48 kHz: above 50 kHz every second sample
//! of the sound, above 100 kHz every fourth, after a low pass. While it looks for a note, an
//! eighth of that rate, low-passed at twice the highest pitch and high-passed at 30 Hz, fills a
//! short history. For every lag of that history it keeps the energy and the correlation, and so
//! the difference of the sound with itself one lag on, normalized: 0 repeats exactly, 0.5 is
//! unrelated sound. A period is taken only from two or three minima under the threshold of the
//! tolerance that sit evenly spaced, within a tenth: a period, twice it and three times it. A
//! lone minimum counts only for a bass. So a loud octave is not taken for the note, and noise,
//! which has no evenly spaced minima, has no pitch.
//!
//! Once a period is found, the tracker follows it at the full detection rate on a small
//! neighbourhood of lags around it, [`PitchSettings::width`] each side, with running sums that
//! cost a few operations a sample. Every [`EVALUATE`] samples it takes the lag where the sound
//! differs least from itself, between its neighbours by a parabola, and moves the neighbourhood
//! by at most one lag toward it. A period that leaves the neighbourhood faster than that, or a
//! sound too quiet to follow, loses the note, and the tracker looks for one again from an empty
//! history.
//!
//! Ours on top: whether the sound between notes is quiet or unclear, and two checks. A chord can
//! repeat too: C, E and G together repeat at the period of a C two octaves under them, a note
//! nobody plays. So a period is found only when the sound has power at its frequency or the
//! octave above. A note whose octave is louder than itself has evenly spaced minima at the
//! octave too, so twice the period is taken when the sound repeats there and has power at its
//! frequency. And a note an octave up still repeats at the old period, so a note followed is
//! let go when the sound repeats at half its period and has no power at its frequency.

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

/// The lowest pitch pitch correction and a stretched clip look for, in Hz: a lower one needs a
/// longer history, and the latency of pitch correction is set for this one.
pub const LOWEST_HZ: f64 = 60.0;

/// Under this mean square, -60 dB, the sound is too quiet to tell a pitch in.
const QUIET: f64 = 1e-6;
/// How fast the level that tells quiet sound falls, in seconds.
const QUIET_SECONDS: f64 = 0.02;
/// The least share of the power of the sound at the frequency of a period found, or at twice
/// it. A sawtooth has 61% there, a bright tone a few; a chord read as its root has almost
/// nothing.
const FUNDAMENTAL: f64 = 0.02;
/// The detection signal is at most this fast.
const DETECTION_HZ: f64 = 50_000.0;
/// One sample of the history a note is looked for in per this many of the detection signal.
const COARSE: usize = 8;
/// The most lags the history holds, and the samples it keeps.
const MOST_LAGS: usize = 250;
const HISTORY: usize = 2 * MOST_LAGS;
/// Lookings for a note before the history is long enough to find one.
const FIRST_LOOK: usize = 10;
/// A lag is a candidate only after the sound rose over this normalized difference, past the
/// lags where it still resembles itself.
const RISEN: f64 = 0.6;
/// How evenly minima must be spaced, as a part of the first.
const EVEN: f64 = 0.1;
/// The energy of two periods under which a note is not followed, as a sum of squares: once to
/// start following, and while following.
const STARTS: f64 = 1.65e-5;
const KEEPS: f64 = 1.5e-5;
/// A followed period is worked out every this many samples of the detection signal.
const EVALUATE: usize = 5;
/// Evaluations from one check for a note an octave up to the next.
const OCTAVE_CHECKS: usize = 4;
/// The pole of the high pass of the history, in Hz.
const HIGH_PASS_HZ: f64 = 30.0;
/// The sections of the low pass before the detection signal drops samples.
const SECTIONS: usize = 5;

/// What the tracker looks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchSettings {
    /// The lowest and the highest fundamental looked for, in Hz. The highest sets where the
    /// history is low-passed, at twice it; the lowest sets how long the history is.
    pub lowest: f64,
    pub highest: f64,
    /// How clearly a sound must repeat to read as a note, from 0, strict, to 1, tolerant. A
    /// tolerant tracker holds a breathy note but also hears notes in more noise.
    pub tolerance: f64,
    /// How many lags each side of the period are followed. A wide neighbourhood follows a
    /// fast glide, a narrow one lets go of a note sooner.
    pub width: usize,
    /// Whether a lone minimum is a period, for a bass, whose low notes leave room for only one
    /// in the history.
    pub lone_minimum: bool,
}

impl PitchSettings {
    /// Every note an instrument plays: about E1, the lowest string of a bass, to about B7.
    pub const FULL: Self = Self {
        lowest: 40.0,
        highest: 4_000.0,
        tolerance: 0.5,
        width: 15,
        lone_minimum: false,
    };

    /// The normalized difference a minimum must be under to count.
    fn threshold(&self) -> f64 {
        0.2 * self.tolerance.clamp(0.0, 1.0)
    }
}

/// The factors of one second-order section: `b0`, `b1`, `b2`, `a1`, `a2`, and its memory.
#[derive(Clone, Copy, Default)]
struct Section {
    factors: [f64; 5],
    memory: [f64; 2],
}

impl Section {
    /// A Butterworth-shaped low or high pass at `hz`, with quality `q`, at `rate`.
    fn new(hz: f64, q: f64, rate: f64, high: bool) -> Self {
        let angle = 2.0 * std::f64::consts::PI * (hz / rate).min(0.49);
        let (sin, cos) = angle.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1) = match high {
            true => ((1.0 + cos) / 2.0, -(1.0 + cos)),
            false => ((1.0 - cos) / 2.0, 1.0 - cos),
        };
        Self {
            factors: [
                b0 / a0,
                b1 / a0,
                b0 / a0,
                -2.0 * cos / a0,
                (1.0 - alpha) / a0,
            ],
            memory: [0.0; 2],
        }
    }

    #[inline]
    fn filter(&mut self, input: f64) -> f64 {
        let [b0, b1, b2, a1, a2] = self.factors;
        let output = b0 * input + self.memory[0];
        self.memory[0] = b1 * input - a1 * output + self.memory[1];
        self.memory[1] = b2 * input - a2 * output;
        output
    }
}

/// Follows the pitch of one channel of sound. Everything it needs is made in [`Self::new`], so
/// [`Self::push`] may run on the audio thread.
pub struct PitchTracker {
    rate: f64,
    /// Samples of the sound per sample of the detection signal, and how many came since the
    /// last.
    steps: usize,
    since: usize,
    threshold: f64,
    width: usize,
    lone_minimum: bool,
    /// The low pass before samples are dropped, and the low and high pass of the history.
    before: [Section; SECTIONS],
    low: Section,
    high: Section,
    /// The detection signal, newest at `newest`, and how many samples it has had.
    signal: Box<[f64]>,
    newest: usize,
    heard: u64,
    /// The history a note is looked for in, the next place in it, how many samples it has,
    /// and for every lag its energy, its correlation and its normalized difference.
    history: [f64; HISTORY],
    next: usize,
    looked: usize,
    lags: usize,
    energy: Box<[f64]>,
    correlation: Box<[f64]>,
    normalized: Box<[f64]>,
    /// Whether a note is followed, the lag of the first of its neighbourhood, and for each lag
    /// of it its energy over two lags and its correlation; then samples to the next evaluation.
    following: bool,
    first: usize,
    longest: usize,
    near_energy: Box<[f64]>,
    near_correlation: Box<[f64]>,
    until: usize,
    checks: usize,
    /// The level of the sound, falling, for quiet; the pitch, its period in frames, and how far
    /// back it describes.
    level: f64,
    fall: f64,
    pitch: Pitch,
    period: f64,
    age: f64,
}

impl PitchTracker {
    /// A tracker for one channel at `rate`.
    pub fn new(rate: f64, settings: PitchSettings) -> Self {
        let steps = match rate {
            rate if rate < DETECTION_HZ => 1,
            rate if rate < 2.0 * DETECTION_HZ => 2,
            _ => 4,
        };
        let detection = rate / steps as f64;
        // Under 1 Hz a history would grow without bound.
        let lowest = settings.lowest.max(1.0);
        let lags = ((detection / (COARSE as f64 * lowest)).ceil() as usize + 2).clamp(5, MOST_LAGS);
        let width = settings.width.max(2);
        let longest = COARSE * lags + 2 * width;
        let quality = |section: usize| {
            let angle = std::f64::consts::PI * (2 * section + 1) as f64 / (4 * SECTIONS) as f64;
            1.0 / (2.0 * angle.cos())
        };
        let before = std::array::from_fn(|section| {
            Section::new(0.45 * detection, quality(section), rate, false)
        });
        let root_half = std::f64::consts::FRAC_1_SQRT_2;
        Self {
            rate,
            steps,
            since: 0,
            threshold: settings.threshold(),
            width,
            lone_minimum: settings.lone_minimum,
            before,
            low: Section::new(2.0 * settings.highest, root_half, detection, false),
            high: Section::new(HIGH_PASS_HZ, root_half, detection, true),
            signal: vec![0.0; 2 * longest + 2].into(),
            newest: 0,
            heard: 0,
            history: [0.0; HISTORY],
            next: 0,
            looked: 0,
            lags,
            energy: vec![0.0; lags].into(),
            correlation: vec![0.0; lags].into(),
            normalized: vec![0.0; lags].into(),
            following: false,
            first: 0,
            longest,
            near_energy: vec![0.0; 2 * width].into(),
            near_correlation: vec![0.0; 2 * width].into(),
            until: 0,
            checks: 0,
            level: 0.0,
            fall: (-1.0 / (QUIET_SECONDS * detection)).exp(),
            pitch: Pitch::Quiet,
            period: 0.0,
            age: 0.0,
        }
    }

    /// Hears the next frames of the channel.
    pub fn push(&mut self, samples: impl IntoIterator<Item = f32>) {
        for sample in samples {
            let sample = f64::from(sample);
            let mut detected = sample;
            if self.steps > 1 {
                for section in &mut self.before {
                    detected = section.filter(detected);
                }
            }
            self.since += 1;
            if self.since < self.steps {
                continue;
            }
            self.since = 0;
            self.detect(sample, detected);
        }
    }

    /// How many frames a reading can depend on: the history a note is found in.
    pub fn length(&self) -> usize {
        (2 * COARSE * self.lags).max(2 * self.longest) * self.steps
    }

    /// How many frames before the newest one heard [`Self::pitch`] describes, on average until
    /// the next evaluation: a followed period is measured on its last two periods.
    pub fn age(&self) -> f64 {
        self.age
    }

    /// The pitch as of the last evaluation.
    pub fn pitch(&self) -> Pitch {
        self.pitch
    }

    /// The period of [`Self::pitch`] in frames, when it is a note.
    pub fn followed_period(&self) -> Option<f64> {
        match self.pitch {
            Pitch::Note(_) => Some(self.period),
            Pitch::Quiet | Pitch::Unclear => None,
        }
    }

    /// One sample of the detection signal: `sound` as it came, for the history, and `detected`,
    /// low-passed, for following.
    fn detect(&mut self, sound: f64, detected: f64) {
        self.newest = (self.newest + 1) % self.signal.len();
        if let Some(slot) = self.signal.get_mut(self.newest) {
            *slot = detected;
        }
        self.heard += 1;
        self.level = (detected * detected).max(self.fall * self.level);
        let history = self.high.filter(self.low.filter(sound));
        if self.following {
            self.follow_sample();
            self.until -= 1;
            if self.until == 0 {
                self.until = EVALUATE;
                if !self.evaluate() {
                    self.lose();
                }
            }
        } else if self.heard.is_multiple_of(COARSE as u64) {
            if let Some(lag) = self.look(history) {
                self.start(lag);
            }
            if !self.following {
                self.pitch = match self.level < QUIET {
                    true => Pitch::Quiet,
                    false => Pitch::Unclear,
                };
            }
        }
    }

    /// The detection signal `back` samples before the newest.
    fn at(&self, back: usize) -> f64 {
        let length = self.signal.len();
        let index = (self.newest + length - back % length) % length;
        self.signal.get(index).copied().unwrap_or(0.0)
    }

    /// Adds `sample` to the history and looks for a note in it: the lag of the period found,
    /// in samples of the detection signal.
    fn look(&mut self, sample: f64) -> Option<usize> {
        let newest = self.next;
        self.next = (self.next + 1) % HISTORY;
        if let Some(slot) = self.history.get_mut(newest) {
            *slot = sample;
        }
        let history = &self.history;
        let back = |lag: usize| {
            let at = (newest + HISTORY - lag % HISTORY) % HISTORY;
            history.get(at).copied().unwrap_or(0.0)
        };
        let lags = self.energy.iter_mut().zip(self.correlation.iter_mut());
        for (lag, (energy, correlation)) in lags.enumerate() {
            let (one, two) = (back(lag), back(2 * lag));
            *correlation += one * sample - one * two;
            *energy += sample * sample - two * two;
        }
        let looked = self.looked;
        self.looked += 1;
        if looked < FIRST_LOOK {
            return None;
        }
        let searched = ((looked - 5) >> 1).min(self.lags - 1);
        let pairs = self.energy.iter().zip(self.correlation.iter());
        for (lag, (normalized, (energy, correlation))) in self
            .normalized
            .iter_mut()
            .zip(pairs)
            .enumerate()
            .take(searched)
        {
            *normalized = match (lag, *energy > 0.0) {
                (0, _) => 0.0,
                (_, true) => (0.5 * energy - correlation) / energy,
                (_, false) => 1.0,
            };
        }
        let normalized = |lag: usize| self.normalized.get(lag).copied().unwrap_or(1.0);
        let start = match searched >= 5 {
            true => (2..=searched - 3)
                .find(|lag| normalized(*lag) > RISEN)
                .unwrap_or(searched - 2),
            false => 2,
        };
        if start + 1 >= searched - 1 {
            return None;
        }
        let mut minima = [0.0; 3];
        let mut found = 0;
        for lag in start + 1..=searched - 2 {
            let (before, here, after) = (normalized(lag - 1), normalized(lag), normalized(lag + 1));
            if !(here < before && here < after) {
                continue;
            }
            let curve = before - 2.0 * here + after;
            let offset = 0.5 * (before - after) / curve;
            let lowest = 0.5 * before * offset * (offset - 1.0)
                - here * (offset + 1.0) * (offset - 1.0)
                + 0.5 * after * offset * (offset + 1.0);
            if lowest < self.threshold {
                if let Some(slot) = minima.get_mut(found) {
                    *slot = COARSE as f64 * (lag as f64 + offset);
                }
                found += 1;
                if found == 3 {
                    break;
                }
            }
        }
        let [first, second, third] = minima;
        let period = match found {
            3 => {
                let (one, two) = (second - first, third - second);
                if (first - one).abs() < EVEN * first && (one - two).abs() < EVEN * first {
                    (first + one + two) / 3.0
                } else if (one - two).abs() < EVEN * one {
                    // The first dip was not a period.
                    (one + two) / 2.0
                } else {
                    return None;
                }
            }
            2 if (first - (second - first)).abs() < EVEN * first => (first + second - first) / 2.0,
            1 if self.lone_minimum => first,
            _ => return None,
        };
        // A note with its octave louder than itself has evenly spaced minima at the octave too.
        let period = match self.under(period, searched) {
            true => 2.0 * period,
            false => period,
        };
        let lag = (period + 0.5) as usize;
        let heard =
            self.share_at(period, 1) >= FUNDAMENTAL || self.share_at(period, 2) >= FUNDAMENTAL;
        (lag > 0 && heard).then_some(lag)
    }

    /// Whether the note is an octave under `period`: the history repeats at twice it too, and
    /// the sound has power at its frequency. `searched` lags of the history are worked out.
    fn under(&self, period: f64, searched: usize) -> bool {
        let lag = 2.0 * period / COARSE as f64;
        let near = lag.floor() as usize..=lag.ceil() as usize;
        let repeats = (near.clone().all(|lag| lag + 1 < searched))
            && near.into_iter().any(|lag| {
                self.normalized
                    .get(lag)
                    .is_some_and(|difference| *difference < self.threshold)
            });
        repeats && self.share_at(2.0 * period, 1) >= FUNDAMENTAL
    }

    /// The share of the power of the last two periods of `period` samples that is at its
    /// `harmonic`, or of the last one when two are longer than the detection signal kept.
    fn share_at(&self, period: f64, harmonic: usize) -> f64 {
        let periods = match 2.0 * period < self.signal.len() as f64 {
            true => 2.0,
            false => 1.0,
        };
        let length = (periods * period).round().max(1.0) as usize;
        let angle = std::f64::consts::TAU * harmonic as f64 / period;
        let (step_sine, step_cosine) = angle.sin_cos();
        let (mut sine, mut cosine) = (0.0, 1.0);
        let (mut power, mut along, mut across) = (0.0, 0.0, 0.0);
        for back in 0..length {
            let sample = self.at(back);
            power += sample * sample;
            along += sample * cosine;
            across += sample * sine;
            (sine, cosine) = (
                sine * step_cosine + cosine * step_sine,
                cosine * step_cosine - sine * step_sine,
            );
        }
        // A sine of amplitude `a` sums to `a * length / 2` over whole periods, and has a mean
        // square of `a * a / 2`.
        let length = length as f64;
        let at = 2.0 * (along * along + across * across) / (length * length);
        match power > 0.0 {
            true => at / (power / length),
            false => 0.0,
        }
    }

    /// Whether the sound followed at `lag` is a note an octave up now: it repeats clearly at
    /// half the lag, and has no power at the frequency followed.
    fn octave_up(&self, lag: f64) -> bool {
        let half = (lag / 2.0).round() as usize;
        if half < 2 {
            return false;
        }
        let difference: f64 = (0..half)
            .map(|back| (self.at(back) - self.at(back + half)).powi(2))
            .sum();
        let energy: f64 = (0..2 * half).map(|back| self.at(back).powi(2)).sum();
        energy > 0.0
            && 0.5 * difference / energy < self.threshold
            && self.share_at(lag, 1) < FUNDAMENTAL
    }

    /// Starts following the period of `lag` samples, unless the sound is too quiet.
    fn start(&mut self, lag: usize) {
        let first = lag.saturating_sub(self.width).max(1);
        if first + 2 * self.width > self.longest {
            return;
        }
        let energy: f64 = (0..2 * first).map(|back| self.at(back).powi(2)).sum();
        if energy < STARTS {
            return;
        }
        self.first = first;
        for slot in 0..2 * self.width {
            self.fill(slot);
        }
        self.following = true;
        self.until = 1;
    }

    /// Works out the energy and the correlation of one lag of the neighbourhood anew.
    fn fill(&mut self, slot: usize) {
        let lag = self.first + slot;
        let energy: f64 = (0..2 * lag).map(|back| self.at(back).powi(2)).sum();
        let correlation: f64 = (0..lag)
            .map(|back| self.at(back) * self.at(back + lag))
            .sum();
        if let (Some(near_energy), Some(near_correlation)) = (
            self.near_energy.get_mut(slot),
            self.near_correlation.get_mut(slot),
        ) {
            (*near_energy, *near_correlation) = (energy, correlation);
        }
    }

    /// Moves the sums of every lag of the neighbourhood on by the newest sample.
    fn follow_sample(&mut self) {
        let newest = self.at(0);
        for slot in 0..2 * self.width {
            let lag = self.first + slot;
            let (one, two) = (self.at(lag), self.at(2 * lag));
            if let (Some(energy), Some(correlation)) = (
                self.near_energy.get_mut(slot),
                self.near_correlation.get_mut(slot),
            ) {
                *energy += newest * newest - two * two;
                *correlation += one * (newest - two);
            }
        }
    }

    /// Where the sound differs least from itself in the neighbourhood: the period, the
    /// neighbourhood moved a lag toward it. False when the note is lost.
    fn evaluate(&mut self) -> bool {
        let difference = |tracker: &Self, slot: usize| {
            let energy = tracker.near_energy.get(slot).copied().unwrap_or(0.0);
            let correlation = tracker.near_correlation.get(slot).copied().unwrap_or(0.0);
            0.5 * energy - correlation
        };
        let slots = 2 * self.width;
        let mut best = 0;
        for slot in 1..slots {
            if difference(self, slot) < difference(self, best) {
                best = slot;
            }
        }
        let energy = self.near_energy.get(best).copied().unwrap_or(0.0);
        if best == 0 || best == slots - 1 || energy < KEEPS {
            return false;
        }
        if best + 1 < self.width && self.first > 1 {
            self.first -= 1;
            self.near_energy.copy_within(..slots - 1, 1);
            self.near_correlation.copy_within(..slots - 1, 1);
            self.fill(0);
            best += 1;
        } else if best > self.width && self.first + slots < self.longest {
            self.first += 1;
            self.near_energy.copy_within(1.., 0);
            self.near_correlation.copy_within(1.., 0);
            self.fill(slots - 1);
            best -= 1;
        }
        let (before, here, after) = (
            difference(self, best - 1),
            difference(self, best),
            difference(self, best + 1),
        );
        let curve = before - 2.0 * here + after;
        let offset = match curve > 0.0 {
            true => (0.5 * (before - after) / curve).clamp(-1.0, 1.0),
            false => 0.0,
        };
        let lag = (self.first + best) as f64 + offset;
        self.checks = (self.checks + 1) % OCTAVE_CHECKS;
        if self.checks == 0 && self.octave_up(lag) {
            return false;
        }
        let period = lag * self.steps as f64;
        self.pitch = Pitch::Note(69.0 + 12.0 * (self.rate / period / 440.0).log2());
        self.period = period;
        self.age = period + 0.5 * (EVALUATE * self.steps) as f64;
        true
    }

    /// Lets go of the note: the history is looked in anew from empty.
    fn lose(&mut self) {
        self.following = false;
        self.history = [0.0; HISTORY];
        self.energy.fill(0.0);
        self.correlation.fill(0.0);
        (self.next, self.looked) = (0, 0);
        self.pitch = match self.level < QUIET {
            true => Pitch::Quiet,
            false => Pitch::Unclear,
        };
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
        width: 15,
        lone_minimum: false,
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

    /// What the tracker reads every 5 ms, with the time of the sound it describes.
    fn track(settings: PitchSettings, sound: &[f32]) -> Vec<(f64, Pitch)> {
        let mut tracker = PitchTracker::new(RATE, settings);
        let mut heard = 0;
        sound
            .chunks(240)
            .map(|frames| {
                tracker.push(frames.iter().copied());
                heard += frames.len();
                ((heard as f64 - tracker.age()) / RATE, tracker.pitch())
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
        // Heard on its own, the faded part reads near the octave: following is what keeps it.
        let faded = sound.get((0.9 * RATE) as usize..).unwrap();
        let (_, last) = *track(PitchSettings::FULL, faded).last().unwrap();
        assert!(
            matches!(last, Pitch::Note(note) if (note - 69.0).abs() < 1.0),
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
    fn a_note_reads_within_what_the_tracker_hears() {
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
            assert!(delay <= tracker.length(), "{delay}");
        }
    }

    #[test]
    fn a_reading_on_a_glide_is_the_note_sung_its_age_back() {
        // Six semitones a second, read every frame.
        let note = |time: f64| 57.0 + 6.0 * time;
        let sound = tone(1.5, note, bright);
        for settings in [VOICE, PitchSettings::FULL] {
            let mut tracker = PitchTracker::new(RATE, settings);
            let mut errors = Vec::new();
            for (frame, sample) in sound.iter().enumerate() {
                tracker.push([*sample]);
                if let (Pitch::Note(read), true) = (tracker.pitch(), frame as f64 > 0.3 * RATE) {
                    let described = (frame as f64 + 1.0 - tracker.age()) / RATE;
                    errors.push(read - note(described));
                }
            }
            let mean = errors.iter().sum::<f64>() / errors.len() as f64;
            // A cent is 80 frames of this glide; half of what a look hears is 600 or more.
            assert!(mean.abs() < 0.01, "{mean} semitones off on average");
        }
    }

    /// Whether the tracker reads a note in `sound` at the end.
    fn reads(settings: PitchSettings, sound: &[f32]) -> bool {
        let mut tracker = PitchTracker::new(RATE, settings);
        tracker.push(sound.iter().copied());
        matches!(tracker.pitch(), Pitch::Note(_))
    }

    #[test]
    fn a_period_needs_minima_that_agree_unless_a_bass_takes_a_lone_one() {
        // A2, whose second dip lies past what a history from 100 Hz holds: one minimum.
        let low = PitchSettings {
            lowest: 100.0,
            highest: 300.0,
            width: 30,
            ..VOICE
        };
        let sound = tone(1.0, |_| 45.0, bright);
        assert!(!reads(low, &sound));
        let bass = PitchSettings {
            lone_minimum: true,
            ..low
        };
        assert!(reads(bass, &sound));
        // From 40 Hz the history holds two dips, a period apart.
        assert!(reads(PitchSettings::FULL, &sound));
    }

    #[test]
    fn a_note_too_quiet_over_two_periods_is_not_followed() {
        // A3 at -86 dB sums to less than the gate over two periods; at -46 dB, more.
        let at = |amplitude: f64| tone(1.0, |_| 57.0, move |_| vec![amplitude]);
        assert!(!reads(VOICE, &at(5e-5)));
        assert!(reads(VOICE, &at(5e-3)));
    }

    #[test]
    fn a_jump_wider_than_the_lags_followed_loses_the_note_and_finds_the_new_one() {
        // A3, then a semitone up at once: twelve lags shorter at 48 kHz.
        let jump = |time: f64| if time < 0.5 { 57.0 } else { 58.0 };
        let sound = tone(1.0, jump, bright);
        let narrow = PitchSettings { width: 6, ..VOICE };
        let wide = PitchSettings { width: 30, ..VOICE };
        let lost = |settings: PitchSettings| {
            let readings = track(settings, &sound);
            let after = readings
                .iter()
                .filter(|(time, _)| (0.5..0.6).contains(time));
            after.filter(|(_, pitch)| *pitch == Pitch::Unclear).count()
        };
        assert!(
            lost(narrow) > 0,
            "six lags each side follow a jump of twelve"
        );
        assert_eq!(lost(wide), 0, "thirty lags each side lose a jump of twelve");
        for settings in [narrow, wide] {
            let (_, last) = *track(settings, &sound).last().unwrap();
            assert!(
                matches!(last, Pitch::Note(note) if (note - 58.0).abs() < 0.05),
                "{last:?}"
            );
        }
    }

    #[test]
    fn a_glide_moves_the_lags_followed_by_one_at_a_time() {
        // Two semitones in a second: well under a lag per evaluation, so it is followed.
        let glide = |time: f64| 57.0 + 2.0 * time;
        let readings = track(
            PitchSettings { width: 6, ..VOICE },
            &tone(1.0, glide, bright),
        );
        for (time, pitch) in readings.iter().filter(|(time, _)| *time > 0.1) {
            let Pitch::Note(note) = pitch else {
                panic!("{pitch:?} at {time}");
            };
            assert!((note - glide(*time)).abs() < 0.05, "{note} at {time}");
        }
    }
}
