//! Shifts the pitch of one voice and keeps its length: what pitch correction plays, and what a
//! region that plays faster or slower than its file can feed.
//!
//! The sound goes into a ring and a head reads it back at `ratio` times its speed, 2 an octave
//! up. A head that reads faster than the sound comes in would catch up with it, and a slower
//! one would fall behind. So when the head is more than [`WINDOW`] of a period from where it
//! should be, it jumps one period of the sound back or ahead, and fades in a straight line
//! from where it was over [`FADE`] of a period. A jump of one period lands on the same part of
//! the wave, so the fade joins two nearly equal sounds: whole periods play twice or not at all,
//! and the length stays. When the readings lose the period, it holds for [`HOLD_PERIODS`] of
//! itself, as through a short consonant; after that a jump takes the last period heard, 440 Hz
//! before any.
//!
//! Every frame comes with what a pitch tracker read, which describes the sound some frames
//! back ([`Shift::age`]). The shifter keeps each reading with the frame it describes, so a
//! head plays each part at the ratio and with the period of that part. The latency leaves the
//! head room for that and for its window.
//!
//! The head reads between frames with a windowed sinc. Reading faster would fold what lies
//! above the new Nyquist frequency down, so there the sinc is stretched by the ratio over
//! [`CUTOFF`]: its transition then ends under that frequency, as in [`Varispeed`]. Just above
//! 1 the stretch eases in, so a correction that crosses 0 does not switch the highs. The sinc
//! is symmetric, so the sound read keeps its phase and lines up with the sound as it came, for
//! a mix of the two. A low pass in front of the ring would not.
//!
//! Around the head, a first-order tilt with its pole at [`TILT_HZ`] is taken out before the
//! ring and put back after it, so the two cancel where nothing moves. Without formants it
//! lifts the lows and lowers the highs before the ring; with them, the other way round.
//!
//! Reading faster also moves the envelope of the spectrum up, the formants that make a voice
//! sound like that voice. With formants kept, a second ring holds the sound with its envelope
//! taken out by linear prediction, and after the head the envelope of the part it reads is put
//! back. The envelope is worked out every [`DESIGN_FRAMES`] from the last period of the voice
//! as one seamless loop, so the harmonics of the voice do not pass for formants, and averaged
//! over the last [`DESIGNS`]. Both filters are lattices of reflection factors under 1, so a
//! factor read between two analyses never makes them ring.

use std::f64::consts::PI;
use std::sync::LazyLock;

#[cfg(doc)]
use crate::Varispeed;
use crate::resample::{SincTable, kaiser, sinc};
use sound_core::bessel_i0;

/// Taps each side of a place of the head, at a ratio up to 1.
const HALF: usize = 8;
const PHASES: usize = 256;
/// The Kaiser window of the sinc, about 70 dB down outside its main lobe.
const BETA: f64 = 7.0;
static TABLE: LazyLock<SincTable> = LazyLock::new(|| SincTable::new(HALF, PHASES, 1.0, BETA));
/// The same sinc at every `1 / PHASES` from `-HALF` to `HALF`, for the stretched one.
static CURVE: LazyLock<Box<[f32]>> = LazyLock::new(|| {
    let normal = bessel_i0(BETA);
    let curve = (0..=2 * HALF * PHASES).map(|index| {
        let distance = index as f64 / PHASES as f64 - HALF as f64;
        (sinc(distance) * kaiser(distance / HALF as f64, BETA, normal)) as f32
    });
    curve.collect()
});

/// How far the head may be from where it should be, as a part of the period, before it jumps.
const WINDOW: f64 = 0.6;
/// The fade of a jump, as a part of the period.
pub(crate) const FADE: f64 = 0.2;
/// How many of its own periods a period holds once the readings lose it.
pub(crate) const HOLD_PERIODS: f64 = 8.0;
/// The period a jump takes before any was heard, in Hz.
const UNHEARD_HZ: f64 = 440.0;
/// The shortest period a jump takes, in frames.
const SHORTEST_PERIOD: f64 = 16.0;
/// The ratios the head reads at.
const RATIOS: (f64, f64) = (0.5, 2.0);
/// Where the stretched sinc cuts, as a part of the Nyquist frequency over the ratio.
const CUTOFF: f64 = 0.85;
/// The ratios above 1 over which the cut moves from the Nyquist frequency to [`CUTOFF`].
const EASE: f64 = 0.2;
/// How long a change of formants takes.
const KEEP_SECONDS: f64 = 0.02;
/// How much louder than the sound shifted with its formants the sound with them kept may be,
/// and how fast the level of each falls.
const LOUDER: f32 = 2.0;
const LEVEL_SECONDS: f64 = 0.01;
/// The pole of the tilt around the head, in Hz.
const TILT_HZ: f64 = 360.0;

/// Reflection factors of the envelope, and frames from one envelope to the next, per step of
/// the rate: 1 under 50 kHz, 2 under 100 kHz, else 4. So both are the same in time.
const ORDER: usize = 50;
const DESIGN_FRAMES: usize = 128;
/// How many envelopes are averaged.
const DESIGNS: usize = 10;
/// The part of a period past the last one that is faded into its start, so the period loops
/// without a step.
const LOOPED: f64 = 0.1;
/// The pole of the tilt an envelope is worked out without, in Hz, so it fits the formants and
/// not the fall of a voice to the highs.
const ANALYSIS_TILT_HZ: f64 = 140.0;
/// Power added at every frequency of an envelope, -20 dB of the whole. A period of a clean
/// tone has nothing between its harmonics, and an envelope that fits that exactly puts the
/// sound back far louder when the period changes under it.
const FLOOR: f64 = 1e-2;

