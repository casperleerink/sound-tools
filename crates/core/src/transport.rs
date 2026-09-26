//! The transport: whether the project plays and where the project position is.
//!
//! The state lives on the audio thread. The control side changes it with messages and reads it
//! back through the engine status.
//!
//! The position is what the device plays. A processor whose output reaches the device later,
//! because something after it has latency, sees its blocks that much earlier on the timeline:
//! its lead. So a track with latency plays ahead and reaches the device in time with the rest.
//! See ARCHITECTURE.md, "Latency compensation".

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
    /// The position moved by a seek or a stop since the previous block, or the latency after
    /// this processor changed, which moves where its blocks are, or a play from rest in a
    /// project with latency. Set for one block. `EngineStatus::jumps` counts only seeks and stops. Nothing
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
}

/// The transport operations of ARCHITECTURE.md, as messages to the audio thread.
#[derive(Copy, Clone, Debug)]
pub(crate) enum TransportCommand {
    Play,
    Pause,
    Stop,
    Seek(Ticks),
}

pub(crate) struct TransportState {
    playing: bool,
    /// Whether the previous block played. Compared per block, not per command, so a pause and
    /// a play that land in the same block are no stop: that block goes on where the last ended.
    was_playing: bool,
    jumped: bool,
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
    clock: Arc<Clock>,
}

impl TransportState {
    pub(crate) fn new(clock: Arc<Clock>) -> Self {
        Self {
            playing: false,
            was_playing: false,
            jumped: false,
            position: Frames(0),
            preroll: 0,
            preroll_due: false,
            started: false,
            resumed: false,
            latency: 0,
            heard_tick: Ticks(0),
            clock,
        }
    }

    pub(crate) fn apply(&mut self, command: TransportCommand) {
        match command {
            TransportCommand::Play => {
                self.started |= !self.playing;
                self.playing = true;
            }
            TransportCommand::Pause => self.playing = false,
            TransportCommand::Stop => {
                self.playing = false;
                self.jump_to(Frames(0));
            }
            TransportCommand::Seek(tick) => self.jump_to(self.clock.frame_of(tick)),
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

    /// A seek or a stop since the last block: what the engine status counts as a jump.
    pub(crate) fn seek_or_stop(&self) -> bool {
        self.jumped
    }

    /// Call once per block, before any [`Self::view`].
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
    /// the device. `moved` says its lead changed since its last block, which it sees as a jump.
    ///
    /// `resume_from` is the tick after the last one this processor was given, when a tempo map
    /// change moved where its blocks are. The clock keeps the tick the device plays, not the
    /// ticks of a processor ahead of it, so without this such a processor would skip ticks
    /// after a faster tempo and get some twice after a slower one. Its range starts there
    /// instead: ticks it would have skipped land at offset 0, late by less than its lead, and
    /// after a slower tempo it gets an empty range until the clock has caught up.
    pub(crate) fn view(
        &self,
        frames: usize,
        lead: u64,
        moved: bool,
        resume_from: Option<Ticks>,
    ) -> Transport<'_> {
        let advance = if self.playing { frames as u64 } else { 0 };
        // What the device has not caught up with yet comes off the front, so no range starts
        // before `position`: nothing earlier than where playback starts is played.
        let ahead = |frames: u64| {
            let past_preroll = frames.saturating_sub(self.preroll);
            Frames(self.position.0.saturating_add(past_preroll))
        };
        let start = ahead(lead);
        let end = ahead(lead.saturating_add(advance));
        let jumped = self.jumped || self.resumed || moved;
        // Both ends come from the same function, so the end of this block is the start of the
        // next, whatever the block sizes and tempo changes are.
        let mut tick_range = self.clock.tick_at(start)..self.clock.tick_at(end);
        if let Some(from) = resume_from.filter(|_| !jumped) {
            tick_range = from..tick_range.end.max(from);
        }
        Transport {
            playing: self.playing,
            stopped_playing: self.was_playing && !self.playing,
            jumped,
            tick_range,
            frame_range: start..end,
            heard_tick: self.heard_tick,
            clock: &self.clock,
            block_start: i128::from(self.position.0) + i128::from(lead) - i128::from(self.preroll),
        }
    }

    /// Call after the block ran, with the frames it had. Returns how many of them the device
    /// played while it waited after a play or a seek.
    pub(crate) fn finish_block(&mut self, frames: usize) -> u64 {
        let advance = if self.playing { frames as u64 } else { 0 };
        let waited = advance.min(self.preroll);
        self.preroll -= waited;
        self.position = Frames(self.position.0.saturating_add(advance - waited));
        self.was_playing = self.playing;
        self.jumped = false;
        self.resumed = false;
        waited
    }
}
