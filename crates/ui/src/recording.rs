//! Recording as the views see it: which instances are armed, and each audio take while it
//! grows.
//!
//! Interface state, like the selection: nothing is saved, and arming is no undo step, like the
//! click. The window owns the input device and records; views arm and draw the takes. The
//! [`Session`](crate::Session) holds the one entity of a window.

use std::collections::BTreeSet;

use gpui::{Context, EventEmitter};
use sound_core::{InstanceId, Ticks};
use sound_media::TakeOverview;

/// An audio take while it records: from where the recording began to the playhead.
#[derive(Clone, Debug)]
pub struct LiveTake {
    pub track: InstanceId,
    pub start: Ticks,
    pub sound: LiveSound,
}

#[derive(Clone, Debug)]
pub struct LiveSound {
    pub overview: TakeOverview,
    /// The second of the file the composer heard at `start`: where its waveform lines up.
    pub start_seconds: f64,
}

/// A take that ended, for the code of the parent of its recorder to keep: the record of a child
/// that would hold it, a `timeline` of notes or a `region` of sound, whose files are written.
#[derive(Clone, Debug)]
pub struct FinishedTake {
    pub recorder: InstanceId,
    /// A name of the files it wrote, for the child that keeps it.
    pub name: String,
    pub tool: &'static str,
    pub state: serde_json::Value,
}

#[derive(Default)]
pub struct Recording {
    armed: BTreeSet<InstanceId>,
    takes: Vec<LiveTake>,
}

impl Recording {
    /// The armed instances, in the order of their ids.
    pub fn armed(&self) -> impl Iterator<Item = &InstanceId> {
        self.armed.iter()
    }

    /// Arms an instance or disarms it: the input is open while one is armed.
    pub fn set_armed(&mut self, track: InstanceId, armed: bool, cx: &mut Context<Self>) {
        let changed = match armed {
            true => self.armed.insert(track),
            false => self.armed.remove(&track),
        };
        if changed {
            cx.notify();
        }
    }

    /// Keeps only the armed instances for which `keep` holds, such as the ones that still exist.
    pub fn retain_armed(&mut self, keep: impl Fn(&InstanceId) -> bool, cx: &mut Context<Self>) {
        let before = self.armed.len();
        self.armed.retain(|track| keep(track));
        if self.armed.len() != before {
            cx.notify();
        }
    }

    pub fn take_of(&self, track: &InstanceId) -> Option<&LiveTake> {
        self.takes.iter().find(|take| take.track == *track)
    }

    pub fn set_takes(&mut self, takes: Vec<LiveTake>, cx: &mut Context<Self>) {
        self.takes = takes;
        cx.notify();
    }

    /// A take ended: the window gives it to the code of its recorder's parent.
    pub fn finished(&mut self, take: FinishedTake, cx: &mut Context<Self>) {
        cx.emit(take);
    }
}

impl EventEmitter<FinishedTake> for Recording {}