/// How to shift a part of the sound, from what a pitch tracker read: pushed with each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shift {
    /// The period of the sound in frames, when it has one.
    pub period: Option<f64>,
    /// How much faster than it came to play that part of the sound: 2 is an octave up.
    pub ratio: f64,
    /// How many frames before the frame pushed with it the reading describes.
    pub age: f64,
}

impl Shift {
    /// No period and no change: what a part of the sound without a reading plays as.
    pub const NONE: Self = Self {
        period: None,
        ratio: 1.0,
        age: 0.0,
    };
}

/// Shifts the pitch of a stereo sound, keeping its length. Everything it needs is made in
/// [`Self::new`], so [`Self::push`] and [`Self::pull`] may run on the audio thread.
pub struct Shifter {
    latency: f64,
    longest: f64,
    keep_step: f32,
    /// The next frame pushed.
    written: i64,
    sound: Ring,
    /// The sound with its envelope taken out.
    flat: Ring,
    /// The period a jump takes and the ratio of each frame, and the frames before which every
    /// frame has them.
    readings: Box<[(f32, f32)]>,
    noted: i64,
    /// Where the head reads, in frames pushed, and the head it fades from.
    head: f64,
    fading_from: f64,
    /// How far the fade is, 0 to 1, and how far it goes per frame.
    fade: f64,
    fade_step: f64,
    /// The period while the readings have one or it holds, 0 after; the frames it still holds
    /// for; and the last period heard.
    held: f64,
    holding: f64,
    last: f64,
    tilt: Tilt,
    formants: Formants,
    /// Whether formants are kept, how far a change of it is, 0 to 1, and how many frames the
    /// flat ring has been filled for.
    keeping: bool,
    keep: f32,
    filled: usize,
    loudness: Loudness,
}

impl Shifter {
    /// A shifter for a sound at `rate` whose periods are at most `longest` frames, with
    /// readings at most `oldest` frames old.
    pub fn new(rate: f64, longest: f64, oldest: f64) -> Self {
        LazyLock::force(&TABLE);
        LazyLock::force(&CURVE);
        let longest = longest.max(SHORTEST_PERIOD);
        // How far a head may go past its window in a fade, and the widest taps of a read.
        let travel = FADE * longest;
        let reach = (HALF as f64 * stretch(RATIOS.1)).ceil();
        // The head is where it should be once a reading of a period describes it, and every
        // head reads only frames that came in.
        let latency = (oldest + longest * WINDOW)
            .max(longest * WINDOW + travel + reach + 1.0)
            .ceil();
        let length = (latency + longest * WINDOW + travel + 2.0 * reach) as usize + 2;
        let tilt = (-2.0 * PI * TILT_HZ / rate).exp() as f32;
        Self {
            latency,
            longest,
            keep_step: (1.0 / (KEEP_SECONDS * rate).max(1.0)) as f32,
            written: 0,
            sound: Ring::new(length),
            flat: Ring::new(length),
            readings: vec![(0.0, 1.0); length].into(),
            noted: 0,
            head: -latency,
            fading_from: -latency,
            fade: 1.0,
            fade_step: 0.0,
            held: 0.0,
            holding: 0.0,
            last: (rate / UNHEARD_HZ).min(longest),
            tilt: Tilt::new(-tilt),
            formants: Formants::new(rate, longest, length, tilt),
            keeping: false,
            keep: 0.0,
            filled: 0,
            loudness: Loudness::new(rate),
        }
    }

    /// How many frames late the sound comes out, on average: the head stays within its window
    /// around it.
    pub fn latency(&self) -> usize {
        self.latency as usize
    }

    /// Whether to keep the formants. A change fades in, once the flat ring is full.
    pub fn keep_formants(&mut self, keep: bool) {
        if keep && !self.keeping {
            self.filled = 0;
        }
        self.keeping = keep;
    }

    /// Hears the next frame, with what the tracker read last. What is not a number is taken as
    /// silence and no shift, so it cannot stay in the rings or the heads.
    pub fn push(&mut self, frame: [f32; 2], shift: Shift) {
        let frame = frame.map(|sample| if sample.is_finite() { sample } else { 0.0 });
        let ratio = match shift.ratio.is_finite() {
            true => shift.ratio.clamp(RATIOS.0, RATIOS.1),
            false => 1.0,
        };
        let period = (shift.period)
            .filter(|period| period.is_finite() && *period > 0.0)
            .map(|period| period.clamp(SHORTEST_PERIOD, self.longest));
        match period {
            Some(period) => {
                (self.held, self.last) = (period, period);
                self.holding = HOLD_PERIODS * period;
            }
            None => {
                self.holding -= 1.0;
                if self.holding <= 0.0 {
                    self.held = 0.0;
                }
            }
        }
        let heard = (self.held > 0.0).then_some(self.held);
        self.sound.write(self.written, self.tilt.take_out(frame));
        if self.keeping || self.keep > 0.0 {
            let flat = self.formants.flatten(self.written, frame, heard, self.last);
            self.flat.write(self.written, flat);
            self.filled = self.filled.saturating_add(1);
        }
        let length = self.readings.len() as f64;
        let age = match shift.age.is_finite() {
            true => shift.age.clamp(0.0, length),
            false => 0.0,
        };
        let described = self.written - age.round() as i64;
        let noted = (heard.unwrap_or(self.last) as f32, ratio as f32);
        let length = self.readings.len() as i64;
        let from = match described >= self.noted {
            true => self.noted.max(described - length + 1),
            // A reading older than the last: only its own frame, while the ring holds it.
            false if described > self.written - length => described,
            false => described + 1,
        };
        for frame in from..=described {
            if let Some(slot) = self.readings.get_mut(frame.rem_euclid(length) as usize) {
                *slot = noted;
            }
        }
        self.noted = self.noted.max(described + 1);
        self.written += 1;
    }

