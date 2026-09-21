//! From the beats of a take to the tempo map of the project and the clip of the take.
//!
//! Everything here is a pure function of the take and the fit record, so the same inputs give
//! the same bytes on every run. Times are microseconds on the project timeline, because that is
//! the one unit a tempo map change cannot alter.
//!
//! The grid it builds:
//!
//! - One tempo step per beat, and consecutive beats that the step before them already lands
//!   right share it. At 100 % steadiness every step holds the same tempo to within a hundredth
//!   of a bpm, and every beat is the same length to within four frames: one tempo as far as
//!   anything can hear. It is not one number, because a beat of thirty thousand frames cannot
//!   be hit exactly by a tempo held in thousandths of a bpm.
//! - The first downbeat lands on a bar line. The bars before it hold the beats that were played
//!   before it, the pickup, and at least one bar of the silence in front of the take: the tempo
//!   of that one step stretches to cover exactly the time between the start of the piece and
//!   the first beat the take has, so the take stays where it was heard.
//! - Every tempo is chosen against the frame the beat has to land on, not against the length of
//!   the beat before it, so the rounding of a tempo to 0.001 bpm does not add up over a long
//!   take. The frames are simulated exactly as [`Clock`] computes them, at
//!   [`FIT_SAMPLE_RATE`]: a map is saved without a sample rate, and building it against the
//!   device of the moment would make a fit different on two machines.

use serde::{Deserialize, Serialize};
use sound_core::{Clock, TICKS_PER_QUARTER, Tempo, TempoChange, TempoMap, Ticks, TimeSignature};
use sound_notes::{Clip, RawTake};

use crate::beats;

/// The sample rate a fitted tempo map is built for. Offline renders use it, so the check that a
/// fitted project renders like the take it came from is exact. At another rate the clock rounds
/// each tempo change down to a whole frame, which moves the grid slowly: see `README.md`.
pub const FIT_SAMPLE_RATE: u32 = 48_000;

/// How many beats the grid gets for every beat the finder heard.
///
/// The finder cannot know the octave from the timing alone: the same playing is a grid of one
/// beat per chord or of two. This is the field an agent corrects when the grid runs twice as
/// fast or half as fast as the music.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BeatRate {
    /// One beat for every two the finder heard.
    Half,
    #[default]
    Normal,
    /// Two beats for every one the finder heard.
    Double,
}

impl BeatRate {
    pub const ALL: [Self; 3] = [Self::Half, Self::Normal, Self::Double];

    pub fn name(self) -> &'static str {
        match self {
            Self::Half => "half",
            Self::Normal => "normal",
            Self::Double => "double",
        }
    }
}

/// Everything a fit computes from its inputs. Nothing of it is saved twice: the tempo map goes
/// into `project.json` and the clip into the take's clip record, and both are made again from
/// the fit record whenever it changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    /// The moment of every beat of the grid on the project timeline, in microseconds. Index 0
    /// is tick 0 and is always 0, so index `j` is tick `j * ticks_per_beat`.
    pub targets_us: Vec<u64>,
    /// Which index of `targets_us` is the first downbeat. Always on a bar line.
    pub first_downbeat: usize,
    /// The tempo map at 0 % steadiness: the grid as it was played.
    pub map: TempoMap,
    /// The clip of the take under that map. The caller puts the take's name back in it.
    pub clip: Clip,
    /// What could not be done exactly, for `problems.txt`.
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FitError {
    #[error(
        "the take has fewer than {} chords or notes to find a beat in, so there is no grid to fit. Record more, or take the fit out",
        beats::MIN_ONSETS
    )]
    TooFewNotes,
    #[error("no beat was found in the take")]
    NoBeats,
}

/// Fits `take` with the inputs of a fit record.
///
/// `first_downbeat_us` counts from the start of the recording, like every time in the take, so
/// an agent reads the right value straight out of the take file.
pub fn fit(
    take: &RawTake,
    time_signature: TimeSignature,
    first_downbeat_us: u64,
    rate: BeatRate,
) -> Result<Fitted, FitError> {
    let heard = beats::find_beats(&take.events);
    if heard.len() < 2 {
        return Err(
            match beats::onsets(&take.events).len() < beats::MIN_ONSETS {
                true => FitError::TooFewNotes,
                false => FitError::NoBeats,
            },
        );
    }
    let (beats_us, downbeat) = at_rate(&heard, first_downbeat_us, rate);
    if beats_us.len() < 2 {
        return Err(FitError::NoBeats);
    }
    // The take keeps its place on the project timeline: every beat moves with it.
    let beats_us: Vec<u64> = beats_us
        .iter()
        .map(|time| time.saturating_add(take.start_us))
        .collect();

    let lead = lead_beats(downbeat, time_signature, beats_us[0]);
    let targets_us = targets(&beats_us, lead);
    let (map, problems) = tempo_map(time_signature, &targets_us);
    let clock = Clock::new(map.clone(), FIT_SAMPLE_RATE);
    let clip = take
        .clip(|time_us| clock.tick_at_micros(time_us))
        .ok_or(FitError::TooFewNotes)?;
    Ok(Fitted {
        targets_us,
        first_downbeat: lead + downbeat,
        map,
        clip,
        problems,
    })
}

