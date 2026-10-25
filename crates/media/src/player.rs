//! Plays stretches of audio files at places in the song: one immutable snapshot of them on the
//! control side, one processor that reads them from the transport's frame range on the audio
//! thread. A `region` plays itself through one.
//!
//! The rules that shape this file:
//!
//! - Nothing reads a file here. Every file is in memory, in the snapshot, before it arrives.
//! - Where clips overlap, the one that comes last in the snapshot is heard. The others are not
//!   changed, only not heard there.
//! - No edge clicks, and no join dips. Where a clip hands over to another, at a cut between
//!   two clips or where one covers or uncovers another, the outgoing clip plays on past the
//!   edge for [`DECLICK_SECONDS`], reading its file past its trim, and fades out while the
//!   incoming one fades in: a crossfade. A free edge, with silence on the other side, and a
//!   hand-over where the outgoing file has nothing more to play, get the same ramp inside the
//!   clip. A fade of 0 ms is this ramp alone.
//! - Where the sound would jump, because of a seek, a stop, a tempo change or an edit while it
//!   plays, what sounded goes on for one ramp and fades out while the new sound fades in.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sound_core::{
    Assets, AudioOutput, Clock, Frames, MAX_BLOCK, Ports, PrepareConfig, ProcessContext, Processor,
    Ticks,
};

use crate::stretch::{Heads, Warp};
use crate::{Audio, AudioAsset, MediaError, Periods, Resampler, SCRATCH_FRAMES};

/// How long the ramp at every edge of a clip is. Not a setting, and not drawn.
pub const DECLICK_SECONDS: f64 = 0.002;

/// One clip as the audio thread plays it: its file, where it starts, and its level, all in
/// engine frames except the start.
struct PlacedAudio {
    start: Ticks,
    audio: Arc<Audio>,
    resampler: Arc<Resampler>,
    /// The file frame the clip starts playing at.
    origin: u64,
    /// Engine frames the clip plays, 1 or more.
    length: u64,
    /// Engine frames the file holds from `origin` on: `length`, and what is past the trim.
    available: u64,
    gain: f32,
    fade_in: u64,
    fade_out: u64,
    /// Its warp points, which make the clock say its length instead.
    warp: Option<Warp>,
}

/// A place in the file pinned to a place in the song.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WarpPoint {
    /// Ticks after the region's start.
    pub ticks: Ticks,
    /// Seconds into the file.
    pub seconds: f64,
}

/// A stretch of an audio file at a place in the song, as a player is given it. Without warp
/// points it plays at the speed of its file, so a tempo change moves only where it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    /// The file, under `assets/audio/`.
    pub asset: AudioAsset,
    /// Where it starts in the song.
    pub start: Ticks,
    /// Where in the file it starts playing.
    pub file_start_seconds: f64,
    /// Where in the file it stops: the end of the file with `None`.
    pub file_end_seconds: Option<f64>,
    /// How much louder it plays than its file, as a factor.
    pub gain: f32,
    /// A straight line from silence at its start to its level, and from its level to silence
    /// at its end.
    pub fade_in_ms: f32,
    pub fade_out_ms: f32,
    /// After its start, which plays `file_start_seconds`, each point plays its second of the file
    /// at its tick, with straight lines between. It stops at the last.
    pub warp: Vec<WarpPoint>,
}

impl Region {
    /// The part of the file that plays, in frames of the file: from, and up to.
    pub fn file_frames(&self, file: &crate::Info) -> (u64, u64) {
        let end = self.warp.last().map(|point| point.seconds);
        file_frames(file, self.file_start_seconds, end.or(self.file_end_seconds))
    }

    /// How many engine frames that part of the file lasts at `engine_rate`, at its own speed.
    pub fn length(&self, file: &crate::Info, engine_rate: u32) -> u64 {
        let (from, to) = self.file_frames(file);
        crate::engine_frames(to - from, file.sample_rate, engine_rate)
    }

