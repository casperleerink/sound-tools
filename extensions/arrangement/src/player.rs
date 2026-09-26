//! Plays the audio clips of one track: one immutable snapshot of its clips on the control side,
//! one processor that reads them from the transport's frame range on the audio thread.
//!
//! The rules that shape this file:
//!
//! - Nothing reads a file here. Every file is in memory, in the snapshot, before it arrives.
//! - Where clips overlap, the one that comes last in the snapshot is heard (see
//!   [`crate::AudioClip::layer`]). The others are not changed, only not heard there.
//! - Every edge of what is heard gets a short ramp, [`DECLICK_SECONDS`]: the start and end of
//!   each clip, and where a clip is covered and uncovered. So no edge clicks, whatever the
//!   file holds there, and a fade of 0 ms is this ramp alone.
//! - Where the sound would jump, because of a seek, a stop, a tempo change or an edit while it
//!   plays, the old sound fades out over the same ramp while the new one fades in.

use std::sync::Arc;

use sound_core::{AudioOutput, MAX_BLOCK, Ports, PrepareConfig, ProcessContext, Processor, Ticks};
use sound_media::{Audio, Resampler, SCRATCH_FRAMES};

/// How long the ramp at every edge of a clip is. Not a setting, and not drawn.
pub const DECLICK_SECONDS: f64 = 0.002;

/// One clip as the audio thread plays it: its file, where it starts, and its level, all in
/// engine frames except the start.
pub struct PlacedAudio {
    pub(crate) start: Ticks,
    pub(crate) audio: Arc<Audio>,
    pub(crate) resampler: Arc<Resampler>,
    /// The file frame the clip starts playing at.
    pub(crate) origin: u64,
    /// Engine frames the clip plays, 1 or more.
    pub(crate) length: u64,
    pub(crate) gain: f32,
    pub(crate) fade_in: u64,
    pub(crate) fade_out: u64,
}

impl PlacedAudio {
    /// The level of frame `frame` of the clip, heard in its part `from..to`: the gain, the
    /// fades, and the ramp at each edge of that part.
    fn level(&self, frame: u64, (from, to): (u64, u64), ramp: u64) -> f32 {
        if frame < from || frame >= to {
            return 0.0;
        }
        let ramp = (ramp + 1) as f64;
        let rising = (frame - from + 1) as f64 / ramp;
        let falling = (to - frame) as f64 / ramp;
        let mut level = rising.min(falling).min(1.0);
        if self.fade_in > 0 {
            level = level.min(frame as f64 / self.fade_in as f64);
        }
        if self.fade_out > 0 {
            level = level.min((self.length - frame) as f64 / self.fade_out as f64);
        }
        (level * f64::from(self.gain)) as f32
    }

    /// Frames `first..` of the clip as heard in its part `part`, into `out`.
    fn write(
        &self,
        first: u64,
        part: (u64, u64),
        ramp: u64,
        out: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
    ) {
        self.resampler
            .render(&self.audio, self.origin, first, out, scratch);
        for (index, frame) in out.iter_mut().enumerate() {
            let level = self.level(first + index as u64, part, ramp);
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
    pub(crate) fn new(clips: Vec<PlacedAudio>) -> Self {
        Self { clips }
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

/// Where a clip is in project frames for one block, under the clock of that block.
#[derive(Copy, Clone, Default)]
struct Span {
    start: u64,
    end: u64,
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

/// The clip heard on the last frame of a block, and its frame after that one.
#[derive(Copy, Clone)]
struct Sounding {
    clip: usize,
    next: u64,
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
    /// What sounded, going on past a jump and fading out.
    tail: Box<[[f32; 2]]>,
    /// The next frame of `tail` to play: its length when it is done.
    tail_at: usize,
    /// Frames of the fade-in after a jump done so far: `ramp` when it is done.
    entry_at: u64,
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
            entry_at: 0,
            last: None,
            played_to: None,
            changed: false,
        }
    }

    /// What was heard goes on for one ramp from where it was, fading out.
    fn start_tail(&mut self, sounding: Sounding) {
        let Some(clip) = self.snapshot.clips.get(sounding.clip) else {
            return;
        };
        let whole = (0, clip.length);
        clip.write(
            sounding.next,
            whole,
            self.ramp,
            &mut self.tail,
            &mut self.scratch,
        );
        let ramp = (self.ramp + 1) as f32;
        for (index, frame) in self.tail.iter_mut().enumerate() {
            let level = (self.ramp as f32 - index as f32) / ramp;
            *frame = frame.map(|sample| sample * level);
        }
        self.tail_at = 0;
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
        self.tail_at = self.tail.len();
        self.entry_at = self.ramp;
    }

    fn update(&mut self, update: &mut AudioUpdate) {
        // The old snapshot is still here: what it played fades out from where it was.
        if let Some(sounding) = self.last.take() {
            self.start_tail(sounding);
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
            if let Some(sounding) = self.last.take() {
                self.start_tail(sounding);
            }
            self.entry_at = 0;
        }
        self.changed = false;
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);

        self.last = None;
        self.played_to = None;
        if playing {
            self.spans.clear();
            for clip in &self.snapshot.clips {
                let start = transport.clock.frame_of(clip.start).0;
                let end = start.saturating_add(clip.length);
                // The update made room for every clip, so this never grows the list.
                if self.spans.len() < self.spans.capacity() {
                    self.spans.push(Span { start, end });
                }
            }
            // The offset in this block of a project frame of `range`. Right after a play in a
            // project with latency, `range` may start later than the block.
            let offset = |frame: u64| (frame + frames as u64 - range.end) as usize;
            let mut frame = range.start;
            while frame < range.end {
                let (owner, next) = heard_at(&self.spans, frame, range.end);
                let clip = owner.and_then(|owner| Some((owner, self.snapshot.clips.get(owner)?)));
                if let (Some((owner, clip)), Some(span)) =
                    (clip, owner.and_then(|owner| self.spans.get(owner)))
                {
                    let part = heard_part(&self.spans, owner, frame);
                    let count = (next - frame) as usize;
                    if let Some(piece) = self.piece.get_mut(..count) {
                        clip.write(
                            frame - span.start,
                            part,
                            self.ramp,
                            piece,
                            &mut self.scratch,
                        );
                        let at = offset(frame);
                        let outputs = left.iter_mut().zip(right.iter_mut()).skip(at);
                        for ((left, right), sample) in outputs.zip(piece.iter()) {
                            *left = sample[0];
                            *right = sample[1];
                        }
                    }
                }
                frame = next;
            }
            // After a jump the new sound comes in along the ramp.
            let outputs = left
                .iter_mut()
                .zip(right.iter_mut())
                .skip(offset(range.start));
            for (left, right) in outputs {
                if self.entry_at >= self.ramp {
                    break;
                }
                let level = (self.entry_at + 1) as f32 / (self.ramp + 1) as f32;
                *left *= level;
                *right *= level;
                self.entry_at += 1;
            }
            let (owner, _) = heard_at(&self.spans, range.end - 1, range.end);
            self.last = owner.and_then(|clip| {
                let span = self.spans.get(clip)?;
                Some(Sounding {
                    clip,
                    next: range.end - span.start,
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
