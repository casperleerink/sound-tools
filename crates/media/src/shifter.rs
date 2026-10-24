//! Shifts the pitch of one voice and keeps its length: what pitch correction plays, and what a
//! region that plays faster or slower than its file can feed.
//!
//! The sound goes into a ring and a head reads it back at `ratio` times its speed, 2 an octave
//! up. A head that reads faster than the sound comes in would catch up with it, and a slower
//! one would fall behind. So when the head is more than [`WINDOW`] of a period from where it
//! should be, it jumps one period of the sound back or ahead, and fades over one period from
//! where it was. A jump of one period lands on the same part of the wave, so the fade joins two
//! nearly equal sounds: whole periods play twice or not at all, and the length stays. Without a
//! period, in a consonant, a breath or silence, the head plays on at its ratio and jumps back to
//! where it should be when it is more than [`SLACK_SECONDS`] off.
//!
//! Every frame comes with what a pitch tracker read, which describes the sound some frames
//! back ([`Shift::age`]). The shifter keeps each reading with the frame it describes, so a
//! head plays each part at the ratio and with the period of that part. The latency leaves the
//! head room for that and for its window.
//!
//! The head reads between frames with a windowed sinc. Reading faster would fold what lies
//! above the new Nyquist frequency down, so there the sinc is stretched by the ratio over
//! [`CUTOFF`], and cuts under it. Both are symmetric, so the sound read keeps its phase and
//! lines up with the sound as it came, for a mix of the two. A low pass in front of the ring
//! would not.
//!
//! Reading faster also moves the envelope of the spectrum up, the formants that make a voice
//! sound like that voice. With formants kept, a second ring holds the sound with its envelope
//! taken out by linear prediction, worked out every [`HOP_SECONDS`] from the last
//! [`ANALYSIS_SECONDS`], and after the head the envelope of the part it reads is put back. Both
//! filters are lattices of reflection factors under 1, so a factor read between two analyses
//! never makes them ring.

use std::f64::consts::PI;
use std::sync::LazyLock;

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
/// How far the head may be off without a period before it jumps back.
const SLACK_SECONDS: f64 = 0.0005;
/// The fade of a jump without a period.
const UNVOICED_FADE_SECONDS: f64 = 0.005;
/// How long a period is kept once the readings have none, as through a short consonant.
const HOLD_SECONDS: f64 = 0.05;
/// The shortest period a jump takes, in frames.
const SHORTEST_PERIOD: f64 = 16.0;
/// The ratios the head reads at.
const RATIOS: (f64, f64) = (0.5, 2.0);
/// Above a ratio of 1, where the stretched sinc cuts, as a part of the Nyquist frequency over
/// the ratio: its transition then ends under that frequency.
const CUTOFF: f64 = 0.9;
/// How long a change of formants takes.
const KEEP_SECONDS: f64 = 0.02;

/// Reflection factors of the envelope.
const ORDER: usize = 24;
const HOP_SECONDS: f64 = 0.005;
const ANALYSIS_SECONDS: f64 = 0.02;
/// The tilt taken out before the envelope and put back after it, so the envelope fits the
/// formants and not the fall of a voice to the highs.
const EMPHASIS: f32 = 0.95;
/// Added to the power of the analysis, -40 dB: a floor, so a pure tone does not make the
/// envelope a needle.
const FLOOR: f64 = 1e-4;
/// The bandwidth each peak of the envelope is widened to, in Hz.
const WIDENING_HZ: f64 = 80.0;

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
    slack: f64,
    unvoiced_fade: f64,
    hold_frames: usize,
    keep_step: f32,
    /// The next frame pushed.
    written: i64,
    sound: Ring,
    /// The sound with its envelope taken out.
    flat: Ring,
    /// The period and ratio of each frame, and the frames before which every frame has one.
    readings: Box<[(f32, f32)]>,
    noted: i64,
    /// Where the head reads, in frames pushed, and the head it fades from.
    head: f64,
    fading_from: f64,
    /// How far the fade is, 0 to 1, and how far it goes per frame.
    fade: f64,
    fade_step: f64,
    /// The last period, and for how many more frames it holds.
    held: f64,
    holding: usize,
    formants: Formants,
    /// Whether formants are kept, how far a change of it is, 0 to 1, and how many frames the
    /// flat ring has been filled for.
    keeping: bool,
    keep: f32,
    filled: usize,
}