    /// Where it stops in the song.
    pub fn end(&self, file: &crate::Info, clock: &Clock) -> Ticks {
        match self.warp.last() {
            Some(point) => Ticks(self.start.0.saturating_add(point.ticks.0)),
            None => {
                let length = self.length(file, clock.sample_rate());
                clock.tick_at(Frames(clock.frame_of(self.start).0 + length))
            }
        }
    }

    /// The second of its file that plays at `tick`, at or after its start.
    pub fn file_seconds_at(&self, tick: Ticks, clock: &Clock) -> f64 {
        let after = tick.0.saturating_sub(self.start.0);
        // The line it is on, or the last one on past its end.
        let index = self.warp.partition_point(|point| point.ticks.0 <= after);
        let index = index.min(self.warp.len().saturating_sub(1));
        let Some(to) = self.warp.get(index).copied() else {
            return clock.seconds_of(tick) - clock.seconds_of(self.start) + self.file_start_seconds;
        };
        let start = WarpPoint {
            ticks: Ticks(0),
            seconds: self.file_start_seconds,
        };
        let from = index
            .checked_sub(1)
            .and_then(|before| self.warp.get(before));
        let from = from.copied().unwrap_or(start);
        let along = (after as f64 - from.ticks.0 as f64) / (to.ticks.0 - from.ticks.0) as f64;
        from.seconds + along * (to.seconds - from.seconds)
    }
}

/// The part of `file` from `start_seconds` to `end_seconds`, the end of the file with `None`, in
/// frames of the file: from, and up to.
pub fn file_frames(file: &crate::Info, start_seconds: f64, end_seconds: Option<f64>) -> (u64, u64) {
    let rate = f64::from(file.sample_rate);
    let frame = |seconds: f64| ((seconds * rate).round().max(0.0) as u64).min(file.frames);
    let from = frame(start_seconds);
    let to = end_seconds.map_or(file.frames, frame);
    (from, to.max(from))
}

/// The snapshot of `regions`, which `what` names in a problem such as `the clip "verse"`, the
/// last one heard where they overlap, and a line for each that cannot play. A region with warp
/// points plays with the periods of its file, and is left out without them.
///
/// Reads every file the first time it is named, on this thread, see [`crate::load`]. A file
/// that does not play, or a region that plays none of its file, costs one look at the file
/// after that, see [`crate::info`].
pub fn snapshot(
    regions: impl IntoIterator<Item = (String, Region, Option<Arc<Periods>>)>,
    assets: &Assets,
    engine_rate: u32,
) -> (AudioSnapshot, Vec<String>) {
    let mut clips = Vec::new();
    let mut problems = Vec::new();
    for (what, region, periods) in regions {
        let file = match crate::info(assets, &region.asset) {
            Ok(file) => file,
            Err(MediaError::Missing { path }) => {
                problems.push(format!(
                    "{what} plays {path}, which is not there. It keeps its place and is silent, and the rest plays. Copy the file into assets/audio/ under that name and it plays, or correct `asset`"
                ));
                continue;
            }
            Err(error) => {
                problems.push(format!("{what} is silent: {error}"));
                continue;
            }
        };
        let length = region.length(&file, engine_rate);
        if length == 0 {
            problems.push(format!(
                "{what} plays nothing: file_start_seconds {} is at or past the end of {}, which is {:.3} s long",
                region.file_start_seconds,
                region.asset,
                file.seconds()
            ));
            continue;
        }
        let audio = match crate::load(assets, &region.asset) {
            Ok(audio) => audio,
            Err(error) => {
                problems.push(format!("{what} is silent: {error}"));
                continue;
            }
        };
        if let Some(point) = (region.warp.iter()).find(|point| point.seconds > file.seconds()) {
            problems.push(format!(
                "{what} is silent: a warp point plays second {} of {}, which is {:.3} s long. Move the point inside the file",
                point.seconds,
                region.asset,
                file.seconds()
            ));
            continue;
        }
        let warp = match (region.warp.is_empty(), periods) {
            (true, _) => None,
            (false, None) => continue,
            (false, Some(periods)) => {
                // Made here, on the control side, and never first on the audio thread.
                crate::varispeed();
                let rates = (audio.sample_rate(), engine_rate);
                Some(Warp::new(periods, &region, rates))
            }
        };
        let file = audio.info();
        let (origin, _) = region.file_frames(&file);
        // What the file holds from where it starts, which a join may play past its end.
        let rate = file.sample_rate;
        let available = crate::engine_frames(file.frames - origin, rate, engine_rate);
        let milliseconds =
            |ms: f32| (f64::from(ms) / 1000.0 * f64::from(engine_rate)).round() as u64;
        clips.push(PlacedAudio {
            start: region.start,
            resampler: crate::resampler(audio.sample_rate(), engine_rate),
            audio,
            origin,
            length,
            available,
            gain: region.gain,
            fade_in: milliseconds(region.fade_in_ms),
            fade_out: milliseconds(region.fade_out_ms),
            warp,
        });
    }
    (AudioSnapshot { clips }, problems)
}

