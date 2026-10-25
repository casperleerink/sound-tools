//! Plays a region longer or shorter than its file, at the pitch of its file: a region with warp
//! points.
//!
//! The points pin places in the file to ticks of the song with straight lines between them, so
//! every frame of the song has a place in the file that should sound then. A head reads the file
//! at its own speed, so at its own pitch, and stays within a period of that place by splicing: a
//! period on or back, faded in a straight line over a part of a period, with the period of the
//! last note heard where there is none, as the [`Shifter`](crate::Shifter) does. Whole periods
//! play twice or not at all.
//!
//! The shifter decides each splice from all it played before, because live sound cannot be read
//! ahead. A clip is heard from wherever the playhead is, and after a seek, a loop or in a render
//! from the middle it must play what playing through plays. So here, as in the
//! [`Resampler`](crate::Resampler), every frame is worked out on its own from the song position.
//! The periods of the whole file are read once, off the audio thread ([`Periods::of`]), and
//! counted from its start they give the phase of every place in it. The song position gives the
//! phase the sound heard has reached: the heads are the places at that phase, a whole number of
//! periods apart, and the two around the place that should sound are heard, the nearer louder.
//!
//! A head at that phase reads a little faster or slower where the period changes within a period
//! of it, as at a change of note. The periods are smoothed over a few hops so that stays small.

use std::sync::Arc;

use sound_core::{Clock, Ticks};

use crate::analysis::{PitchSettings, PitchTracker};
use crate::shifter::{FADE, UNHEARD_HZ};
use crate::{Audio, Region, varispeed};

/// How often the period is read, in seconds of the file.
const HOP_SECONDS: f64 = 0.005;
/// The lowest pitch looked for, in Hz, as pitch correction does.
const LOWEST_HZ: f64 = 60.0;
/// Hops each side the period is averaged over.
const SMOOTHING_HOPS: usize = 2;
/// How long the crossfade takes to follow a change of speed, so no weight jumps.
const EASE_SECONDS: f64 = 0.01;
/// The narrowest crossfade, in periods. Near the speed of the file the heads hardly move, and
/// a narrower one would switch from one to the other at once.
const NARROWEST: f64 = 0.05;

/// The periods of a file and its phase: its periods counted from its start.
#[derive(Debug)]
pub struct Periods {
    /// File frames per hop.
    hop: f64,
    /// The period over each hop, in frames of the file. Never empty.
    periods: Box<[f64]>,
    /// The phase where each hop starts, and where the last one ends.
    phases: Box<[f64]>,
}

impl Periods {
    /// Reads the pitch of the whole file: about a second for a few minutes of sound, so do it
    /// away from the audio thread and the thread that draws.
    pub fn of(audio: &Audio) -> Self {
        let rate = f64::from(audio.sample_rate().max(1));
        let hop = ((HOP_SECONDS * rate).round() as usize).max(1);
        let frames = usize::try_from(audio.frames()).unwrap_or(usize::MAX);
        let hops = frames.div_ceil(hop).max(1);
        let settings = PitchSettings {
            lowest: LOWEST_HZ,
            ..PitchSettings::FULL
        };
        let mut tracker = PitchTracker::new(rate, settings);
        let mut found = vec![None; hops];
        let mut block = vec![[0.0_f32; 2]; hop];
        // On into the silence after the end, so the last hops get a reading too.
        let end = hops
            .saturating_mul(hop)
            .saturating_add(2 * tracker.length());
        let mut heard = 0;
        while heard < end {
            audio.read(i64::try_from(heard).unwrap_or(i64::MAX), &mut block);
            tracker.push(block.iter().map(|[left, right]| 0.5 * (left + right)));
            heard += hop;
            let described = heard as f64 - tracker.age();
            if described >= 0.0
                && let Some(slot) = found.get_mut((described / hop as f64) as usize)
            {
                *slot = tracker.followed_period();
            }
        }
        // Where there is no clear period, as in drums, noise or a chord, the last one heard.
        let held: Vec<f64> = (found.iter())
            .scan(rate / UNHEARD_HZ, |last, period| {
                *last = period.unwrap_or(*last);
                Some(last.ln())
            })
            .collect();
        let periods: Box<[f64]> = (0..hops)
            .map(|index| {
                let around = index.saturating_sub(SMOOTHING_HOPS)..(index + SMOOTHING_HOPS + 1);
                let around = held.get(around.start..around.end.min(hops));
                let around = around.unwrap_or_default();
                (around.iter().sum::<f64>() / around.len().max(1) as f64).exp()
            })
            .collect();
        let phases = std::iter::once(0.0).chain(periods.iter().scan(0.0, |phase, period| {
            *phase += hop as f64 / period;
            Some(*phase)
        }));
        Self {
            hop: hop as f64,
            phases: phases.collect(),
            periods,
        }
    }