impl Shifter {
    /// A shifter for a sound at `rate` whose periods are at most `longest` frames, with
    /// readings at most `oldest` frames old.
    pub fn new(rate: f64, longest: f64, oldest: f64) -> Self {
        LazyLock::force(&TABLE);
        LazyLock::force(&CURVE);
        let longest = longest.max(SHORTEST_PERIOD);
        // The head is where it should be once a reading of a period describes it, and the head it
        // fades from stays clear of the newest frame at a ratio of 2.
        let latency = (oldest + longest * WINDOW)
            .max(longest * (1.0 + WINDOW) + (HALF + 1) as f64)
            .ceil();
        // The head behind by its window and a fade at a ratio of a half, and its widest taps.
        let length = (latency + longest * 2.0) as usize + 8 * HALF;
        Self {
            latency,
            longest,
            slack: (SLACK_SECONDS * rate).max(1.0),
            unvoiced_fade: (UNVOICED_FADE_SECONDS * rate).max(1.0),
            hold_frames: (HOLD_SECONDS * rate) as usize,
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
            holding: 0,
            formants: Formants::new(rate, length),
            keeping: false,
            keep: 0.0,
            filled: 0,
        }
    }

    /// How many frames late the sound comes out, on average: the head stays within its window
    /// around it, and jumps to it after a part without a period.
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

    /// Hears the next frame, with what the tracker read last.
    pub fn push(&mut self, frame: [f32; 2], shift: Shift) {
        let ratio = shift.ratio.clamp(RATIOS.0, RATIOS.1);
        self.sound.write(self.written, frame);
        if self.keeping || self.keep > 0.0 {
            let flat = self.formants.flatten(self.written, frame);
            self.flat.write(self.written, flat);
            self.filled = self.filled.saturating_add(1);
        }
        let described = self.written - shift.age.round() as i64;
        let noted = (shift.period.unwrap_or(0.0) as f32, ratio as f32);
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
        let period = match period {
            Some(period) => {
                (self.held, self.holding) = (period, self.hold_frames);
                Some(period)
            }
            None if self.holding > 0 => {
                self.holding -= 1;
                Some(self.held)
            }
            None => None,
        };
        if self.fade >= 1.0 {
            self.jump(period, ratio);
        }
        let ready = self.filled >= self.flat.length;
        let target = if self.keeping && ready { 1.0 } else { 0.0 };
        self.keep = match self.keep < target {
            true => (self.keep + self.keep_step).min(target),
            false => (self.keep - self.keep_step).max(target),
        };
        let weight = match self.fade < 1.0 {
            true => (0.5 - 0.5 * (PI * self.fade).cos()) as f32,
            false => 1.0,
        };
        let both = |ring: &Ring| {
            let now = ring.read(self.head, ratio);
            match weight >= 1.0 {
                true => now,
                false => between(ring.read(self.fading_from, ratio), now, weight),
            }
        };
        let mut out = both(&self.sound);
        if self.keep > 0.0 {
            let flat = both(&self.flat);
            let kept = self.formants.restore(self.head.floor() as i64, flat);
            out = between(out, kept, self.keep);
        }
        self.head += ratio;
        self.fading_from += ratio;
        self.fade = (self.fade + self.fade_step).min(1.0);
        out
    }

    /// The period and the ratio of the frame at `place`, from its reading or else the newest.
    fn reading_at(&self, place: f64) -> (Option<f64>, f64) {
        if self.noted == 0 {
            return (None, 1.0);
        }
        let frame = (place.floor() as i64).min(self.noted - 1);
        let at = frame.rem_euclid(self.readings.len() as i64) as usize;
        let (period, ratio) = self.readings.get(at).copied().unwrap_or((0.0, 1.0));
        let period = (period > 0.0).then(|| f64::from(period).clamp(SHORTEST_PERIOD, self.longest));
        (period, f64::from(ratio))
    }

    /// Jumps the head when it is too far from where it should be, and starts the fade.
    fn jump(&mut self, period: Option<f64>, ratio: f64) {
        let should = (self.written - 1) as f64 - self.latency;
        let off = self.head - should;
        let (to, fade) = match period {
            // Behind, its ratio would take it further behind: a period ahead. Ahead, a period back.
            Some(period) if off < -WINDOW * period => (self.head + period, period),
            Some(period) if off > WINDOW * period => (self.head - period, period),
            Some(_) => return,
            None if off.abs() > self.slack => (should, self.unvoiced_fade),
            None => return,
        };
        // A fade that ends before the head could jump again.
        let fade = fade.min(0.9 * fade / (ratio - 1.0).abs().max(1e-9));
        self.fading_from = self.head;
        self.head = to;
        self.fade = 0.0;
        self.fade_step = 1.0 / fade.max(1.0);
    }
}