    /// The next frame out.
    pub fn pull(&mut self) -> [f32; 2] {
        let (period, ratio) = self.reading_at(self.head);
        if self.fade >= 1.0 {
            self.jump(period);
        }
        let ready = self.filled >= self.flat.length;
        let target = if self.keeping && ready { 1.0 } else { 0.0 };
        self.keep = match self.keep < target {
            true => (self.keep + self.keep_step).min(target),
            false => (self.keep - self.keep_step).max(target),
        };
        let weight = self.fade as f32;
        let both = |ring: &Ring| {
            let now = ring.read(self.head, ratio);
            match weight >= 1.0 {
                true => now,
                false => between(ring.read(self.fading_from, ratio), now, weight),
            }
        };
        let mut out = self.tilt.put_back(both(&self.sound));
        if self.keep > 0.0 {
            let flat = both(&self.flat);
            let kept = self.formants.restore(self.head.floor() as i64, flat);
            let gain = self.loudness.gain(out, kept);
            out = between(out, kept.map(|sample| gain * sample), self.keep);
        }
        self.head += ratio;
        self.fading_from += ratio;
        self.fade = (self.fade + self.fade_step).min(1.0);
        out
    }

    /// The period and the ratio of the frame at `place`, from its reading or else the newest.
    fn reading_at(&self, place: f64) -> (f64, f64) {
        if self.noted == 0 {
            return (self.last, 1.0);
        }
        let frame = (place.floor() as i64).min(self.noted - 1);
        let at = frame.rem_euclid(self.readings.len() as i64) as usize;
        let (period, ratio) = (self.readings.get(at).copied()).unwrap_or((self.last as f32, 1.0));
        (f64::from(period), f64::from(ratio))
    }

    /// Jumps the head a period when it is too far from where it should be, and starts the fade.
    fn jump(&mut self, period: f64) {
        let should = (self.written - 1) as f64 - self.latency;
        let off = self.head - should;
        // Behind, its ratio would take it further behind: a period ahead. Ahead, a period back.
        let to = match off {
            off if off < -WINDOW * period => self.head + period,
            off if off > WINDOW * period => self.head - period,
            _ => return,
        };
        self.fading_from = self.head;
        self.head = to;
        self.fade = 0.0;
        self.fade_step = 1.0 / (FADE * period).max(1.0);
    }
}

/// `from` towards `to` by `weight`, 0 to 1.
fn between(from: [f32; 2], to: [f32; 2], weight: f32) -> [f32; 2] {
    [
        from[0] + weight * (to[0] - from[0]),
        from[1] + weight * (to[1] - from[1]),
    ]
}

/// The level of the sound shifted with its formants and of the sound with them kept: when the
/// envelope put back meets a period it was not worked out from, as where the pitch jumps, the
/// sound kept could ring far louder.
struct Loudness {
    fall: f32,
    shifted: f32,
    kept: f32,
}

impl Loudness {
    fn new(rate: f64) -> Self {
        Self {
            fall: (-1.0 / (LEVEL_SECONDS * rate)).exp() as f32,
            shifted: 0.0,
            kept: 0.0,
        }
    }

    /// The gain that holds `kept` to [`LOUDER`] times the level of `shifted`.
    fn gain(&mut self, shifted: [f32; 2], kept: [f32; 2]) -> f32 {
        let peak = |frame: [f32; 2]| frame[0].abs().max(frame[1].abs());
        self.shifted = peak(shifted).max(self.fall * self.shifted);
        self.kept = peak(kept).max(self.fall * self.kept);
        match self.kept > LOUDER * self.shifted {
            true => LOUDER * self.shifted / self.kept,
            false => 1.0,
        }
    }
}

/// The steps of the rate the envelope counts in: 1 under 50 kHz, 2 under 100 kHz, else 4.
fn rate_steps(rate: f64) -> usize {
    match rate {
        rate if rate < 50_000.0 => 1,
        rate if rate < 100_000.0 => 2,
        _ => 4,
    }
}

/// A first-order tilt of each channel, taken out as `x[n] - factor x[n-1]` and put back as
/// `y[n] = x[n] + factor y[n-1]`.
struct Tilt {
    factor: f32,
    before: [f32; 2],
    after: [f32; 2],
}

impl Tilt {
    fn new(factor: f32) -> Self {
        Self {
            factor,
            before: [0.0; 2],
            after: [0.0; 2],
        }
    }

    fn take_out(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let out = [
            frame[0] - self.factor * self.before[0],
            frame[1] - self.factor * self.before[1],
        ];
        self.before = frame;
        out
    }

