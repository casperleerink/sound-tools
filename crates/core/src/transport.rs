//! The transport: whether the project plays and where the project position is.
//!
//! The state lives on the audio thread. The control side changes it with messages and reads it
//! back through the engine status.
//!
//! The position is what the device plays. A processor whose output reaches the device later,
//! because something after it has latency, sees its blocks that much earlier on the timeline:
//! its lead. So a track with latency plays ahead and reaches the device in time with the rest.

use std::ops::Range;
use std::sync::Arc;

use crate::clock::{Clock, Frames, Ticks};

/// What a processor sees of the transport for one block.
///
/// While playing, the blocks of a processor cover the project timeline without gaps or
/// overlaps: each `tick_range` starts where the previous one ended, until `jumped` says
/// otherwise. So a processor that emits what starts inside `tick_range` emits everything
/// exactly once. While not playing both ranges are empty, so nothing new starts. What already
/// sounds is the processor's to end: release held notes when `stopped_playing` or `jumped`.
#[derive(Clone, Debug)]
pub struct Transport<'a> {
    pub playing: bool,
    /// The previous block played and this one does not: a pause or a stop. Set for one block.
    /// The ranges are empty from here on, so release held notes now or they sound forever.
    pub stopped_playing: bool,
    /// The position moved by a seek, a stop or a loop back since the previous block, or the
    /// latency after this processor changed, which moves where its blocks are, or a play from
    /// rest in a project with latency. Set for one block. `EngineStatus::jumps` counts only
    /// seeks, stops and loop backs. Nothing
    /// between the old and the new position is replayed. Each processor decides what to do,
    /// for example release its held notes.
    pub jumped: bool,
    /// The project frames this block covers, ahead of what the device plays by the latency of
    /// everything after this processor. As long as the block while playing, except right after
    /// a play or a seek in a project with latency: then it starts where playback starts, and is
    /// empty or short until the device has caught up. A tempo map change moves the frame
    /// position, because it keeps the tick position.
    pub frame_range: Range<Frames>,
    /// The ticks that land on a frame of this block.
    pub tick_range: Range<Ticks>,
    /// The tick the device plays at the start of this block, the same for every processor.
    /// It is `tick_range` without the latency after this processor. Something that arrives
    /// live, such as a key, is stamped with this: it is where the player heard the project.
    pub heard_tick: Ticks,
    /// For any other conversion, for example the tempo at a tick.
    pub clock: &'a Clock,
    /// The project frame of the first frame of this block. Before `frame_range.start` while
    /// the device has not caught up after a play or a seek, and then possibly below zero.
    block_start: i128,
}

impl Transport<'_> {
    /// The frame offset inside this block at which `tick` lands, ready for
    /// `event_outputs.push`. `None` when the tick is not in `tick_range`.
    pub fn offset_of(&self, tick: Ticks) -> Option<usize> {
        if !self.tick_range.contains(&tick) {
            return None;
        }
        let frame = i128::from(self.clock.frame_of(tick).0);
        // A tick before the block only after a tempo map change, for a processor ahead of the
        // device: it plays at once, late, rather than never.
        usize::try_from((frame - self.block_start).max(0)).ok()
    }

    /// The tick the frame `offset` into this block plays, for what lands inside a block.
    pub fn tick_at_offset(&self, offset: usize) -> Ticks {
        let frame = (self.block_start + offset as i128).max(i128::from(self.frame_range.start.0));
        self.clock
            .tick_at(Frames(u64::try_from(frame).unwrap_or(u64::MAX)))
    }

    /// Where the first frame of this block is in quarter notes from the project start, with
    /// the part of a tick it falls inside. `None` while stopped.
    ///
    /// While the device catches up after a play or a seek, the block starts before playback
    /// does: counted back from where playback starts at the tempo there, so the project frames
    /// of the block are in the right place. It may be below zero.
    pub fn quarters(&self) -> Option<f64> {
        if !self.playing {
            return None;
        }
        let start = self.frame_range.start;
        let early = (i128::from(start.0) - self.block_start) as f64;
        let bpm = self.clock.tempo_at(self.tick_range.start).bpm();
        let quarters_per_frame = bpm / 60.0 / f64::from(self.clock.sample_rate());
        Some(self.clock.quarters_at(start) - early * quarters_per_frame)
    }
}