impl Fitted {
    /// The tempo map at a steadiness from 0 to 1. At 0 it is [`Self::map`] again, byte for
    /// byte. At 1 every beat is the same length to within four frames, 83 microseconds at 48
    /// kHz, so the piece has one tempo up to a hundredth of a bpm.
    ///
    /// The first and the last beat keep their moment, so the piece begins and ends where it
    /// did. Everything between them slides, which is what moves the playing towards the grid.
    pub fn map_at(&self, time_signature: TimeSignature, steadiness: f32) -> TempoMap {
        let steadiness = f64::from(steadiness.clamp(0.0, 1.0));
        if steadiness == 0.0 {
            return self.map.clone();
        }
        let (map, _) = tempo_map(time_signature, &steady(&self.targets_us, steadiness));
        map
    }

    /// The tick of the first downbeat, for a summary and for the agent doc.
    pub fn first_downbeat_tick(&self, time_signature: TimeSignature) -> Ticks {
        Ticks(self.first_downbeat as u64 * time_signature.ticks_per_beat())
    }

    /// How many beats the grid has, without the one at tick 0.
    pub fn beat_count(&self) -> usize {
        self.targets_us.len().saturating_sub(1)
    }
}

/// The beats between the ends, moved towards even spacing by `steadiness`.
fn steady(targets_us: &[u64], steadiness: f64) -> Vec<u64> {
    let Some(last) = targets_us.last().copied() else {
        return Vec::new();
    };
    let steps = targets_us.len().saturating_sub(1).max(1) as f64;
    targets_us
        .iter()
        .enumerate()
        .map(|(index, played)| {
            let even = last as f64 * index as f64 / steps;
            ((1.0 - steadiness) * *played as f64 + steadiness * even).round() as u64
        })
        .collect()
}

/// The beats the grid has at this rate, and which of them is the first downbeat.
fn at_rate(heard: &[u64], first_downbeat_us: u64, rate: BeatRate) -> (Vec<u64>, usize) {
    let nearest = heard
        .iter()
        .enumerate()
        .min_by_key(|(_, time)| time.abs_diff(first_downbeat_us))
        .map_or(0, |(index, _)| index);
    match rate {
        BeatRate::Normal => (heard.to_vec(), nearest),
        BeatRate::Double => {
            let mut beats = Vec::with_capacity(heard.len() * 2);
            for pair in heard.windows(2) {
                beats.push(pair[0]);
                beats.push(pair[0] + (pair[1] - pair[0]) / 2);
            }
            beats.extend(heard.last().copied());
            (beats, nearest * 2)
        }
        BeatRate::Half => {
            // Every second beat, counted from the downbeat, so the downbeat stays a beat.
            let beats: Vec<u64> = heard
                .iter()
                .enumerate()
                .filter(|(index, _)| index.abs_diff(nearest) % 2 == 0)
                .map(|(_, time)| *time)
                .collect();
            (beats, nearest / 2)
        }
    }
}

/// How many beats of the grid come before the first beat of the take.
///
/// The first downbeat lands on a bar line, so the beats before it fill whole bars: the pickup
/// the composer played, plus at least one beat of the silence in front of the take. A take that
/// begins at the very start of the piece with its downbeat first needs no bar in front of it.
fn lead_beats(downbeat: usize, time_signature: TimeSignature, first_beat_us: u64) -> usize {
    let per_bar = time_signature.numerator() as usize;
    if downbeat == 0 && first_beat_us == 0 {
        return 0;
    }
    (downbeat / per_bar + 1) * per_bar - downbeat
}

/// Every beat of the grid on the project timeline, index 0 at tick 0.
fn targets(beats_us: &[u64], lead: usize) -> Vec<u64> {
    let first = beats_us.first().copied().unwrap_or(0);
    let mut targets = Vec::with_capacity(lead + beats_us.len());
    // The silence in front of the take is one stretch of even beats, so the map needs one
    // tempo step for it however many beats it holds.
    for index in 0..lead {
        targets.push(first * index as u64 / lead as u64);
    }
    targets.extend_from_slice(beats_us);
    targets
}