    fn put_back(&mut self, frame: [f32; 2]) -> [f32; 2] {
        self.after = [
            frame[0] + self.factor * self.after[0],
            frame[1] + self.factor * self.after[1],
        ];
        self.after
    }
}

/// Frames of stereo sound, each written twice, so the taps of any place are one slice.
struct Ring {
    length: usize,
    frames: Box<[[f32; 2]]>,
}

impl Ring {
    fn new(length: usize) -> Self {
        Self {
            length,
            frames: vec![[0.0; 2]; 2 * length].into(),
        }
    }

    fn write(&mut self, frame: i64, value: [f32; 2]) {
        let at = frame.rem_euclid(self.length as i64) as usize;
        for at in [at, at + self.length] {
            if let Some(slot) = self.frames.get_mut(at) {
                *slot = value;
            }
        }
    }

    /// The sound at `place`, between its frames by the windowed sinc, stretched to cut under
    /// what a head at `ratio` would fold.
    fn read(&self, place: f64, ratio: f64) -> [f32; 2] {
        let whole = place.floor();
        if ratio <= 1.0 {
            let first = whole as i64 - HALF as i64 + 1;
            let start = first.rem_euclid(self.length as i64) as usize;
            let frames = self.frames.get(start..).unwrap_or_default();
            return TABLE.apply(frames, 0, place - whole);
        }
        let stretch = stretch(ratio);
        let reach = (HALF as f64 * stretch).ceil() as i64;
        let first = whole as i64 - reach + 1;
        let start = first.rem_euclid(self.length as i64) as usize;
        let Some(frames) = self.frames.get(start..start + 2 * reach as usize) else {
            return [0.0; 2];
        };
        let (inverse, past) = ((1.0 / stretch) as f32, (place - whole) as f32);
        let (mut sum, mut weights) = ([0.0_f32; 2], 0.0_f32);
        for (tap, frame) in frames.iter().enumerate() {
            let weight = kernel((tap as i64 - reach + 1) as f32 - past, inverse);
            sum = [sum[0] + weight * frame[0], sum[1] + weight * frame[1]];
            weights += weight;
        }
        // A steady level comes out at that level.
        match weights > 0.0 {
            true => sum.map(|sample| sample / weights),
            false => [0.0; 2],
        }
    }
}

/// How far the sinc is stretched for a head at `ratio` above 1: by the ratio over [`CUTOFF`],
/// eased in over the first [`EASE`] above 1, so a ratio that crosses 1 does not change the
/// sound at once.
fn stretch(ratio: f64) -> f64 {
    let part = ((ratio - 1.0) / EASE).clamp(0.0, 1.0);
    ratio / (1.0 - (1.0 - CUTOFF) * part)
}

/// The stretched sinc at `distance` frames, `inverse` the inverse of its stretch.
#[inline]
fn kernel(distance: f32, inverse: f32) -> f32 {
    let place = (distance * inverse + HALF as f32) * PHASES as f32;
    // Before the kernel, and a place that is not a number, weigh nothing.
    if place.is_nan() || place <= 0.0 {
        return 0.0;
    }
    let index = place as usize;
    let between = place - index as f32;
    match (CURVE.get(index), CURVE.get(index + 1)) {
        (Some(lower), Some(upper)) => lower + between * (upper - lower),
        _ => 0.0,
    }
}

/// The envelope of the sound: what takes it out before the ring and puts it back after the
/// head.
struct Formants {
    order: usize,
    hop: usize,
    analysis_tilt: f64,
    /// The last frames of the sound in one channel, and where the next goes.
    heard: Box<[f32]>,
    next: usize,
    /// The period the envelope is worked out from, looped and tilted, with room for the lags.
    looped: Box<[f64]>,
    /// The power of each lag of the last [`DESIGNS`] envelopes, their sum, which of them goes
    /// next, and how many there are.
    designs: Box<[f64]>,
    sum: Box<[f64]>,
    oldest: usize,
    count: usize,
    /// The reflection factors of each envelope, by its number: envelope `n` ends before frame
    /// `n * hop`. Then the factors at one frame, and the predictions Levinson and Durbin work
    /// with.
    analyses: Box<[f32]>,
    factors: Box<[f32]>,
    prediction: Box<[f64]>,
    before: Box<[f64]>,
    tilt: Tilt,
    /// The backward sound of each stage of the lattice of each channel, on the way in and on
    /// the way out.
    flatten: [Box<[f32]>; 2],
    restore: [Box<[f32]>; 2],
}

impl Formants {
    fn new(rate: f64, longest: f64, length: usize, tilt: f32) -> Self {
        let steps = rate_steps(rate);
        let order = ORDER * steps;
        let hop = DESIGN_FRAMES * steps;
        let heard = (longest * (1.0 + LOOPED)).ceil() as usize + 2;
        Self {
            order,
            hop,
            analysis_tilt: (-2.0 * PI * ANALYSIS_TILT_HZ / rate).exp(),
            heard: vec![0.0; heard].into(),
            next: 0,
            looped: vec![0.0; heard + order].into(),
            designs: vec![0.0; DESIGNS * (order + 1)].into(),
            sum: vec![0.0; order + 1].into(),
            oldest: 0,
            count: 0,
            analyses: vec![0.0; (length / hop + 4) * order].into(),
            factors: vec![0.0; order].into(),
            prediction: vec![0.0; order + 1].into(),
            before: vec![0.0; order + 1].into(),
            tilt: Tilt::new(tilt),
            flatten: [vec![0.0; order].into(), vec![0.0; order].into()],
            restore: [vec![0.0; order].into(), vec![0.0; order].into()],
        }
    }

