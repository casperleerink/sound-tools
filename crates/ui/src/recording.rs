//! Recording audio as the views see it: which audio tracks are armed, the level of the input,
//! and each take while it grows.
//!
//! Interface state, like the selection: nothing is saved, and arming is no undo step, like the
//! click. The window owns the input device and records; views arm tracks, show the level and
//! draw the takes. The [`Session`](crate::Session) holds the one entity of a window.
//!
//! The level changes every poll while the input is open, so it is an event, [`InputLevels`],
//! and not a notification: a view that shows a meter subscribes to it, and one that draws the
//! arm toggles or the takes observes the entity.

use std::collections::BTreeSet;
use std::ops::Range;

use gpui::{Context, EventEmitter};
use sound_core::{InstanceId, Ticks};
use sound_media::TakeOverview;

/// A take while it records, as the timeline draws it: from where the recording began to the
/// playhead, with a red border.
#[derive(Clone, Debug)]
pub struct LiveTake {
    pub track: InstanceId,
    pub start: Ticks,
    /// What its file holds so far, once the first frames came and the recording is tied to
    /// the timeline. Until then it draws no waveform.
    pub sound: Option<LiveSound>,
}

#[derive(Clone, Debug)]
pub struct LiveSound {
    pub overview: TakeOverview,
    /// The second of the file the composer heard at `start`: where its waveform lines up.
    pub start_seconds: f64,
}

/// A new reading of the input level, once per poll while the input is open.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InputLevels;

#[derive(Default)]
pub struct Recording {
    armed: BTreeSet<InstanceId>,
    input_channels: Option<usize>,
    /// The loudest sample of each channel of the input since the last poll.
    levels: Vec<f32>,
    takes: Vec<LiveTake>,
}

impl EventEmitter<InputLevels> for Recording {}

impl Recording {
    pub fn is_armed(&self, track: &InstanceId) -> bool {
        self.armed.contains(track)
    }

    /// The armed tracks, in the order of their ids.
    pub fn armed(&self) -> impl Iterator<Item = &InstanceId> {
        self.armed.iter()
    }

    /// Arms a track or disarms it. An armed track shows the level of its input, and the record
    /// control records it.
    pub fn set_armed(&mut self, track: InstanceId, armed: bool, cx: &mut Context<Self>) {
        let changed = match armed {
            true => self.armed.insert(track),
            false => self.armed.remove(&track),
        };
        if changed {
            cx.notify();
        }
    }

    /// Keeps only the armed tracks for which `keep` holds, such as the ones that still exist.
    pub fn retain_armed(&mut self, keep: impl Fn(&InstanceId) -> bool, cx: &mut Context<Self>) {
        let before = self.armed.len();
        self.armed.retain(|track| keep(track));
        if self.armed.len() != before {
            cx.notify();
        }
    }

    /// How many channels the default input has, once the window opened it. `None` before, and
    /// while there is no input.
    pub fn input_channels(&self) -> Option<usize> {
        self.input_channels
    }

    pub fn set_input_channels(&mut self, channels: Option<usize>, cx: &mut Context<Self>) {
        if self.input_channels != channels {
            self.input_channels = channels;
            if channels.is_none() {
                self.levels.clear();
            }
            cx.notify();
        }
    }

    /// The level of these channels of the input since the last poll, left and right. One
    /// channel shows on both sides. Silence for a channel the input does not have.
    pub fn level(&self, channels: Range<usize>) -> [f32; 2] {
        let at = |channel: usize| self.levels.get(channel).copied().unwrap_or_default();
        let (first, last) = (channels.start, channels.end.saturating_sub(1).max(channels.start));
        [at(first), at(last)]
    }

    /// One poll of the input: the loudest sample of each channel since the last one.
    pub fn set_levels(&mut self, levels: Vec<f32>, cx: &mut Context<Self>) {
        self.levels = levels;
        cx.emit(InputLevels);
    }

    /// The takes of the recording that runs, one per armed track. Empty while nothing records.
    pub fn takes(&self) -> &[LiveTake] {
        &self.takes
    }

    pub fn take_of(&self, track: &InstanceId) -> Option<&LiveTake> {
        self.takes.iter().find(|take| take.track == *track)
    }

    pub fn set_takes(&mut self, takes: Vec<LiveTake>, cx: &mut Context<Self>) {
        self.takes = takes;
        cx.notify();
    }
}