    /// The phase at `place`, in frames of the file. Past its ends its first and last periods
    /// go on.
    fn phase(&self, place: f64) -> f64 {
        let last = self.periods.len().saturating_sub(1);
        let index = ((place / self.hop).floor().max(0.0) as usize).min(last);
        match (self.phases.get(index), self.periods.get(index)) {
            (Some(start), Some(period)) => start + (place - index as f64 * self.hop) / period,
            _ => 0.0,
        }
    }

    /// The period at `place`, in frames of the file.
    fn period(&self, place: f64) -> f64 {
        let last = self.periods.len().saturating_sub(1);
        let index = ((place / self.hop).floor().max(0.0) as usize).min(last);
        self.periods.get(index).copied().unwrap_or(1.0)
    }

    /// The place at `phase`: the inverse of [`Self::phase`].
    fn place(&self, phase: f64) -> f64 {
        let starts = self.phases.get(..self.periods.len()).unwrap_or_default();
        let index = starts
            .partition_point(|start| *start <= phase)
            .saturating_sub(1);
        match (self.phases.get(index), self.periods.get(index)) {
            (Some(start), Some(period)) => index as f64 * self.hop + (phase - start) * period,
            _ => 0.0,
        }
    }
}

/// The warp points of a region as a player plays them.
pub(crate) struct Warp {
    periods: Arc<Periods>,
    /// Where the region starts in the song, and the file frame there.
    start: Ticks,
    origin: f64,
    /// Ticks after the start, and the file frame each pins there.
    points: Box<[(u64, f64)]>,
    /// File frames per engine frame at the speed of the file.
    step: f64,
    /// The phase at `origin`.
    origin_phase: f64,
    /// Engine frames of [`EASE_SECONDS`].
    ease: f64,
}

/// The two heads around the place that should sound, in frames of the file, and how much of
/// the one behind is heard.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Heads {
    ahead: f64,
    behind: f64,
    weight: f32,
}

/// A stretch of the region over which its file goes by at one speed, from frame `from` of the
/// region up to `until`. `heard` is the phase the sound heard has reached at `from`, in periods
/// of the place that should sound, at the speed of the file. `width` is the width of the
/// crossfade of the piece before, which this one's takes over from in [`EASE_SECONDS`].
#[derive(Clone, Copy)]
struct Piece {
    from: f64,
    until: f64,
    place: f64,
    phase: f64,
    speed: f64,
    heard: f64,
    width: f64,
}

impl Piece {
    /// How much of the head behind is heard at frame `frame`, the head ahead `ahead` periods
    /// ahead of the place that should sound.
    fn weight(&self, frame: f64, ahead: f64, warp: &Warp) -> f64 {
        let eased = ((frame - self.from) / warp.ease).clamp(0.0, 1.0);
        let own = width(self.speed, warp.step);
        (1.0 - eased) * weight(ahead, self.width) + eased * weight(ahead, own)
    }
}

/// The width of the crossfade at `speed`, in periods of the head ahead: [`FADE`] of a period
/// of what is heard, where `step` is the speed of the file.
fn width(speed: f64, step: f64) -> f64 {
    (FADE * (1.0 - speed / step).abs()).clamp(NARROWEST, 1.0)
}

/// How much of the head behind is heard in a crossfade of `width` centred where both heads are
/// as near.
fn weight(ahead: f64, width: f64) -> f64 {
    ((ahead - 0.5) / width + 0.5).clamp(0.0, 1.0)
}