/// A part of a clip that is heard, in frames of the clip: from, up to, and whether it hands
/// over to another clip at its end, so that it plays on past it for one ramp, fading out.
#[derive(Copy, Clone)]
struct Part {
    from: u64,
    to: u64,
    hands_over: bool,
}

/// Where a clip reads its file in a block: through the clock of the block, or on from where its
/// heads were, for what sounded before a jump.
#[derive(Copy, Clone)]
enum Reading<'a> {
    Clock(&'a Clock),
    On(Heads),
}

impl PlacedAudio {
    /// How many engine frames the clip plays, and how many its file holds from where it starts.
    fn bounds(&self, clock: &Clock) -> (u64, u64) {
        match &self.warp {
            None => (self.length, self.available),
            Some(warp) => warp.bounds(clock, self.audio.frames()),
        }
    }

    /// The level of frame `frame` of the clip, `length` frames long, heard in `part`: the gain,
    /// the fades, and the ramp at each edge of that part. `entered` frames ago the sound came in
    /// after a jump, which is one more edge with the same ramp. A clip that starts where the
    /// sound comes in therefore gets one ramp and not two.
    fn level(&self, frame: u64, length: u64, part: Part, ramp: u64, entered: u64) -> f32 {
        let end = match part.hands_over {
            true => part.to + ramp,
            false => part.to,
        };
        if frame < part.from || frame >= end {
            return 0.0;
        }
        let steps = (ramp + 1) as f64;
        let rising = (frame - part.from + 1) as f64 / steps;
        let falling = (end - frame) as f64 / steps;
        let entering = entered.saturating_add(1) as f64 / steps;
        let mut level = rising.min(falling).min(entering).min(1.0);
        if self.fade_in > 0 {
            level = level.min(frame as f64 / self.fade_in as f64);
        }
        if self.fade_out > 0 {
            level = level.min(length.saturating_sub(frame) as f64 / self.fade_out as f64);
        }
        (level * f64::from(self.gain)) as f32
    }

    /// Frames `first..` of the clip, `length` frames long, as heard in `part`, into `out`.
    /// `entered` is for `first`, see [`Self::level`].
    fn write(
        &self,
        reading: Reading<'_>,
        (first, entered): (u64, u64),
        (part, length): (Part, u64),
        ramp: u64,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        match (&self.warp, reading) {
            (None, _) => self
                .resampler
                .render(&self.audio, self.origin, first, out, scratch),
            (Some(warp), Reading::Clock(clock)) => {
                warp.render(&self.audio, clock, first, out, scratch);
            }
            (Some(warp), Reading::On(heads)) => warp.render_on(&self.audio, heads, out, scratch),
        }
        for (index, frame) in out.iter_mut().enumerate() {
            let index = index as u64;
            let entered = entered.saturating_add(index);
            let level = self.level(first + index, length, part, ramp, entered);
            *frame = frame.map(|sample| sample * level);
        }
    }
}

/// The audio clips of one track, the one heard where they overlap last.
#[derive(Default)]
pub struct AudioSnapshot {
    clips: Vec<PlacedAudio>,
}