    /// The reflection factors at `frame` into `self.factors`: from the envelope before it
    /// towards the last one.
    fn factors_at(&mut self, frame: i64) {
        let hop = self.hop as i64;
        let number = frame.div_euclid(hop);
        let along = frame.rem_euclid(hop) as f32 / hop as f32;
        let count = (self.analyses.len() / self.order) as i64;
        let at = |number: i64| number.rem_euclid(count) as usize * self.order;
        let (before, last) = (at(number - 1), at(number));
        let before = self.analyses.get(before..before + self.order);
        let last = self.analyses.get(last..last + self.order);
        let (Some(before), Some(last)) = (before, last) else {
            return;
        };
        for ((factor, before), last) in self.factors.iter_mut().zip(before).zip(last) {
            *factor = before + along * (last - before);
        }
    }

    /// Frame `frame` with its envelope taken out. It also hears the frame for the next
    /// envelope, which is worked out from `heard`, the period of the voice now, or else from
    /// `last`, the last one heard.
    fn flatten(&mut self, frame: i64, sound: [f32; 2], heard: Option<f64>, last: f64) -> [f32; 2] {
        if let Some(slot) = self.heard.get_mut(self.next) {
            *slot = 0.5 * (sound[0] + sound[1]);
        }
        self.next = (self.next + 1) % self.heard.len();
        if (frame + 1).rem_euclid(self.hop as i64) == 0 {
            self.design((frame + 1) / self.hop as i64, heard.unwrap_or(last), last);
        }
        self.factors_at(frame);
        let mut out = self.tilt.take_out(sound);
        for (sample, state) in out.iter_mut().zip(&mut self.flatten) {
            *sample = flatten(&self.factors, state, *sample);
        }
        out
    }

    /// The flat sound read at `frame` with the envelope of that frame put back.
    fn restore(&mut self, frame: i64, flat: [f32; 2]) -> [f32; 2] {
        self.factors_at(frame);
        let mut out = flat;
        for (sample, state) in out.iter_mut().zip(&mut self.restore) {
            *sample = restore(&self.factors, state, *sample);
        }
        self.tilt.put_back(out)
    }

    /// Envelope `number`: the newest `period` of the sound, its next part faded into its start
    /// so it loops, tilted and correlated with itself around the loop, each lag weighed by how
    /// the harmonics `last` apart blur together, averaged with the envelopes before it and
    /// turned into reflection factors by Levinson and Durbin.
    fn design(&mut self, number: i64, period: f64, last: f64) {
        let size = self.heard.len();
        let looped = ((LOOPED * period) as usize).min(size / 2);
        let length = (period.round() as usize).clamp(2, size - looped);
        // Newest first.
        let newest = (0..length + looped).map(|back| (self.next + size - 1 - back) % size);
        for (slot, at) in self.looped.iter_mut().zip(newest) {
            *slot = f64::from(self.heard.get(at).copied().unwrap_or(0.0));
        }
        for at in 0..looped {
            let weight = 1.0 - at as f64 / looped as f64;
            let earlier = self.looped.get(at + length).copied().unwrap_or(0.0);
            if let Some(slot) = self.looped.get_mut(at) {
                *slot += weight * (earlier - *slot);
            }
        }
        for at in length..length + self.order {
            let wrapped = self.looped.get(at % length).copied().unwrap_or(0.0);
            if let Some(slot) = self.looped.get_mut(at) {
                *slot = wrapped;
            }
        }
        let mut previous = self.looped.get(length - 1).copied().unwrap_or(0.0);
        for slot in self.looped.iter_mut().take(length + self.order) {
            (*slot, previous) = (*slot - self.analysis_tilt * previous, *slot);
        }
        let row = self.oldest * (self.order + 1);
        let Some(lags) = self.designs.get_mut(row..row + self.order + 1) else {
            return;
        };
        for ((lag, power), sum) in lags.iter_mut().enumerate().zip(&mut self.sum) {
            *sum -= *power;
            let pairs = self.looped.iter().take(length);
            let product: f64 = (pairs.zip(self.looped.iter().skip(lag)))
                .map(|(early, late)| early * late)
                .sum();
            let blur = match lag {
                0 => 1.0,
                _ => {
                    let angle = PI * lag as f64 / last;
                    (angle.sin() / angle).powi(2)
                }
            };
            *power = product * blur;
            *sum += *power;
        }
        self.oldest = (self.oldest + 1) % DESIGNS;
        self.count = (self.count + 1).min(DESIGNS);
        self.levinson(number);
    }

