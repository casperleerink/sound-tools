//! The musical clock: ticks, tempo, time signature, the tempo map and its conversions.
//!
//! [`TempoMap`] is the saved form and knows nothing about sample rates. [`Clock`] is a tempo
//! map compiled for one sample rate. It is the only place where ticks become frames, so every
//! part of the application gets the same frame for the same tick.
//!
//! How the rounding works: a tick lands on the frame that contains its exact time (the exact
//! position rounded down). Every tempo change starts a segment at the frame of its own tick, so
//! all math inside a segment is integer math from an integer start. `frame_of` never goes down
//! when the tick goes up, and `tick_at` is its inverse: the first tick at or after a frame.

use std::fmt;
use std::ops::Add;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

/// Musical time resolution. One quarter note is 960 ticks, whatever the time signature is.
pub const TICKS_PER_QUARTER: u64 = 960;

/// Below this sample rate a tick can be shorter than a frame at the fastest tempo, and two
/// ticks can share a frame. From this rate up, `tick_at(frame_of(tick)) == tick` always holds.
pub const MIN_EXACT_SAMPLE_RATE: u32 = 16_000;

/// A position or a length in musical time. Saved positions use this.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Ticks(pub u64);

impl Ticks {
    pub fn saturating_sub(self, other: Ticks) -> Ticks {
        Ticks(self.0.saturating_sub(other.0))
    }
}

/// Saturates, so a sum on the audio thread can never panic.
impl Add for Ticks {
    type Output = Ticks;

    fn add(self, other: Ticks) -> Ticks {
        Ticks(self.0.saturating_add(other.0))
    }
}

/// A position on the project timeline in audio frames. Not engine time: the project position
/// stands still while the transport is stopped.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frames(pub u64);

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ClockError {
    #[error("a tempo of {0} bpm is outside {min} to {max} bpm", min = Tempo::MIN_BPM, max = Tempo::MAX_BPM)]
    TempoOutOfRange(f64),
    #[error(
        "time signature {numerator}/{denominator} is not supported: the numerator must be 1 to 32 and the denominator 1, 2, 4, 8, 16 or 32"
    )]
    UnsupportedTimeSignature { numerator: u32, denominator: u32 },
    #[error("\"{0}\" is not a time signature, write it like \"4/4\"")]
    TimeSignatureSyntax(String),
    #[error("the first tempo change must be at tick 0")]
    NoTempoAtStart,
    #[error("tempo changes must be sorted by tick, each tick used once: see tick {}", .0.0)]
    TempoChangesNotSorted(Ticks),
    #[error("{position} is not a position in {time_signature}")]
    InvalidBarBeat {
        position: BarBeat,
        time_signature: TimeSignature,
    },
}

/// Beats per minute, where a beat is a quarter note. Held in steps of 0.001 bpm so all clock
/// math is integer math. Saved as a plain number, for example `120.0` or `93.5`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Tempo {
    milli_bpm: u32,
}

impl Tempo {
    pub const MIN_BPM: f64 = 10.0;
    /// At this tempo a tick still lasts a whole frame at [`MIN_EXACT_SAMPLE_RATE`].
    pub const MAX_BPM: f64 = 1000.0;

    /// Rounds to the nearest 0.001 bpm.
    pub fn from_bpm(bpm: f64) -> Result<Self, ClockError> {
        if !(Self::MIN_BPM..=Self::MAX_BPM).contains(&bpm) {
            return Err(ClockError::TempoOutOfRange(bpm));
        }
        Ok(Self {
            milli_bpm: (bpm * 1000.0).round() as u32,
        })
    }

    pub fn bpm(self) -> f64 {
        f64::from(self.milli_bpm) / 1000.0
    }

    /// The tempo in thousandths of a bpm, which is how it is held and how all clock math uses
    /// it. For code that has to work out a frame count exactly, as building a tempo map does.
    pub fn milli_bpm(self) -> u32 {
        self.milli_bpm
    }
}

impl Default for Tempo {
    fn default() -> Self {
        Self { milli_bpm: 120_000 }
    }
}

impl TryFrom<f64> for Tempo {
    type Error = ClockError;

    fn try_from(bpm: f64) -> Result<Self, ClockError> {
        Self::from_bpm(bpm)
    }
}

impl From<Tempo> for f64 {
    fn from(tempo: Tempo) -> f64 {
        tempo.bpm()
    }
}

/// One time signature for the whole project. Saved as a string, for example `"6/8"`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TimeSignature {
    numerator: u32,
    denominator: u32,
}