/// The transport operations, as messages to the audio thread.
#[derive(Clone, Debug)]
pub(crate) enum TransportCommand {
    Play,
    Pause,
    Seek(Ticks),
    /// Once playback reaches `at`, it goes on from `to`. Replaces one that waits; a seek drops it.
    JumpAt {
        at: Ticks,
        to: Ticks,
    },
    Loop(Option<Range<Ticks>>),
}

pub(crate) struct TransportState {
    playing: bool,
    /// Whether the previous block played. Compared per block, not per command, so a pause and
    /// a play that land in the same block are no stop: that block goes on where the last ended.
    was_playing: bool,
    jumped: bool,
    /// The device reached the end of the loop or the tick of a jump since the last block. The
    /// processors ahead of it went on from there on their own frame already, so only those
    /// that are not ahead see a jump.
    wrapped: bool,
    /// The project frame the device plays next. While the device waits after a play or a seek
    /// (`preroll`), the frame playback starts from.
    position: Frames,
    /// Frames the device still plays before it reaches `position`: after a play or a seek, the
    /// longest latency, so that the processor furthest ahead starts exactly at `position` and
    /// nothing of any track is skipped.
    preroll: u64,
    /// A jump, or a play from rest, whose preroll is set at the next block, once the latency
    /// of the processors that came with the same batch is known.
    preroll_due: bool,
    /// A play from rest since the last block.
    started: bool,
    /// This block resumes after a play from rest in a project with latency. Processors see it
    /// as a jump, but it is no seek: the engine status does not count it, so a view or a take
    /// that ends on a jump is not ended by pressing play.
    resumed: bool,
    /// The longest latency from any processor to the device.
    latency: u64,
    /// The tick of `position` at the start of the block, for [`Transport::heard_tick`].
    heard_tick: Ticks,
    /// When the device reaches the end of this range, playback seeks to its start. Never empty.
    looped: Option<Range<Ticks>>,
    /// Where playback goes on from once it reaches a tick, once.
    jump_at: Option<(Ticks, Ticks)>,
    clock: Arc<Clock>,
}

impl TransportState {
    pub(crate) fn new(clock: Arc<Clock>) -> Self {
        Self {
            playing: false,
            was_playing: false,
            jumped: false,
            wrapped: false,
            position: Frames(0),
            preroll: 0,
            preroll_due: false,
            started: false,
            resumed: false,
            latency: 0,
            heard_tick: Ticks(0),
            looped: None,
            jump_at: None,
            clock,
        }
    }

    pub(crate) fn apply(&mut self, command: &TransportCommand) {
        match command {
            TransportCommand::Play => {
                self.started |= !self.playing;
                self.playing = true;
            }
            TransportCommand::Pause => self.playing = false,
            TransportCommand::Seek(tick) => {
                self.jump_at = None;
                self.jump_to(self.clock.frame_of(*tick));
            }
            // A tick playback has already passed jumps at once.
            TransportCommand::JumpAt { at, to } => {
                match self.clock.frame_of(*at) <= self.position {
                    true => {
                        self.jump_at = None;
                        self.jump_to(self.clock.frame_of(*to));
                    }
                    false => self.jump_at = Some((*at, *to)),
                }
            }
            TransportCommand::Loop(range) => {
                self.looped = range.clone().filter(|range| !range.is_empty());
            }
        }
    }

    fn jump_to(&mut self, position: Frames) {
        self.position = position;
        self.jumped = true;
        self.preroll_due = true;
    }

    /// Swaps the clocks, so the old one can travel back to the control side. Keeps the musical
    /// position: the next block starts with the same tick as it would have with the old clock.
    pub(crate) fn swap_clock(&mut self, clock: &mut Arc<Clock>) {
        let next_tick = self.clock.tick_at(self.position);
        std::mem::swap(clock, &mut self.clock);
        self.position = self.clock.frame_of(next_tick);
    }

    /// The longest latency from any processor to the device, found after every batch.
    pub(crate) fn set_latency(&mut self, latency: u64) {
        self.latency = latency;
    }

    pub(crate) fn latency(&self) -> u64 {
        self.latency
    }