/// `from` towards `to` by `weight`, 0 to 1.
fn between(from: [f32; 2], to: [f32; 2], weight: f32) -> [f32; 2] {
    [
        from[0] + weight * (to[0] - from[0]),
        from[1] + weight * (to[1] - from[1]),
    ]
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
        let stretch = ratio / CUTOFF;
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
    hop: usize,
    /// The window of an analysis, the last frames of the sound in one channel, tilted, and
    /// those frames in order under the window.
    window: Box<[f32]>,
    heard: Box<[f32]>,
    windowed: Box<[f64]>,
    next: usize,
    tilted: f32,
    /// The reflection factors of each analysis, by its number: analysis `n` ends before frame
    /// `n * hop`.
    analyses: Box<[[f32; ORDER]]>,
    /// The widening of each lag of the analysis.
    widening: [f64; ORDER + 1],
    /// The tilt and the lattice of each channel, on the way in and on the way out.
    tilt_in: [f32; 2],
    flatten: [[f32; ORDER]; 2],
    restore: [[f32; ORDER]; 2],
    tilt_out: [f32; 2],
}

impl Formants {
    fn new(rate: f64, length: usize) -> Self {
        let hop = ((HOP_SECONDS * rate).round() as usize).max(1);
        let size = ((ANALYSIS_SECONDS * rate).round() as usize).max(ORDER + 1);
        let window = (0..size).map(|frame| {
            let place = (frame as f64 + 0.5) / size as f64;
            (0.5 - 0.5 * (2.0 * PI * place).cos()) as f32
        });
        let widening = std::array::from_fn(|lag| {
            let spread = 2.0 * PI * WIDENING_HZ * lag as f64 / rate;
            (-0.5 * spread * spread).exp()
        });
        Self {
            hop,
            window: window.collect(),
            heard: vec![0.0; size].into(),
            windowed: vec![0.0; size].into(),
            next: 0,
            tilted: 0.0,
            analyses: vec![[0.0; ORDER]; length / hop + 4].into(),
            widening,
            tilt_in: [0.0; 2],
            flatten: [[0.0; ORDER]; 2],
            restore: [[0.0; ORDER]; 2],
            tilt_out: [0.0; 2],
        }
    }

    /// The reflection factors at `frame`: from the analysis before it towards the last one.
    fn factors(&self, frame: i64) -> [f32; ORDER] {
        let hop = self.hop as i64;
        let number = frame.div_euclid(hop);
        let along = frame.rem_euclid(hop) as f32 / hop as f32;
        let length = self.analyses.len() as i64;
        let at = |number: i64| {
            let at = number.rem_euclid(length) as usize;
            self.analyses.get(at).copied().unwrap_or([0.0; ORDER])
        };
        let (mut factors, last) = (at(number - 1), at(number));
        for (factor, last) in factors.iter_mut().zip(last) {
            *factor += along * (last - *factor);
        }
        factors
    }

    /// Frame `frame` with its envelope taken out. It also hears the frame for the next
    /// analysis.
    fn flatten(&mut self, frame: i64, sound: [f32; 2]) -> [f32; 2] {
        let mono = 0.5 * (sound[0] + sound[1]);
        if let Some(slot) = self.heard.get_mut(self.next) {
            *slot = mono - EMPHASIS * self.tilted;
        }
        self.tilted = mono;
        self.next = (self.next + 1) % self.heard.len();
        if (frame + 1).rem_euclid(self.hop as i64) == 0 {
            self.analyse((frame + 1) / self.hop as i64);
        }
        let factors = self.factors(frame);
        let mut out = sound;
        let lattices = self.tilt_in.iter_mut().zip(&mut self.flatten);
        for (sample, (tilt, state)) in out.iter_mut().zip(lattices) {
            let tilted = *sample - EMPHASIS * *tilt;
            *tilt = *sample;
            *sample = flatten(&factors, state, tilted);
        }
        out
    }

    /// The flat sound read at `frame` with the envelope of that frame put back.
    fn restore(&mut self, frame: i64, flat: [f32; 2]) -> [f32; 2] {
        let factors = self.factors(frame);
        let mut out = flat;
        let lattices = self.tilt_out.iter_mut().zip(&mut self.restore);
        for (sample, (tilt, state)) in out.iter_mut().zip(lattices) {
            *sample = restore(&factors, state, *sample) + EMPHASIS * *tilt;
            *tilt = *sample;
        }
        out
    }