impl TimeSignature {
    /// `numerator` beats per bar, 1 to 32. `denominator` is the beat's note value: 1, 2, 4, 8,
    /// 16 or 32. These all divide a whole note of 3840 ticks, so a beat is a whole tick count.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, ClockError> {
        if !(1..=32).contains(&numerator) || !matches!(denominator, 1 | 2 | 4 | 8 | 16 | 32) {
            return Err(ClockError::UnsupportedTimeSignature {
                numerator,
                denominator,
            });
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    pub fn numerator(self) -> u32 {
        self.numerator
    }

    pub fn denominator(self) -> u32 {
        self.denominator
    }

    pub fn ticks_per_beat(self) -> u64 {
        TICKS_PER_QUARTER * 4 / u64::from(self.denominator)
    }

    pub fn ticks_per_bar(self) -> u64 {
        self.ticks_per_beat() * u64::from(self.numerator)
    }

    pub fn bar_beat_of(self, position: Ticks) -> BarBeat {
        let in_bar = position.0 % self.ticks_per_bar();
        BarBeat {
            bar: position.0 / self.ticks_per_bar() + 1,
            // Both fit: a bar has at most 32 beats and a beat at most 3840 ticks.
            beat: (in_bar / self.ticks_per_beat()) as u32 + 1,
            tick: (in_bar % self.ticks_per_beat()) as u32,
        }
    }

    /// Fails when the position does not exist in this signature, for example beat 5 in 4/4.
    pub fn ticks_of(self, position: BarBeat) -> Result<Ticks, ClockError> {
        let beat_exists = (1..=self.numerator).contains(&position.beat);
        let tick_exists = u64::from(position.tick) < self.ticks_per_beat();
        position
            .bar
            .checked_sub(1)
            .filter(|_| beat_exists && tick_exists)
            .and_then(|bars| bars.checked_mul(self.ticks_per_bar()))
            .and_then(|ticks| {
                let in_bar =
                    u64::from(position.beat - 1) * self.ticks_per_beat() + u64::from(position.tick);
                ticks.checked_add(in_bar)
            })
            .map(Ticks)
            .ok_or(ClockError::InvalidBarBeat {
                position,
                time_signature: self,
            })
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

impl fmt::Display for TimeSignature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.numerator, self.denominator)
    }
}

impl FromStr for TimeSignature {
    type Err = ClockError;

    fn from_str(text: &str) -> Result<Self, ClockError> {
        let parts = text.split_once('/').and_then(|(numerator, denominator)| {
            Some((
                numerator.trim().parse().ok()?,
                denominator.trim().parse().ok()?,
            ))
        });
        match parts {
            Some((numerator, denominator)) => Self::new(numerator, denominator),
            None => Err(ClockError::TimeSignatureSyntax(text.to_owned())),
        }
    }
}

impl TryFrom<String> for TimeSignature {
    type Error = ClockError;

    fn try_from(text: String) -> Result<Self, ClockError> {
        text.parse()
    }
}

impl From<TimeSignature> for String {
    fn from(time_signature: TimeSignature) -> String {
        time_signature.to_string()
    }
}

/// A position as musicians say it. `bar` and `beat` count from 1, `tick` counts from 0 inside
/// the beat. Shown as `bar:beat:tick`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BarBeat {
    pub bar: u64,
    pub beat: u32,
    pub tick: u32,
}

impl fmt::Display for BarBeat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}:{:03}", self.bar, self.beat, self.tick)
    }
}

/// From `tick` on, the tempo is `bpm`. A step: there are no ramps.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempoChange {
    pub tick: Ticks,
    pub bpm: Tempo,
}

/// The saved musical clock of a project: one time signature and the tempo changes.
///
/// ```json
/// {
///   "time_signature": "4/4",
///   "tempo_changes": [
///     { "tick": 0, "bpm": 120.0 },
///     { "tick": 15360, "bpm": 93.5 }
///   ]
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempoMap {
    time_signature: TimeSignature,
    #[serde(deserialize_with = "deserialize_tempo_changes")]
    tempo_changes: Vec<TempoChange>,
}

impl TempoMap {
    /// The first change must be at tick 0. Ticks must go up, each tick used once.
    pub fn new(
        time_signature: TimeSignature,
        tempo_changes: Vec<TempoChange>,
    ) -> Result<Self, ClockError> {
        check_tempo_changes(&tempo_changes)?;
        Ok(Self {
            time_signature,
            tempo_changes,
        })
    }

    /// One tempo for the whole project.
    pub fn constant(time_signature: TimeSignature, bpm: Tempo) -> Self {
        Self {
            time_signature,
            tempo_changes: vec![TempoChange {
                tick: Ticks(0),
                bpm,
            }],
        }
    }