    /// Frames the device plays before playback reaches the end of the loop or the tick of a
    /// jump, so a sub-block can end there. `None` with neither, while stopped, and from such a
    /// tick on: a seek past it plays on. Call after [`Self::begin_block`], which sets the preroll.
    pub(crate) fn frames_to_jump(&self) -> Option<u64> {
        if !self.playing {
            return None;
        }
        let ends = (self.looped.as_ref().map(|looped| looped.end))
            .into_iter()
            .chain(self.jump_at.map(|(at, _)| at));
        let left = ends
            .filter_map(|end| {
                let end = self.clock.frame_of(end);
                end.0.checked_sub(self.position.0).filter(|left| *left > 0)
            })
            .min()?;
        Some(left.saturating_add(self.preroll))
    }

    /// A seek, a stop or a loop back since the last block: what the engine status counts as a
    /// jump.
    pub(crate) fn seek_or_stop(&self) -> bool {
        self.jumped || self.wrapped
    }

    /// The next jump the device makes without a seek, while playing: the frame it is at, the
    /// frame it goes on from, and for the loop its length, as it goes back again and again.
    fn next_jump(&self) -> Option<(u64, u64, Option<u64>)> {
        if !self.playing {
            return None;
        }
        let ahead = |at: Ticks| {
            let at = self.clock.frame_of(at).0;
            (at > self.position.0).then_some(at)
        };
        let jump =
            (self.jump_at).and_then(|(at, to)| Some((ahead(at)?, self.clock.frame_of(to).0, None)));
        let looped = self.looped.as_ref().and_then(|looped| {
            let (at, to) = (ahead(looped.end)?, self.clock.frame_of(looped.start).0);
            Some((at, to, at.checked_sub(to).filter(|length| *length > 0)))
        });
        [jump, looped]
            .into_iter()
            .flatten()
            .min_by_key(|(at, ..)| *at)
    }

    /// Where the frame `frame` of a processor ahead of the device plays: past the next jump
    /// of the device it goes on from where the device will, as the device will when it gets
    /// there. `end` keeps a frame right on the jump before it, for the end of a range.
    fn played_at(&self, frame: u64, end: bool) -> u64 {
        let Some((at, to, length)) = self.next_jump() else {
            return frame;
        };
        let over = match frame.checked_sub(at) {
            Some(over) if !(end && over == 0) => over,
            _ => return frame,
        };
        match length {
            Some(length) if end && over % length == 0 => to + length,
            Some(length) => to + over % length,
            None => to + over,
        }
    }

    /// Whether a processor ahead of the device that starts a block at `frame` goes on from
    /// where the device will jump to, right there.
    fn on_jump(&self, frame: u64) -> bool {
        let Some((at, _, length)) = self.next_jump() else {
            return false;
        };
        match (frame.checked_sub(at), length) {
            (Some(0), _) => true,
            (Some(over), Some(length)) => over % length == 0,
            _ => false,
        }
    }

    /// Frames from `from` into the next block of a processor `lead` frames ahead until it
    /// reaches the next jump of the device, when that is inside `frames`: its block ends
    /// there, so the rest plays from where the device goes on.
    pub(crate) fn frames_to_wrap(&self, from: usize, frames: usize, lead: u64) -> Option<usize> {
        let (at, _, length) = self.next_jump()?;
        let lead = lead.saturating_add(from as u64);
        let frame = self.position.0 + lead.saturating_sub(self.preroll);
        let left = match frame.checked_sub(at) {
            None => at - frame,
            Some(over) => length? - over % length?,
        };
        usize::try_from(left)
            .ok()
            .filter(|left| *left > 0 && *left < frames)
    }

    /// Call once per sub-block, before any [`Self::view`].
    ///
    /// A play from rest in a project with latency starts like a seek to where the project
    /// stands: the tracks with latency went quiet when it stopped, and would otherwise miss
    /// their first notes. Without latency it is what it always was, with no jump.
    pub(crate) fn begin_block(&mut self) {
        if std::mem::take(&mut self.started) && self.playing && self.latency > 0 {
            self.resumed = true;
            self.preroll_due = true;
        }
        if std::mem::take(&mut self.preroll_due) {
            self.preroll = self.latency;
        }
        self.heard_tick = self.clock.tick_at(self.position);
    }

    /// Whether the project plays.
    pub(crate) fn playing(&self) -> bool {
        self.playing
    }