    /// The reflection factors of the last frames heard, by Levinson and Durbin, as analysis
    /// `number`.
    fn analyse(&mut self, number: i64) {
        let (newer, older) = self.heard.split_at(self.next.min(self.heard.len()));
        let heard = older.iter().chain(newer.iter());
        for ((windowed, sample), window) in self.windowed.iter_mut().zip(heard).zip(&self.window) {
            *windowed = f64::from(sample * window);
        }
        let mut lags = [0.0_f64; ORDER + 1];
        for (lag, (sum, widening)) in lags.iter_mut().zip(&self.widening).enumerate() {
            let pairs = self.windowed.iter().zip(self.windowed.iter().skip(lag));
            *sum = widening * pairs.map(|(early, late)| early * late).sum::<f64>();
        }
        lags[0] *= 1.0 + FLOOR;
        let length = self.analyses.len() as i64;
        let Some(factors) = self.analyses.get_mut(number.rem_euclid(length) as usize) else {
            return;
        };
        *factors = [0.0; ORDER];
        let mut prediction = [0.0_f64; ORDER + 1];
        prediction[0] = 1.0;
        let mut error = lags[0];
        for stage in 1..=ORDER {
            let (Some(taps), Some(lagged)) = (prediction.get(..stage), lags.get(1..=stage)) else {
                break;
            };
            if error <= 1e-12 {
                break;
            }
            let sum: f64 = (taps.iter().zip(lagged.iter().rev()))
                .map(|(tap, lag)| tap * lag)
                .sum();
            let factor = (-sum / error).clamp(-0.999, 0.999);
            let before = prediction;
            if let (Some(now), Some(mirrored)) =
                (prediction.get_mut(1..stage), before.get(1..stage))
            {
                for (now, mirrored) in now.iter_mut().zip(mirrored.iter().rev()) {
                    *now += factor * mirrored;
                }
            }
            if let (Some(tap), Some(slot)) = (prediction.get_mut(stage), factors.get_mut(stage - 1))
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
fn flatten(factors: &[f32; ORDER], state: &mut [f32; ORDER], sample: f32) -> f32 {
    let (mut forward, mut backward) = (sample, sample);
    for (factor, before) in factors.iter().zip(state.iter_mut()) {
        let older = std::mem::replace(before, backward);
        backward = older + factor * forward;
        forward += factor * older;
    }
    forward
}

/// One sample through the lattice that puts the envelope back, the inverse of [`flatten`]:
/// down the stages for the forward sound, then up them for the backward sound.
#[inline]
fn restore(factors: &[f32; ORDER], state: &mut [f32; ORDER], sample: f32) -> f32 {
    let mut forwards = [0.0; ORDER];
    let mut forward = sample;
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
        assert_eq!(output[latency..], sound[..sound.len() - latency]);
    }

    #[test]
    fn a_head_that_reads_faster_folds_nothing_down() {
        // A voice at 220 Hz with air at 18 kHz, a fifth up: the air would land at 27 kHz, above
        // the Nyquist frequency, and fold down to 21 kHz.
        let voice = tone(1.0, 220.0, &[0.3, 0.1], |_| true);
        let air = tone(1.0, 18_000.0, &[0.1], |_| true);
        let sound: Vec<f32> = voice.iter().zip(&air).map(|(voice, air)| voice + air).collect();
        let (output, _) = shift(&sound, 1.5, false);
        let steady = &output[(0.5 * RATE) as usize..];
        let folded = power_at(steady, 48_000.0 - 27_000.0) / power_at(&air, 18_000.0);
        assert!(10.0 * folded.log10() < -50.0, "folded at {} dB", 10.0 * folded.log10());
    }

    #[test]
    fn a_ratio_moves_the_pitch_by_it_and_keeps_where_each_note_starts() {
        // Notes of 300 ms with 300 ms between them.
        let on = |time: f64| time % 0.6 < 0.3;
        let sound = tone(2.4, 220.0, &[0.4, 0.2, 0.1, 0.05], on);
        for semitones in [-5.0, -1.0, 0.5, 3.0, 7.0] {
            let ratio = 2_f64.powf(semitones / 12.0);
            let (output, latency) = shift(&sound, ratio, false);
            assert_eq!(output.len(), sound.len());
            let heard = note(&output[(0.1 * RATE) as usize + latency..(0.28 * RATE) as usize]);
            let expected = 57.0 + semitones;
            assert!(
                (heard - expected).abs() < 0.05,
                "{semitones}: {heard} for {expected}"
            );
            let (before, after) = (onsets(&sound), onsets(&output));
            assert_eq!(before.len(), after.len(), "{semitones}: {after:?}");
            for (before, after) in before.iter().zip(&after) {
                let late = *after as i64 - (*before + latency) as i64;
                assert!(late.abs() < 48, "{semitones}: {late} frames off");
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
}