    pub fn time_signature(&self) -> TimeSignature {
        self.time_signature
    }

    pub fn tempo_changes(&self) -> &[TempoChange] {
        &self.tempo_changes
    }

    /// The same map with the tempo change that starts exactly at `tick` set to `bpm`. `None`
    /// when the map has no change there, for example because it was removed from outside.
    ///
    /// Only a tempo changes, so the ticks keep their order and the result is valid by
    /// construction. That is why this cannot fail: an interface that edits one tempo change
    /// has no error to handle and none to drop.
    pub fn with_tempo_at(&self, tick: Ticks, bpm: Tempo) -> Option<Self> {
        let mut tempo_changes = self.tempo_changes.clone();
        let change = tempo_changes
            .iter_mut()
            .find(|change| change.tick == tick)?;
        change.bpm = bpm;
        Some(Self {
            time_signature: self.time_signature,
            tempo_changes,
        })
    }
}

/// 120 bpm in 4/4.
impl Default for TempoMap {
    fn default() -> Self {
        Self::constant(TimeSignature::default(), Tempo::default())
    }
}

fn check_tempo_changes(tempo_changes: &[TempoChange]) -> Result<(), ClockError> {
    if tempo_changes.first().map(|change| change.tick) != Some(Ticks(0)) {
        return Err(ClockError::NoTempoAtStart);
    }
    match tempo_changes
        .windows(2)
        .find(|pair| matches!(pair, [earlier, later] if later.tick <= earlier.tick))
    {
        Some([_, later]) => Err(ClockError::TempoChangesNotSorted(later.tick)),
        _ => Ok(()),
    }
}

fn deserialize_tempo_changes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<TempoChange>, D::Error> {
    let tempo_changes = Vec::deserialize(deserializer)?;
    check_tempo_changes(&tempo_changes).map_err(serde::de::Error::custom)?;
    Ok(tempo_changes)
}

/// The stretch of the timeline from one tempo change to the next.
#[derive(Copy, Clone, Debug)]
struct Segment {
    tick: u64,
    /// The frame of `tick`, rounded down like the frame of any tick. So each tempo change can
    /// move the ticks after it early by less than one frame. Every conversion goes through
    /// the same clock, so all parts of the application still agree on the frame of a tick.
    frame: u64,
    /// The same moment as [`Self::frame`], with the part of a frame it falls inside: the
    /// position in units of 1/2^[`SUB_FRAME_BITS`] of a frame.
    ///
    /// A tempo map with one change per beat has a segment per beat, and a segment that began
    /// on a whole frame would throw away up to a frame of the moment it really begins on. Over
    /// a thousand beats that is a part of a second of drift against the performance the map was
    /// fitted to. Carrying the fraction costs a shift and keeps the map the same piece at every
    /// sample rate. Every tick still lands on a whole frame: only the start of a segment keeps
    /// the fraction.
    start: u128,
    bpm: Tempo,
    /// One tick lasts `frames_per_tick.0 / frames_per_tick.1` frames, in lowest terms.
    frames_per_tick: (u64, u64),
}

/// How much of a frame a segment start keeps, as a binary fraction. A frame at 48 kHz is 21
/// microseconds, so this is far below anything the rest of the application can tell apart.
const SUB_FRAME_BITS: u32 = 32;

impl Segment {
    /// The exact position of a tick, in sub-frames.
    fn start_of(&self, tick: u64) -> u128 {
        let (frames, ticks) = self.frames_per_tick;
        let since_start = u128::from(tick.saturating_sub(self.tick))
            * u128::from(frames)
            * (1_u128 << SUB_FRAME_BITS)
            / u128::from(ticks);
        self.start.saturating_add(since_start)
    }

    fn frame_of(&self, tick: u64) -> u64 {
        let frame = self.start_of(tick) >> SUB_FRAME_BITS;
        u64::try_from(frame).unwrap_or(u64::MAX)
    }

    fn tick_at(&self, frame: u64) -> u64 {
        let (frames, ticks) = self.frames_per_tick;
        let since_start = (u128::from(frame) << SUB_FRAME_BITS).saturating_sub(self.start);
        let since_start = (since_start * u128::from(ticks))
            .div_ceil(u128::from(frames) * (1_u128 << SUB_FRAME_BITS));
        self.tick
            .saturating_add(u64::try_from(since_start).unwrap_or(u64::MAX))
    }
}

/// A [`TempoMap`] compiled for one sample rate. Immutable. The audio thread and the control
/// side share it through an `Arc`. Lookups are a binary search over the tempo changes.
#[derive(Clone, Debug)]
pub struct Clock {
    tempo_map: TempoMap,
    sample_rate: u32,
    first: Segment,
    /// The segments of every tempo change after the first, sorted by tick and by frame.
    later: Vec<Segment>,
}