impl Warp {
    /// The warp of `region`, whose points each come after the one before in ticks and seconds,
    /// from a file at the first of `rates` in an engine at the second.
    pub(crate) fn new(periods: Arc<Periods>, region: &Region, (file, engine): (u32, u32)) -> Self {
        let rate = f64::from(file.max(1));
        let origin = region.file_start_seconds * rate;
        Self {
            origin_phase: periods.phase(origin),
            periods,
            start: region.start,
            origin,
            points: (region.warp.iter())
                .map(|point| (point.ticks.0, point.seconds * rate))
                .collect(),
            step: rate / f64::from(engine.max(1)),
            ease: EASE_SECONDS * f64::from(engine),
        }
    }

    /// Where the speed changes, in frames of the region and of the file: its start, each point
    /// and each tempo change between two.
    fn knots<'a>(&'a self, clock: &'a Clock) -> impl Iterator<Item = (f64, f64)> + 'a {
        let base = clock.frame_of(self.start).0;
        let frame = move |ticks: u64| {
            let tick = Ticks(self.start.0.saturating_add(ticks));
            clock.frame_of(tick).0.saturating_sub(base) as f64
        };
        let changes = clock.tempo_map().tempo_changes();
        let anchors = std::iter::once((0, self.origin)).chain(self.points.iter().copied());
        let segments = anchors.zip(self.points.iter().copied());
        let inside = segments.flat_map(move |((from, at), (to, then))| {
            let (first, last) = (
                self.start.0.saturating_add(from),
                self.start.0.saturating_add(to),
            );
            let after = changes.partition_point(|change| change.tick.0 <= first);
            let between = (changes.get(after..).unwrap_or_default().iter())
                .take_while(move |change| change.tick.0 < last);
            let knots = between.map(move |change| {
                let along = (change.tick.0 - first) as f64 / (last - first) as f64;
                (
                    frame(change.tick.0 - self.start.0),
                    at + along * (then - at),
                )
            });
            knots.chain(std::iter::once((frame(to), then)))
        });
        std::iter::once((0.0, self.origin)).chain(inside)
    }

    /// The piece that holds frame `frame` of the region. After the last point the file goes on
    /// at the speed of the last piece.
    fn piece_at(&self, clock: &Clock, frame: f64) -> Piece {
        let mut knots = self.knots(clock);
        let (from, place) = knots.next().unwrap_or((0.0, self.origin));
        let mut piece = Piece {
            from,
            until: f64::INFINITY,
            place,
            phase: self.origin_phase,
            speed: self.step,
            heard: 0.0,
            width: 0.0,
        };
        let mut first = true;
        for (until, then) in knots {
            // Within one frame: the next piece starts here.
            if until <= piece.from {
                continue;
            }
            let speed = (then - piece.place) / (until - piece.from);
            if first {
                piece.width = width(speed, self.step);
                first = false;
            }
            piece.speed = speed;
            if frame < until {
                return Piece { until, ..piece };
            }
            let phase = self.periods.phase(then);
            piece = Piece {
                from: until,
                place: then,
                phase,
                heard: piece.heard + (phase - piece.phase) / speed,
                width: width(speed, self.step),
                ..piece
            };
        }
        piece
    }

    fn heads(&self, piece: &Piece, frame: f64) -> Heads {
        let place = piece.place + piece.speed * (frame - piece.from);
        let phase = self.periods.phase(place);
        let heard = piece.heard + (phase - piece.phase) / piece.speed;
        let reading = self.origin_phase + self.step * heard;
        // How far the head ahead is ahead of the place, from 0 to 1 period.
        let periods = (reading - phase).floor();
        let ahead = reading - phase - periods;
        let weight = piece.weight(frame, ahead, self);
        Heads {
            ahead: self.periods.place(reading - periods),
            behind: self.periods.place(reading - periods - 1.0),
            weight: weight as f32,
        }
    }

    /// The heads at frame `frame` of the region.
    pub(crate) fn heads_at(&self, clock: &Clock, frame: u64) -> Heads {
        let frame = frame as f64;
        self.heads(&self.piece_at(clock, frame), frame)
    }