impl AudioSnapshot {
    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

/// Where a clip is in project frames for one block, under the clock of that block, and how
/// many frames its file holds from its start.
#[derive(Copy, Clone, Default)]
struct Span {
    start: u64,
    end: u64,
    available: u64,
}

/// What a [`AudioPlayer`] gets from its behaviour on every change of its track.
pub struct AudioUpdate {
    snapshot: Arc<AudioSnapshot>,
    /// Room for the place of every clip, made here so the audio thread never grows it.
    spans: Vec<Span>,
}

impl AudioUpdate {
    pub fn new(snapshot: AudioSnapshot) -> Self {
        Self {
            spans: Vec::with_capacity(snapshot.len()),
            snapshot: Arc::new(snapshot),
        }
    }
}

/// The clip heard on the last frame of a block, its frame after that one, its length, and
/// where its heads are there when it has warp points.
#[derive(Copy, Clone)]
struct Sounding {
    clip: usize,
    next: u64,
    length: u64,
    heads: Heads,
}

pub struct AudioPlayer {
    snapshot: Arc<AudioSnapshot>,
    spans: Vec<Span>,
    /// Frames of the ramp at every edge.
    ramp: u64,
    /// File frames on their way through the resampler.
    scratch: Box<[[f32; 2]]>,
    /// One block of one clip.
    piece: Box<[[f32; 2]]>,
    /// What sounded, going on past a jump and fading out. One ramp long.
    tail: Box<[[f32; 2]]>,
    /// The next frame of `tail` to play: its length when it is done.
    tail_at: usize,
    /// Where a new tail is made, one ramp long.
    going_on: Box<[[f32; 2]]>,
    /// Frames since the sound came in after the last jump.
    entered: u64,
    last: Option<Sounding>,
    /// The project frame after the last block, when it played.
    played_to: Option<u64>,
    /// A new snapshot came since the last block.
    changed: bool,
}

impl AudioPlayer {
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub fn new() -> Self {
        Self {
            snapshot: Arc::default(),
            spans: Vec::new(),
            ramp: 0,
            scratch: vec![[0.0; 2]; SCRATCH_FRAMES].into_boxed_slice(),
            piece: vec![[0.0; 2]; MAX_BLOCK].into_boxed_slice(),
            tail: Box::default(),
            tail_at: 0,
            going_on: Box::default(),
            entered: u64::MAX,
            last: None,
            played_to: None,
            changed: false,
        }
    }

    /// What is heard now goes on for one ramp from where it was, fading out: the clip that
    /// sounded, still coming in if it came in after a jump just now, and what is left of a
    /// tail of an earlier jump.
    fn start_tail(&mut self) {
        let sounding = self.last.take();
        let clip =
            sounding.and_then(|sounding| Some((sounding, self.snapshot.clips.get(sounding.clip)?)));
        match clip {
            Some((sounding, clip)) => {
                let whole = Part {
                    from: 0,
                    to: sounding.length,
                    hands_over: false,
                };
                let first = (sounding.next, self.entered);
                clip.write(
                    Reading::On(sounding.heads),
                    first,
                    (whole, sounding.length),
                    self.ramp,
                    &mut self.going_on,
                    &mut self.scratch,
                );
            }
            None => self.going_on.fill([0.0; 2]),
        }
        // The rest of the old tail is read from ahead of where the new one is written, so one
        // pass over the one buffer is enough.
        let steps = (self.ramp + 1) as f32;
        for index in 0..self.tail.len() {
            let left_over = self
                .tail
                .get(self.tail_at + index)
                .copied()
                .unwrap_or_default();
            let going_on = self.going_on.get(index).copied().unwrap_or_default();
            let level = (self.ramp as f32 - index as f32) / steps;
            if let Some(frame) = self.tail.get_mut(index) {
                *frame = [0, 1].map(|channel| (going_on[channel] + left_over[channel]) * level);
            }
        }
        self.tail_at = 0;
    }