    /// The first tick the next block of a processor `lead` frames ahead would start on with
    /// the clock as it is. Read before a tempo map change, see [`Self::view`].
    pub(crate) fn next_tick(&self, lead: u64) -> Ticks {
        let past_preroll = lead.saturating_sub(self.preroll);
        self.clock
            .tick_at(Frames(self.position.0.saturating_add(past_preroll)))
    }

    /// The view for the next block of `frames` frames of a processor `lead` frames ahead of
    /// the device, `from` frames into the sub-block: a processor may play one in pieces, and
    /// only the first piece sees a jump or a stop. `moved` says its lead changed since its
    /// last block, which it sees as a jump.
    ///
    /// `resume_from` is the tick after the last one this processor was given, when a tempo map
    /// change moved where its blocks are. The clock keeps the tick the device plays, not the
    /// ticks of a processor ahead of it, so without this such a processor would skip ticks
    /// after a faster tempo and get some twice after a slower one. Its range starts there
    /// instead: ticks it would have skipped land at offset 0, late by less than its lead, and
    /// after a slower tempo it gets an empty range until the clock has caught up.
    pub(crate) fn view(
        &self,
        from: usize,
        frames: usize,
        lead: u64,
        moved: bool,
        resume_from: Option<Ticks>,
    ) -> Transport<'_> {
        let first = from == 0;
        let played = if self.playing { from as u64 } else { 0 };
        let lead = lead.saturating_add(played);
        let advance = if self.playing { frames as u64 } else { 0 };
        // What the device has not caught up with yet comes off the front, so no range starts
        // before `position`: nothing earlier than where playback starts is played.
        let ahead = |frames: u64| {
            let past_preroll = frames.saturating_sub(self.preroll);
            Frames(self.position.0.saturating_add(past_preroll))
        };
        let (start, end) = (ahead(lead), ahead(lead.saturating_add(advance)));
        // Right on the next jump of the device, a processor ahead of it goes on from there.
        let wraps = self.on_jump(start.0);
        let (start, end) = (
            Frames(self.played_at(start.0, false)),
            Frames(self.played_at(end.0, true)),
        );
        let led = lead.saturating_sub(self.preroll) > 0;
        let jumped = (first && (self.jumped || self.resumed || moved || (self.wrapped && !led)))
            || (wraps && self.playing);
        // Both ends come from the same function, so the end of this block is the start of the
        // next, whatever the block sizes and tempo changes are.
        let mut tick_range = self.clock.tick_at(start)..self.clock.tick_at(end);
        if let Some(from) = resume_from.filter(|_| !jumped) {
            tick_range = from..tick_range.end.max(from);
        }
        Transport {
            playing: self.playing,
            stopped_playing: first && self.was_playing && !self.playing,
            jumped,
            tick_range,
            frame_range: start..end,
            heard_tick: self.heard_tick,
            clock: &self.clock,
            block_start: i128::from(start.0) - i128::from(self.preroll.saturating_sub(lead)),
        }
    }

    /// Call after the block ran, with the frames it had. Returns how many of them the device
    /// played while it waited after a play or a seek.
    ///
    /// A block that reaches the end of the loop goes on from its start, also one that starts
    /// right on it, where a tempo change can put the position; the same for the tick of a
    /// jump. The processors ahead of the device went on from there on their own frame, so
    /// nothing waits and nothing is played twice.
    pub(crate) fn finish_block(&mut self, frames: usize) -> u64 {
        let advance = if self.playing { frames as u64 } else { 0 };
        let waited = advance.min(self.preroll);
        self.preroll -= waited;
        let before = self.position;
        self.position = Frames(self.position.0.saturating_add(advance - waited));
        self.was_playing = self.playing;
        self.jumped = false;
        self.wrapped = false;
        self.resumed = false;
        let reached = |tick: Ticks| {
            let end = self.clock.frame_of(tick);
            before <= end && self.position >= end && self.position > before
        };
        // The processors ahead of the device went on from there already: no seek, no wait.
        if let Some((_, to)) = self.jump_at.filter(|(at, _)| reached(*at)) {
            self.jump_at = None;
            self.position = self.clock.frame_of(to);
            self.wrapped = true;
        } else if let Some(looped) = self.looped.clone().filter(|looped| reached(looped.end)) {
            self.position = self.clock.frame_of(looped.start);
            self.wrapped = true;
        }
        waited
    }
}