    /// How many engine frames the region plays, 1 or more, and how many its file can play
    /// from its start, past its end too.
    pub(crate) fn bounds(&self, clock: &Clock, file_frames: u64) -> (u64, u64) {
        let ticks = self.points.last().map_or(0, |(ticks, _)| *ticks);
        let end = clock.frame_of(Ticks(self.start.0.saturating_add(ticks))).0;
        let length = end.saturating_sub(clock.frame_of(self.start).0).max(1);
        // The head ahead may be a period ahead of the place.
        let last = self.piece_at(clock, f64::INFINITY);
        let end = file_frames as f64 - self.periods.period(last.place);
        let left = (end - last.place).max(0.0) / last.speed;
        (length, length.saturating_add(left as u64))
    }

    /// Frames `first..` of the region, into `out`. `scratch` is at least
    /// [`SCRATCH_FRAMES`](crate::SCRATCH_FRAMES) long.
    ///
    /// Realtime safe: no allocation, lock or system call.
    pub(crate) fn render(
        &self,
        audio: &Audio,
        clock: &Clock,
        first: u64,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        let mut piece = self.piece_at(clock, first as f64);
        for (index, frame) in out.iter_mut().enumerate() {
            let at = (first + index as u64) as f64;
            if at >= piece.until {
                piece = self.piece_at(clock, at);
            }
            *frame = self.read(audio, self.heads(&piece, at), scratch);
        }
    }

    /// What `heads` read on at the speed of the file, with no more splices, into `out`: what
    /// sounded goes on for a fade after a jump.
    pub(crate) fn render_on(
        &self,
        audio: &Audio,
        heads: Heads,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        for (index, frame) in out.iter_mut().enumerate() {
            let on = index as f64 * self.step;
            let heads = Heads {
                ahead: heads.ahead + on,
                behind: heads.behind + on,
                ..heads
            };
            *frame = self.read(audio, heads, scratch);
        }
    }

    fn read(&self, audio: &Audio, heads: Heads, scratch: &mut [[f32; 2]]) -> [f32; 2] {
        let mut sum = [0.0; 2];
        let mut frame = [[0.0; 2]];
        for (place, weight) in [
            (heads.ahead, 1.0 - heads.weight),
            (heads.behind, heads.weight),
        ] {
            if weight > 0.0 {
                varispeed().render(audio, place, self.step, &mut frame, scratch);
                sum = [0, 1].map(|channel| sum[channel] + weight * frame[0][channel]);
            }
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use sound_core::{Tempo, TempoMap, TimeSignature};

    use super::*;
    use crate::{AudioAsset, WarpPoint};

    /// The largest move from one frame to the next, around frame `around`, of where the heads
    /// read, each by its weight, for a sound of one period of 10 ms. A weight that jumps moves
    /// it by part of a period at once.
    fn largest_move(warp: &[(u64, f64)], around: u64) -> f64 {
        let periods = Periods {
            hop: 240.0,
            periods: vec![480.0; 1_000].into(),
            phases: (0..=1_000).map(|hop| f64::from(hop) / 2.0).collect(),
        };
        let region = Region {
            asset: AudioAsset::try_from("drums.wav".to_string()).unwrap(),
            start: Ticks(0),
            file_start_seconds: 0.0,
            file_end_seconds: None,
            gain: 1.0,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            warp: (warp.iter())
                .map(|(ticks, seconds)| WarpPoint {
                    ticks: Ticks(*ticks),
                    seconds: *seconds,
                })
                .collect(),
        };
        let warp = Warp::new(Arc::new(periods), &region, (48_000, 48_000));
        let signature = TimeSignature::new(4, 4).unwrap();
        let tempo = TempoMap::constant(signature, Tempo::from_bpm(120.0).unwrap());
        let clock = Clock::new(tempo, 48_000);
        let reads: Vec<f64> = (around - 2_000..around + 2_000)
            .map(|frame| {
                let heads = warp.heads_at(&clock, frame);
                let weight = f64::from(heads.weight);
                (1.0 - weight) * heads.ahead + weight * heads.behind
            })
            .collect();
        (reads.windows(2))
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn the_crossfade_follows_a_change_of_speed_with_no_jump() {
        // From half the speed of the file to its speed at frame 24,000, wherever the heads are.
        for second in [0.2547, 0.2548, 0.2552, 0.2553] {
            let moved = largest_move(&[(960, second), (2_880, second + 1.0)], 24_000);
            assert!(moved < 10.0, "{second}: {moved} frames");
        }
    }
}