    /// Adds, to frames `range` of a block, the first of which is at `offset` in the block, what
    /// the clips that hand over play past their edge.
    fn play_past_hand_overs(
        &mut self,
        clock: &Clock,
        range: std::ops::Range<u64>,
        offset: usize,
        outputs: [&mut [f32]; 2],
    ) {
        let [left, right] = outputs;
        let ramp = self.ramp;
        for (index, span) in self.spans.iter().enumerate() {
            for edge in [span.start, span.end] {
                // The frames a hand-over at this edge plays past it must meet this block.
                if edge == 0 || edge >= range.end || edge + ramp <= range.start {
                    continue;
                }
                // Once for each edge, which several clips may share.
                let before = self.spans.get(..index).unwrap_or_default();
                let seen = before
                    .iter()
                    .any(|other| other.start == edge || other.end == edge);
                if seen || (edge == span.end && span.start == edge) {
                    continue;
                }
                let Some((outgoing, part)) = hand_over(&self.spans, edge, ramp) else {
                    continue;
                };
                let (Some(clip), Some(outgoing_span)) =
                    (self.snapshot.clips.get(outgoing), self.spans.get(outgoing))
                else {
                    continue;
                };
                let from = edge.max(range.start);
                let to = (edge + ramp).min(range.end);
                let Some(piece) = self.piece.get_mut(..(to - from) as usize) else {
                    continue;
                };
                let entered = self.entered.saturating_add(from - range.start);
                let first = (from - outgoing_span.start, entered);
                let length = outgoing_span.end - outgoing_span.start;
                let reading = Reading::Clock(clock);
                clip.write(
                    reading,
                    first,
                    (part, length),
                    ramp,
                    piece,
                    &mut self.scratch,
                );
                let at = offset + (from - range.start) as usize;
                let outputs = left.iter_mut().zip(right.iter_mut()).skip(at);
                for ((left, right), sample) in outputs.zip(piece.iter()) {
                    *left += sample[0];
                    *right += sample[1];
                }
            }
        }
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

/// The clip heard at `frame`, the top one of those that hold it, and the next frame where that
/// may change, at most `limit`.
fn heard_at(spans: &[Span], frame: u64, limit: u64) -> (Option<usize>, u64) {
    let owner = spans
        .iter()
        .rposition(|span| span.start <= frame && frame < span.end);
    let edges = spans.iter().flat_map(|span| [span.start, span.end]);
    let next = edges.filter(|edge| *edge > frame).min().unwrap_or(limit);
    (owner, next.min(limit))
}

/// The part of clip `owner` that is heard around `frame`, in frames of the clip: from where the
/// last clip above it stopped covering it, to where the next one starts.
fn heard_part(spans: &[Span], owner: usize, frame: u64) -> (u64, u64) {
    let Some(span) = spans.get(owner) else {
        return (0, 0);
    };
    let above = spans.get(owner + 1..).unwrap_or_default();
    let from = above
        .iter()
        .filter(|other| other.end <= frame)
        .map(|other| other.end)
        .fold(span.start, u64::max);
    let to = above
        .iter()
        .filter(|other| other.start > frame)
        .map(|other| other.start)
        .fold(span.end, u64::min);
    (from - span.start, to - span.start)
}

/// Whether one clip hands over to another at project frame `edge`, with file enough to play
/// on past it for one ramp: the clip heard before the edge and its part, which then ends in
/// a crossfade.
fn hand_over(spans: &[Span], edge: u64, ramp: u64) -> Option<(usize, Part)> {
    let before = edge.checked_sub(1)?;
    let (outgoing, _) = heard_at(spans, before, edge);
    let (incoming, _) = heard_at(spans, edge, edge + 1);
    let (outgoing, incoming) = (outgoing?, incoming?);
    if outgoing == incoming {
        return None;
    }
    let span = spans.get(outgoing)?;
    let (from, to) = heard_part(spans, outgoing, before);
    if span.start + to != edge || to + ramp > span.available {
        return None;
    }
    let hands_over = true;
    Some((
        outgoing,
        Part {
            from,
            to,
            hands_over,
        },
    ))
}

impl Processor for AudioPlayer {
    type Update = AudioUpdate;

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.ramp = (DECLICK_SECONDS * f64::from(config.sample_rate))
            .round()
            .max(1.0) as u64;
        self.tail = vec![[0.0; 2]; self.ramp as usize].into_boxed_slice();
        self.going_on = vec![[0.0; 2]; self.ramp as usize].into_boxed_slice();
        self.tail_at = self.tail.len();
    }

    fn update(&mut self, update: &mut AudioUpdate) {
        // The old snapshot is still here: what it played fades out from where it was.
        if self.last.is_some() {
            self.start_tail();
        }
        // The old ones ride back to the control thread inside the update.
        std::mem::swap(&mut self.snapshot, &mut update.snapshot);
        std::mem::swap(&mut self.spans, &mut update.spans);
        self.changed = true;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let frames = context.frames;
        let transport = &context.transport;
        let range = transport.frame_range.start.0..transport.frame_range.end.0;
        let playing = transport.playing && range.start < range.end;
        let continues =
            playing && !self.changed && !transport.jumped && self.played_to == Some(range.start);
        if !continues {
            if self.last.is_some() {
                self.start_tail();
            }
            self.entered = 0;
        }
        self.changed = false;
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);

        self.last = None;
        self.played_to = None;
        if playing {
            self.spans.clear();
            for clip in &self.snapshot.clips {
                let start = transport.clock.frame_of(clip.start).0;
                let (length, available) = clip.bounds(transport.clock);
                let end = start.saturating_add(length);
                // The update made room for every clip, so this never grows the list.
                if self.spans.len() < self.spans.capacity() {
                    self.spans.push(Span {
                        start,
                        end,
                        available,
                    });
                }
            }
            // Where `range` starts in the block. Right after a play in a project with latency
            // it starts later than the block, whose first frame may then be before frame 0.
            let offset = frames.saturating_sub((range.end - range.start) as usize);
            let mut frame = range.start;
            while frame < range.end {
                let (owner, next) = heard_at(&self.spans, frame, range.end);
                let clip = owner.and_then(|owner| Some((owner, self.snapshot.clips.get(owner)?)));
                if let (Some((owner, clip)), Some(span)) =
                    (clip, owner.and_then(|owner| self.spans.get(owner)))
                {
                    let (from, to) = heard_part(&self.spans, owner, frame);
                    let hands_over = hand_over(&self.spans, span.start + to, self.ramp)
                        .is_some_and(|(outgoing, _)| outgoing == owner);
                    let part = Part {
                        from,
                        to,
                        hands_over,
                    };
                    let count = (next - frame) as usize;
                    if let Some(piece) = self.piece.get_mut(..count) {
                        let entered = self.entered.saturating_add(frame - range.start);
                        let first = (frame - span.start, entered);
                        let length = span.end - span.start;
                        let reading = Reading::Clock(transport.clock);
                        let (ramp, scratch) = (self.ramp, &mut self.scratch);
                        clip.write(reading, first, (part, length), ramp, piece, scratch);
                        let at = offset + (frame - range.start) as usize;
                        let outputs = left.iter_mut().zip(right.iter_mut()).skip(at);
                        for ((left, right), sample) in outputs.zip(piece.iter()) {
                            *left = sample[0];
                            *right = sample[1];
                        }
                    }
                }
                frame = next;
            }
            let outputs = [&mut *left, &mut *right];
            self.play_past_hand_overs(transport.clock, range.clone(), offset, outputs);
            self.entered = self.entered.saturating_add(range.end - range.start);
            let (owner, _) = heard_at(&self.spans, range.end - 1, range.end);
            self.last = owner.and_then(|clip| {
                let span = self.spans.get(clip)?;
                let next = range.end - span.start;
                let warp = self.snapshot.clips.get(clip)?.warp.as_ref();
                Some(Sounding {
                    clip,
                    next,
                    length: span.end - span.start,
                    heads: warp
                        .map_or_else(Heads::default, |warp| warp.heads_at(transport.clock, next)),
                })
            });
            self.played_to = Some(range.end);
        }

        // What sounded before a jump, fading out.
        let tail = self.tail.get(self.tail_at..).unwrap_or_default();
        let count = tail.len().min(frames);
        for ((left, right), sample) in left.iter_mut().zip(right.iter_mut()).zip(tail) {
            *left += sample[0];
            *right += sample[1];
        }
        self.tail_at += count;
    }
}
