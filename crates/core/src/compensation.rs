//! The delay on the faster paths of an output that feeds paths of different latency.
//!
//! A processor that feeds several others runs as far ahead as the one that needs the most, see
//! `Engine::find_leads`. Every other one of them would hear it early, so the engine keeps the
//! recent past of such an output and plays it to each of them that much later. Only audio is
//! delayed: an event connection to a faster path arrives early, and a live input is not led, so
//! it is never early.

use crate::delay_line::DelayLine;
use crate::graph::NodeId;
use crate::processor::{AudioBuffer, CHANNELS, MAX_BLOCK};

/// The longest delay the engine makes up for, in frames: one second at 48 kHz, half at 96 kHz.
/// That is more than a chain of linear phase equalizers and lookahead limiters reports, and it
/// keeps the memory of each delayed output fixed. A longer delay is made up to this much.
const MAX_COMPENSATION: u64 = 48_000;

/// One audio output of a processor that feeds two or more others, or another and the device.
/// Built when the schedule compiles; its delays are worked out on the audio thread, because
/// only there are the latencies known.
pub(crate) struct Compensation {
    /// The processor and the index of its output: the same in the next schedule, so the past
    /// carries over a routing edit.
    pub key: (NodeId, usize),
    pub buffer: usize,
    pub to_device: bool,
    /// How far ahead of the device this output runs.
    pub ahead: u64,
    /// How far ahead of the device the least led of its listeners runs. The device is not led.
    pub least_lead: u64,
    history: History,
}

/// What an output played lately. It is written only while a listener needs a delay, so a
/// project in time without one costs nothing.
struct History {
    /// Room for the longest delay and the block itself.
    lines: [DelayLine; CHANNELS],
    /// Frames written since it started this time: where the next frame goes.
    written: u64,
    active: bool,
    /// The `ahead` it was written under. Kept here, not read from the `Compensation`, because
    /// the history moves to the next schedule.
    ahead: u64,
}

impl Compensation {
    /// Allocates: make it on the control thread.
    pub(crate) fn new(key: (NodeId, usize), buffer: usize, to_device: bool) -> Self {
        let frames = MAX_COMPENSATION as usize + MAX_BLOCK;
        Self {
            key,
            buffer,
            to_device,
            ahead: 0,
            least_lead: 0,
            history: History {
                lines: std::array::from_fn(|_| DelayLine::new(frames)),
                written: 0,
                active: false,
                ahead: 0,
            },
        }
    }

    /// Takes in new `ahead` and `least_lead`. The history starts over when a listener comes
    /// to need a delay, so no listener hears a past from before. A new `ahead` starts it over
    /// too: the producer jumped, and what it played before is on another timeline. A new
    /// `least_lead` does not; that listener jumps itself.
    pub(crate) fn retime(&mut self) {
        let delayed = self.ahead > self.least_lead;
        let history = &mut self.history;
        if delayed && (!history.active || history.ahead != self.ahead) {
            history.written = 0;
        }
        history.active = delayed;
        history.ahead = self.ahead;
    }

    /// Takes over the past of the same output in the schedule before, by swapping, so nothing
    /// is copied and the one replaced rides back with that schedule.
    pub(crate) fn carry_over(&mut self, before: &mut Self) {
        std::mem::swap(&mut self.history, &mut before.history);
    }

    /// Keeps the first `frames` of `block`, the output of this block.
    pub(crate) fn write(&mut self, block: &AudioBuffer, frames: usize) {
        let history = &mut self.history;
        if !history.active {
            return;
        }
        // The line wraps by its length, a power of two, so a count that wraps loses nothing.
        let start = history.written as usize;
        for (line, channel) in history.lines.iter_mut().zip(block) {
            for (offset, sample) in channel.iter().take(frames).enumerate() {
                line.write(start.wrapping_add(offset), *sample);
            }
        }
        history.written += frames as u64;
    }

    /// What a listener `lead` frames ahead of the device hears of this block of `frames`, when
    /// that is not the block as it is.
    pub(crate) fn heard(&self, lead: u64, frames: usize) -> Option<AudioBuffer> {
        let delay = self.ahead.saturating_sub(lead).min(MAX_COMPENSATION);
        let history = &self.history;
        if delay == 0 || !history.active {
            return None;
        }
        let reach = delay as usize + frames;
        // Frames from before the history started are silence.
        let silent = (reach as u64).saturating_sub(history.written) as usize;
        let end = history.written as usize;
        let mut heard = [[0.0; MAX_BLOCK]; CHANNELS];
        for (channel, line) in heard.iter_mut().zip(&history.lines) {
            let channel = channel.iter_mut().enumerate().take(frames).skip(silent);
            for (frame, sample) in channel {
                *sample = line.read(end, reach - frame);
            }
        }
        Some(heard)
    }
}