impl Clock {
    /// Conversions are exact from [`MIN_EXACT_SAMPLE_RATE`] up. A sample rate of 0 counts as 1.
    pub fn new(tempo_map: TempoMap, sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1);
        let segment = |tick: Ticks, start: u128, bpm: Tempo| {
            let frames = u64::from(sample_rate) * 60_000;
            let ticks = u64::from(bpm.milli_bpm) * TICKS_PER_QUARTER;
            let divisor = greatest_common_divisor(frames, ticks);
            Segment {
                tick: tick.0,
                frame: u64::try_from(start >> SUB_FRAME_BITS).unwrap_or(u64::MAX),
                start,
                bpm,
                frames_per_tick: (frames / divisor, ticks / divisor),
            }
        };
        let mut changes = tempo_map.tempo_changes.iter();
        // A tempo map always starts with a change at tick 0. The fallback is never used.
        let first_bpm = changes.next().map(|change| change.bpm).unwrap_or_default();
        let first = segment(Ticks(0), 0, first_bpm);
        let mut later = Vec::with_capacity(changes.len());
        let mut previous = first;
        for change in changes {
            // The exact moment the change falls on, fraction of a frame and all, so a map of a
            // thousand changes is the same piece as one of two.
            previous = segment(change.tick, previous.start_of(change.tick.0), change.bpm);
            later.push(previous);
        }
        Self {
            tempo_map,
            sample_rate,
            first,
            later,
        }
    }

    /// The same sample rate with another tempo map.
    pub fn with_tempo_map(&self, tempo_map: TempoMap) -> Self {
        Self::new(tempo_map, self.sample_rate)
    }

    pub fn tempo_map(&self) -> &TempoMap {
        &self.tempo_map
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The one place where ticks become frames: the frame that contains the tick's exact time.
    pub fn frame_of(&self, tick: Ticks) -> Frames {
        Frames(self.segment_of_tick(tick).frame_of(tick.0))
    }

    /// The first tick at or after `frame`. So `tick_at(frame_of(tick)) == tick`, and the ticks
    /// inside frames `a..b` are exactly `tick_at(a)..tick_at(b)`.
    pub fn tick_at(&self, frame: Frames) -> Ticks {
        // The last segment that starts before the frame. The segment that starts on the frame
        // would also do when ticks are longer than frames, but not below the exact sample rate.
        let count = self
            .later
            .partition_point(|segment| segment.frame < frame.0);
        Ticks(self.segment_before(count).tick_at(frame.0))
    }

    pub fn tempo_at(&self, tick: Ticks) -> Tempo {
        self.segment_of_tick(tick).bpm
    }

    /// Seconds from the project start, through `frame_of`, so it agrees with the audio.
    pub fn seconds_of(&self, tick: Ticks) -> f64 {
        self.frame_of(tick).0 as f64 / f64::from(self.sample_rate)
    }

    /// The first tick at or after `seconds`. Negative values give tick 0.
    pub fn tick_at_seconds(&self, seconds: f64) -> Ticks {
        self.tick_at(Frames(
            (seconds * f64::from(self.sample_rate)).round() as u64
        ))
    }

    /// Microseconds from the project start, rounded to the nearest microsecond.
    ///
    /// This is the unit for a time that has to keep its meaning when the tempo map changes,
    /// such as a recorded performance. A microsecond is a small part of a frame at every
    /// sample rate this application allows, so `tick_at_micros(micros_of(tick)) == tick`.
    pub fn micros_of(&self, tick: Ticks) -> u64 {
        let frame = self.frame_of(tick).0 as f64;
        (frame * 1_000_000.0 / f64::from(self.sample_rate)).round() as u64
    }

    /// The tick of a time in microseconds from the project start: the inverse of
    /// [`Self::micros_of`].
    pub fn tick_at_micros(&self, micros: u64) -> Ticks {
        let frame = micros as f64 * f64::from(self.sample_rate) / 1_000_000.0;
        self.tick_at(Frames(frame.round() as u64))
    }

    fn segment_of_tick(&self, tick: Ticks) -> &Segment {
        let count = self.later.partition_point(|segment| segment.tick <= tick.0);
        self.segment_before(count)
    }

    /// The last of the first `count` later segments, or the first segment when `count` is 0.
    fn segment_before(&self, count: usize) -> &Segment {
        count
            .checked_sub(1)
            .and_then(|index| self.later.get(index))
            .unwrap_or(&self.first)
    }
}

fn greatest_common_divisor(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
