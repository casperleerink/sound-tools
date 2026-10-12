//! The delay on the faster paths of an output that feeds paths of different latency.
//!
//! A processor that feeds several others runs as far ahead as the one that needs the most, see
//! `Engine::find_leads`. Every other one of them would hear it early, so the engine keeps the
//! recent past of such an output and plays it to each of them that much later. Only audio is
//! delayed: an event connection to a faster path arrives early, and a live input is not led, so
//! it is never early.

use crate::graph::NodeId;
use crate::processor::{AudioBuffer, CHANNELS, MAX_BLOCK};

/// The longest delay the engine makes up for, in frames: one second at 48 kHz, half at 96 kHz.
/// That is more than a chain of linear phase equalizers and lookahead limiters reports, and the
/// memory is fixed, under 400 KB for each output that feeds paths of different latency. A longer
/// delay is made up to this much, and the rest shows as `EngineStatus::uncompensated_latency`.
pub(crate) const MAX_COMPENSATION: u64 = 48_000;

/// The frames one block reads back at most: the longest delay, and the block itself.
const HISTORY: usize = MAX_COMPENSATION as usize + MAX_BLOCK;

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
    channels: [Vec<f32>; CHANNELS],
    /// Frames written since it started this time.
    written: u64,
    active: bool,
}

impl Compensation {
    /// Allocates: make it on the control thread.
    pub(crate) fn new(key: (NodeId, usize), buffer: usize, to_device: bool) -> Self {
        Self {
            key,
            buffer,
            to_device,
            ahead: 0,
            least_lead: 0,
            history: History {
                channels: std::array::from_fn(|_| vec![0.0; HISTORY]),
                written: 0,
                active: false,
            },
        }
    }

    /// The delay the listener that needs most needs, from `ahead` and `least_lead`. Starts
    /// writing when it is no longer zero, so no listener hears a past from before.
    pub(crate) fn longest_delay(&mut self) -> u64 {
        let longest = self.ahead.saturating_sub(self.least_lead);
        if longest > 0 && !self.history.active {
            self.history.written = 0;
        }
        self.history.active = longest > 0;
        longest
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
        let start = (history.written % HISTORY as u64) as usize;
        for (line, channel) in history.channels.iter_mut().zip(block) {
            let Some((front, back)) = line.split_at_mut_checked(start) else {
                continue;
            };
            for (kept, sample) in back
                .iter_mut()
                .chain(front)
                .zip(channel.iter().take(frames))
            {
                *kept = *sample;
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
        let reach = delay + frames as u64;
        // Frames from before the history started are silence.
        let silent = reach.saturating_sub(history.written).min(frames as u64);
        let start = (history.written + silent).saturating_sub(reach) % HISTORY as u64;
        let mut heard = [[0.0; MAX_BLOCK]; CHANNELS];
        for (channel, line) in heard.iter_mut().zip(&history.channels) {
            let Some((front, back)) = line.split_at_checked(start as usize) else {
                continue;
            };
            let samples = back.iter().chain(front);
            let channel = channel.iter_mut().take(frames).skip(silent as usize);
            for (sample, kept) in channel.zip(samples) {
                *sample = *kept;
            }
        }
        Some(heard)
    }
}