    /// The reflection factors of the averaged lags, as envelope `number`.
    fn levinson(&mut self, number: i64) {
        let count = (self.analyses.len() / self.order) as i64;
        let row = number.rem_euclid(count) as usize * self.order;
        let Some(factors) = self.analyses.get_mut(row..row + self.order) else {
            return;
        };
        factors.fill(0.0);
        let lags = &self.sum;
        self.prediction.fill(0.0);
        if let Some(first) = self.prediction.first_mut() {
            *first = 1.0;
        }
        let mut error = lags.first().copied().unwrap_or(0.0) * (1.0 + FLOOR);
        for stage in 1..=self.order {
            let (Some(taps), Some(lagged)) = (self.prediction.get(..stage), lags.get(1..=stage))
            else {
                break;
            };
            if error <= 1e-12 {
                break;
            }
            let sum: f64 = (taps.iter().zip(lagged.iter().rev()))
                .map(|(tap, lag)| tap * lag)
                .sum();
            let factor = (-sum / error).clamp(-0.999, 0.999);
            self.before.copy_from_slice(&self.prediction);
            let mirrored = self.before.get(1..stage).unwrap_or_default();
            if let Some(now) = self.prediction.get_mut(1..stage) {
                for (now, mirrored) in now.iter_mut().zip(mirrored.iter().rev()) {
                    *now += factor * mirrored;
                }
            }
            if let (Some(tap), Some(slot)) =
                (self.prediction.get_mut(stage), factors.get_mut(stage - 1))
            {
                (*tap, *slot) = (factor, factor as f32);
            }
            error *= 1.0 - factor * factor;
        }
    }
}

/// One sample through the lattice that takes the envelope out. `state` holds the backward
/// sound of each stage at the frame before.
#[inline]
fn flatten(factors: &[f32], state: &mut [f32], sample: f32) -> f32 {
    let (mut forward, mut backward) = (sample, sample);
    for (factor, before) in factors.iter().zip(state.iter_mut()) {
        let older = std::mem::replace(before, backward);
        backward = older + factor * forward;
        forward += factor * older;
    }
    forward
}