/// The tempo map that puts beat `j` on the frame of `targets_us[j]`, and what it could not do.
fn tempo_map(time_signature: TimeSignature, targets_us: &[u64]) -> (TempoMap, Vec<String>) {
    let ticks_per_beat = time_signature.ticks_per_beat();
    let frames: Vec<u64> = targets_us.iter().map(|time| frame_of(*time)).collect();
    let mut changes: Vec<TempoChange> = Vec::new();
    let mut segment: Option<Segment> = None;
    let mut actual = 0_u64;
    let mut clamped = 0_usize;
    for (index, target) in frames.iter().enumerate().skip(1) {
        let tick = index as u64 * ticks_per_beat;
        // What this beat needs from where the beat before it really lands, so that the rounding
        // of every earlier tempo is corrected here instead of adding up.
        let needed = target.saturating_sub(actual).max(1);
        let wanted = 60.0 * f64::from(FIT_SAMPLE_RATE) * ticks_per_beat as f64
            / (TICKS_PER_QUARTER as f64 * needed as f64);
        let bpm = Tempo::from_bpm(wanted.clamp(Tempo::MIN_BPM, Tempo::MAX_BPM))
            .unwrap_or_else(|_| Tempo::default());
        if !(Tempo::MIN_BPM..=Tempo::MAX_BPM).contains(&wanted) {
            clamped += 1;
        }
        let start = (index as u64 - 1) * ticks_per_beat;
        // The step of the beat before this one is kept when it already lands this beat where
        // it belongs. Without that, a run of evenly spaced beats would get a step each, whose
        // tempos differ by a thousandth of a bpm as the rounding is corrected, and 100 %
        // steadiness would not be one tempo but hundreds that are nearly the same.
        let keeps = |segment: &Segment| {
            segment.bpm == bpm || segment.frame_of(tick).abs_diff(*target) <= TOLERANCE_FRAMES
        };
        let current = match segment.filter(keeps) {
            Some(segment) => segment,
            None => {
                changes.push(TempoChange {
                    tick: Ticks(start),
                    bpm,
                });
                let fresh = Segment::new(start, actual, bpm);
                segment = Some(fresh);
                fresh
            }
        };
        actual = current.frame_of(tick);
    }
    let map = TempoMap::new(time_signature, changes)
        .unwrap_or_else(|_| TempoMap::constant(time_signature, Tempo::default()));
    let mut problems = Vec::new();
    if clamped > 0 {
        problems.push(format!(
            "{clamped} of {} beats are too far apart or too close together for a tempo between {} and {} bpm, so the grid does not follow the playing there",
            frames.len() - 1,
            Tempo::MIN_BPM,
            Tempo::MAX_BPM
        ));
    }
    (map, problems)
}

/// How far a beat may land from the frame it was built for before the map gets a step of its
/// own for it. One frame at 48 kHz is 21 microseconds, far below a tick.
const TOLERANCE_FRAMES: u64 = 1;

/// The frame of a project time in microseconds, at the rate a fit is built for.
fn frame_of(time_us: u64) -> u64 {
    (time_us as f64 * f64::from(FIT_SAMPLE_RATE) / 1_000_000.0).round() as u64
}

/// One stretch of the tempo map, with the same arithmetic [`Clock`] uses, so that the frame
/// this builder expects for a beat is the frame the clock gives it. A test holds the two
/// together.
#[derive(Copy, Clone, Debug, PartialEq)]
struct Segment {
    tick: u64,
    frame: u64,
    bpm: Tempo,
    frames_per_tick: (u64, u64),
}

impl Segment {
    fn new(tick: u64, frame: u64, bpm: Tempo) -> Self {
        let frames = u64::from(FIT_SAMPLE_RATE) * 60_000;
        let ticks = u64::from(bpm.milli_bpm()) * TICKS_PER_QUARTER;
        let divisor = greatest_common_divisor(frames, ticks);
        Self {
            tick,
            frame,
            bpm,
            frames_per_tick: (frames / divisor, ticks / divisor),
        }
    }

    fn frame_of(&self, tick: u64) -> u64 {
        let (frames, ticks) = self.frames_per_tick;
        let since =
            u128::from(tick.saturating_sub(self.tick)) * u128::from(frames) / u128::from(ticks);
        self.frame
            .saturating_add(u64::try_from(since).unwrap_or(u64::MAX))
    }
}

fn greatest_common_divisor(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
