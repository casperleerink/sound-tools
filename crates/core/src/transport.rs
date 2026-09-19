//! The transport: whether the project plays and where the project position is.
//!
//! The state lives on the audio thread. The control side changes it with messages and reads it
//! back through the engine status.

use std::ops::Range;
use std::sync::Arc;

use crate::clock::{Clock, Frames, Ticks};

/// What a processor sees of the transport for one block.
///
/// While playing, the blocks of a processor cover the project timeline without gaps or
/// overlaps: each `tick_range` starts where the previous one ended, until `jumped` says
/// otherwise. So a processor that emits what starts inside `tick_range` emits everything
/// exactly once. While not playing both ranges are empty.
#[derive(Clone, Debug)]
pub struct Transport<'a> {
    pub playing: bool,
    /// The position moved by a seek or a stop since the previous block. Set for one block.
    /// Nothing between the old and the new position is replayed. Each processor decides what
    /// to do, for example release its held notes.
    pub jumped: bool,
    /// The project frames this block covers. As long as the block while playing. A tempo map
    /// change moves the frame position, because it keeps the tick position.
    pub frame_range: Range<Frames>,
    /// The ticks that land on a frame of this block.
    pub tick_range: Range<Ticks>,
    /// For any other conversion, for example the tempo at a tick.
    pub clock: &'a Clock,
}

impl Transport<'_> {
    /// The frame offset inside this block at which `tick` lands, ready for
    /// `event_outputs.push`. `None` when the tick is not in `tick_range`.
    pub fn offset_of(&self, tick: Ticks) -> Option<usize> {
        if !self.tick_range.contains(&tick) {
            return None;
        }
        let frame = self.clock.frame_of(tick).0;
        usize::try_from(frame.checked_sub(self.frame_range.start.0)?).ok()
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
    jumped: bool,
    position: Frames,
    clock: Arc<Clock>,
}

impl TransportState {
    pub(crate) fn new(clock: Arc<Clock>) -> Self {
        Self {
            playing: false,
            jumped: false,
            position: Frames(0),
            clock,
        }
    }

    pub(crate) fn apply(&mut self, command: TransportCommand) {
        match command {
            TransportCommand::Play => self.playing = true,
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
    }

    /// Swaps the clocks, so the old one can travel back to the control side. Keeps the musical
    /// position: the next block starts with the same tick as it would have with the old clock.
    pub(crate) fn swap_clock(&mut self, clock: &mut Arc<Clock>) {
        let next_tick = self.clock.tick_at(self.position);
        std::mem::swap(clock, &mut self.clock);
        self.position = self.clock.frame_of(next_tick);
    }

    /// The view for the next block of `frames` frames.
    pub(crate) fn block(&self, frames: usize) -> Transport<'_> {
        let advance = if self.playing { frames as u64 } else { 0 };
        let end = Frames(self.position.0.saturating_add(advance));
        Transport {
            playing: self.playing,
            jumped: self.jumped,
            frame_range: self.position..end,
            // Both ends come from the same function, so the end of this block is the start of
            // the next, whatever the block sizes and tempo changes are.
            tick_range: self.clock.tick_at(self.position)..self.clock.tick_at(end),
            clock: &self.clock,
        }
    }

    /// Call after the block ran, with the end of its frame range.
    pub(crate) fn finish_block(&mut self, end: Frames) {
        self.position = end;
        self.jumped = false;
    }
}