/// One sample through the lattice that puts the envelope back, the inverse of [`flatten`]:
/// down the stages for the forward sound, then up them for the backward sound. Each stage
/// keeps its forward sound in `state` until the way up, which writes the backward sound.
#[inline]
fn restore(factors: &[f32], state: &mut [f32], sample: f32) -> f32 {
    let mut forward = sample;
    let mut forwards = [0.0_f32; 4 * ORDER];
    let stages = factors.iter().zip(state.iter()).zip(forwards.iter_mut());
    for ((factor, before), at) in stages.rev() {
        forward -= factor * before;
        *at = forward;
    }
    let mut backward = forward;
    let stages = factors.iter().zip(state.iter_mut()).zip(&forwards);
    for ((factor, before), at) in stages {
        let older = std::mem::replace(before, backward);
        backward = older + factor * at;
    }
    forward
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{Pitch, PitchSettings, PitchTracker};

    const RATE: f64 = 48_000.0;
    const VOICE: PitchSettings = PitchSettings {
        lowest: 80.0,
        highest: 1_000.0,
        tolerance: 0.5,
    };

    /// A bright tone at `hz` with the harmonics of `amplitudes`, while `on` says so.
    fn tone(seconds: f64, hz: f64, amplitudes: &[f64], on: impl Fn(f64) -> bool) -> Vec<f32> {
        (0..(seconds * RATE) as usize)
            .map(|frame| {
                let time = frame as f64 / RATE;
                let phase = std::f64::consts::TAU * hz * time;
                let sample: f64 = (amplitudes.iter().enumerate())
                    .map(|(index, amplitude)| amplitude * (phase * (index + 1) as f64).sin())
                    .sum();
                if on(time) { sample as f32 } else { 0.0 }
            })
            .collect()
    }

    /// `sound` through a shifter at `ratio`, its periods read by a tracker as a device would.
    fn shift(sound: &[f32], ratio: f64, keep: bool) -> (Vec<f32>, usize) {
        let mut tracker = PitchTracker::new(RATE, VOICE);
        let longest = RATE / VOICE.lowest;
        let mut shifter = Shifter::new(RATE, longest, 1.5 * longest + 120.0);
        shifter.keep_formants(keep);
        let output = sound
            .iter()
            .map(|sample| {
                tracker.push([*sample]);
                let period = match tracker.pitch() {
                    Pitch::Note(note) => Some(RATE / (440.0 * 2_f64.powf((note - 69.0) / 12.0))),
                    _ => None,
                };
                let ratio = if period.is_some() { ratio } else { 1.0 };
                let age = tracker.age();
                shifter.push([*sample; 2], Shift { period, ratio, age });
                shifter.pull()[0]
            })
            .collect();
        (output, shifter.latency())
    }

    /// The note the tracker reads most of the time over `sound`.
    fn note(sound: &[f32]) -> f64 {
        let mut tracker = PitchTracker::new(RATE, VOICE);
        let mut notes: Vec<f64> = (sound.chunks(240))
            .filter_map(|chunk| {
                tracker.push(chunk.iter().copied());
                match tracker.pitch() {
                    Pitch::Note(note) => Some(note),
                    _ => None,
                }
            })
            .collect();
        notes.sort_by(f64::total_cmp);
        notes[notes.len() / 2]
    }

    /// The frames from the start of each run of sound to where it is first loud.
    fn onsets(sound: &[f32]) -> Vec<usize> {
        let mut onsets = Vec::new();
        let mut quiet = usize::MAX;
        for (frame, sample) in sound.iter().enumerate() {
            if sample.abs() > 0.05 {
                if quiet > 2_000 {
                    onsets.push(frame);
                }
                quiet = 0;
            } else {
                quiet = quiet.saturating_add(1);
            }
        }
        onsets
    }

    #[test]
    fn with_no_shift_the_sound_comes_out_as_it_went_in_as_late_as_it_says() {
        let sound = tone(0.5, 440.0, &[0.5, 0.2], |_| true);
        let mut shifter = Shifter::new(RATE, 600.0, 1_000.0);
        let output: Vec<f32> = (sound.iter())
            .map(|sample| {
                shifter.push([*sample; 2], Shift::NONE);
                shifter.pull()[0]
            })
            .collect();
        let latency = shifter.latency();
        assert_eq!(output[..latency], vec![0.0; latency]);
        // The tilt around the head cancels to within rounding.
        for (out, sample) in output[latency..].iter().zip(&sound) {
            assert!((out - sample).abs() < 1e-5, "{out} for {sample}");
        }
    }

    #[test]
    fn a_head_that_reads_faster_folds_nothing_down() {
        // A voice at 220 Hz with air at 20 kHz, a fifth up: the air would land at 30 kHz, above
        // the Nyquist frequency, and fold down to 18 kHz.
        let voice = tone(1.0, 220.0, &[0.3, 0.1], |_| true);
        let air = tone(1.0, 20_000.0, &[0.1], |_| true);
        let sound: Vec<f32> = voice
            .iter()
            .zip(&air)
            .map(|(voice, air)| voice + air)
            .collect();
        let (output, _) = shift(&sound, 1.5, false);
        let steady = &output[(0.5 * RATE) as usize..];
        let folded = power_at(steady, 48_000.0 - 30_000.0) / power_at(&air, 20_000.0);
        assert!(
            10.0 * folded.log10() < -50.0,
            "folded at {} dB",
            10.0 * folded.log10()
        );
    }

    #[test]
    fn a_ratio_moves_the_pitch_by_it_and_keeps_where_each_note_starts() {
        // Notes of 300 ms with 300 ms between them.
        let on = |time: f64| time % 0.6 < 0.3;
        let sound = tone(2.4, 220.0, &[0.4, 0.2, 0.1, 0.05], on);
        for semitones in [-5.0, -1.0, 0.5, 3.0, 7.0] {
            let ratio = 2_f64.powf(semitones / 12.0);
            let (output, latency) = shift(&sound, ratio, false);
            let heard = note(&output[(0.1 * RATE) as usize + latency..(0.28 * RATE) as usize]);
            let expected = 57.0 + semitones;
            assert!(
                (heard - expected).abs() < 0.05,
                "{semitones}: {heard} for {expected}"
            );
            let (before, after) = (onsets(&sound), onsets(&output));
            assert_eq!(before.len(), after.len(), "{semitones}: {after:?}");
            // The head stays within its window of a period around the latency, also through a
            // rest, where nothing moves it back.
            let window = (WINDOW * RATE / 220.0).ceil() as i64;
            for (before, after) in before.iter().zip(&after) {
                let late = *after as i64 - (*before + latency) as i64;
                assert!(late.abs() <= window, "{semitones}: {late} frames off");
            }
        }
    }

    /// The power of `sound` at `hz`, as the mean square of the sine there.
    fn power_at(sound: &[f32], hz: f64) -> f64 {
        let (mut sine, mut cosine) = (0.0, 0.0);
        for (frame, sample) in sound.iter().enumerate() {
            let phase = std::f64::consts::TAU * hz * frame as f64 / RATE;
            sine += f64::from(*sample) * phase.sin();
            cosine += f64::from(*sample) * phase.cos();
        }
        let amplitude = 2.0 * sine.hypot(cosine) / sound.len() as f64;
        amplitude * amplitude / 2.0
    }

    /// The power of `sound` that is not at the first `count` harmonics of `hz`, against its
    /// whole power, in dB: each harmonic is fitted and taken out, twice over.
    fn rest_db(sound: &[f32], hz: f64, count: usize) -> f64 {
        let power = |sound: &[f64]| sound.iter().map(|sample| sample * sample).sum::<f64>();
        let mut rest: Vec<f64> = sound.iter().map(|sample| f64::from(*sample)).collect();
        let whole = power(&rest);
        for _ in 0..2 {
            for harmonic in 1..=count {
                let step = std::f64::consts::TAU * hz * harmonic as f64 / RATE;
                let (mut sine, mut cosine) = (0.0, 0.0);
                for (frame, sample) in rest.iter().enumerate() {
                    sine += sample * (step * frame as f64).sin();
                    cosine += sample * (step * frame as f64).cos();
                }
                let scale = 2.0 / rest.len() as f64;
                for (frame, sample) in rest.iter_mut().enumerate() {
                    let phase = step * frame as f64;
                    *sample -= scale * (sine * phase.sin() + cosine * phase.cos());
                }
            }
        }
        10.0 * (power(&rest) / whole).log10()
    }

    #[test]
    fn a_shifted_steady_tone_has_almost_all_its_power_at_its_new_harmonics() {
        let harmonics = [0.4, 0.2, 0.1, 0.05];
        let sound = tone(1.5, 220.0, &harmonics, |_| true);
        for semitones in [-5.0, -0.3, 2.0, 6.0] {
            let ratio = 2_f64.powf(semitones / 12.0);
            let (output, _) = shift(&sound, ratio, false);
            // Half a second, a whole number of cycles of the new tone.
            let hz = 220.0 * ratio;
            let cycles = (0.5 * hz).round();
            let steady = &output[(0.5 * RATE) as usize..][..(cycles / hz * RATE) as usize];
            let rest_db = rest_db(steady, hz, harmonics.len());
            assert!(
                rest_db < -60.0,
                "{semitones}: {rest_db} dB besides the harmonics"
            );
        }
    }

    /// A voice of pulses at `hz` through one resonance at `formant_hz`: a vowel of one formant.
    fn vowel(seconds: f64, hz: f64, formant_hz: f64) -> Vec<f32> {
        let radius = (-std::f64::consts::PI * 150.0 / RATE).exp();
        let angle = std::f64::consts::TAU * formant_hz / RATE;
        let (a1, a2) = (-2.0 * radius * angle.cos(), radius * radius);
        let (mut y1, mut y2) = (0.0, 0.0);
        let period = RATE / hz;
        let mut next = 0.0;
        (0..(seconds * RATE) as usize)
            .map(|frame| {
                let pulse = if frame as f64 >= next {
                    next += period;
                    1.0
                } else {
                    0.0
                };
                let y = 0.05 * pulse - a1 * y1 - a2 * y2;
                (y2, y1) = (y1, y);
                y as f32
            })
            .collect()
    }

    /// Where the power of `sound` peaks between 300 Hz and 3 kHz, from its harmonics at `hz`.
    fn formant(sound: &[f32], hz: f64) -> f64 {
        let harmonics = (1..).map(|harmonic| hz * harmonic as f64);
        let harmonics = harmonics
            .skip_while(|at| *at < 300.0)
            .take_while(|at| *at < 3_000.0);
        let loudest = harmonics.map(|at| (at, power_at(sound, at)));
        loudest
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0.0, |(at, _)| at)
    }

    #[test]
    fn kept_formants_stay_where_they_were_and_unkept_ones_move_with_the_pitch() {
        let sound = vowel(1.5, 200.0, 1_000.0);
        let ratio = 2_f64.powf(5.0 / 12.0);
        let hz = 200.0 * ratio;
        let before = formant(
            &sound[(0.5 * RATE) as usize..][..(0.5 * RATE) as usize],
            200.0,
        );
        let at = |keep| {
            let (output, _) = shift(&sound, ratio, keep);
            formant(
                &output[(0.5 * RATE) as usize..][..(0.5 * RATE) as usize],
                hz,
            )
        };
        let (moved, kept) = (at(false), at(true));
        assert!((moved / before - ratio).abs() < 0.1, "{moved}");
        assert!((kept / before - 1.0).abs() < 0.1, "{kept}");
    }

    #[test]
    fn a_ratio_that_crosses_1_keeps_the_highs() {
        // Air at 21 kHz, read at a ratio just under 1 and just over it.
        let air = tone(0.3, 21_000.0, &[0.2], |_| true);
        let level = |ratio: f64| {
            let mut shifter = Shifter::new(RATE, 600.0, 1_000.0);
            let shift = Shift {
                ratio,
                ..Shift::NONE
            };
            let output: Vec<f32> = (air.iter())
                .map(|sample| {
                    shifter.push([*sample; 2], shift);
                    shifter.pull()[0]
                })
                .collect();
            power_at(&output[(0.1 * RATE) as usize..], 21_000.0 * ratio)
        };
        let change = 10.0 * (level(1.000_001) / level(0.999_999)).log10();
        assert!(change.abs() < 0.1, "{change} dB");
    }

    #[test]
    fn a_sample_that_is_not_a_number_leaves_no_trace() {
        let sound = tone(1.0, 220.0, &[0.3, 0.1], |_| true);
        let mut shifter = Shifter::new(RATE, 600.0, 1_000.0);
        shifter.keep_formants(true);
        let mut output = Vec::new();
        for (frame, sample) in sound.iter().enumerate() {
            let (sample, ratio) = match frame == 1_000 {
                true => (f32::NAN, f64::NAN),
                false => (*sample, 1.1),
            };
            let shift = Shift {
                period: Some(RATE / 220.0),
                ratio,
                age: 0.0,
            };
            shifter.push([sample; 2], shift);
            output.push(shifter.pull()[0]);
        }
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(
            output[RATE as usize / 2..]
                .iter()
                .any(|sample| sample.abs() > 0.1)
        );
    }

    #[test]
    fn kept_formants_stay_at_the_level_of_the_sound_when_the_pitch_jumps() {
        // A bright tone that jumps between two notes every 125 ms, a third up and down: a clean
        // tone has no power between its harmonics, the hardest case for an envelope.
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            for (low, high) in [(110.0, 220.0), (100.0, 600.0)] {
                let mut shifter = Shifter::new(rate, rate / 60.0, 1.6 * rate / 60.0);
                shifter.keep_formants(true);
                let (mut phase, mut loudest) = (0.0_f64, 0.0_f32);
                let switch = (0.125 * rate) as usize;
                for frame in 0..(3.0 * rate) as usize {
                    let hz = if (frame / switch).is_multiple_of(2) {
                        low
                    } else {
                        high
                    };
                    phase += std::f64::consts::TAU * hz / rate;
                    let sample: f64 = (1..=16)
                        .map(|harmonic| 0.15 * (phase * harmonic as f64).sin() / harmonic as f64)
                        .sum();
                    let shift = Shift {
                        period: Some(rate / hz),
                        ratio: 1.03,
                        age: 0.0,
                    };
                    shifter.push([sample as f32; 2], shift);
                    let out = shifter.pull()[0];
                    if frame > rate as usize {
                        loudest = loudest.max(out.abs());
                    }
                }
                // The tone peaks at 0.26.
                assert!(loudest < 0.55, "{rate} Hz, {low} and {high} Hz: {loudest}");
            }
        }
    }
}
