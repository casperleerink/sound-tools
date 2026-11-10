//! Recording as the views see it: which instances are armed, and each take while it grows: its
//! sound, and the notes it heard.
//!
//! Interface state, like the selection: nothing is saved, and arming is no undo step, like the
//! click. The window owns the input device and records; views arm and draw the takes. The
//! [`Session`](crate::Session) holds the one entity of a window.

use std::collections::BTreeSet;
use std::rc::Rc;

use gpui::{Context, EventEmitter};
use sound_core::{InstanceId, Ticks};
use sound_media::TakeOverview;

/// An audio take while it records: from where the recording began to the playhead.
#[derive(Clone, Debug)]
pub struct LiveTake {
    pub recorder: InstanceId,
    pub start: Ticks,
    pub sound: LiveSound,
}

#[derive(Clone, Debug)]
pub struct LiveSound {
    pub overview: TakeOverview,
    /// The second of the file the composer heard at `start`: where its waveform lines up.
    pub start_seconds: f64,
}

/// The notes a take heard so far, while it records.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveNotes {
    pub recorder: InstanceId,
    pub start: Ticks,
    pub notes: Vec<LiveNote>,
}

/// A note of a take while it records: a held key has no end yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveNote {
    pub pitch: u8,
    pub start: Ticks,
    pub end: Option<Ticks>,
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
    /// Shared with what draws them on every frame.
    notes: Vec<Rc<LiveNotes>>,
    input: Vec<f32>,
}

impl Recording {
    /// The armed instances, in the order of their ids.
    pub fn armed(&self) -> impl Iterator<Item = &InstanceId> {
        self.armed.iter()
    }

    /// Arms an instance or disarms it: the input is open while one is armed.
    pub fn set_armed(&mut self, recorder: InstanceId, armed: bool, cx: &mut Context<Self>) {
        let changed = match armed {
            true => self.armed.insert(recorder),
            false => self.armed.remove(&recorder),
        };
        if changed {
            cx.notify();
        }
    }

    /// Keeps only the armed instances for which `keep` holds, such as the ones that still exist.
    pub fn retain_armed(&mut self, keep: impl Fn(&InstanceId) -> bool, cx: &mut Context<Self>) {
        let before = self.armed.len();
        self.armed.retain(|recorder| keep(recorder));
        if self.armed.len() != before {
            cx.notify();
        }
    }

    /// Whether a take is recording.
    pub fn is_recording(&self) -> bool {
        !self.takes.is_empty() || !self.notes.is_empty()
    }

    /// The newest take of `recorder`: at a loop the one before may still wait for its end.
    pub fn take_of(&self, recorder: &InstanceId) -> Option<&LiveTake> {
        self.takes
            .iter()
            .rev()
            .find(|take| take.recorder == *recorder)
    }

    pub fn set_takes(&mut self, takes: Vec<LiveTake>, cx: &mut Context<Self>) {
        self.takes = takes;
        cx.notify();
    }

    /// The notes of the take of `recorder` that runs.
    pub fn notes_of(&self, recorder: &InstanceId) -> Option<&Rc<LiveNotes>> {
        self.notes.iter().find(|take| take.recorder == *recorder)
    }

    pub fn set_notes(&mut self, notes: Vec<LiveNotes>, cx: &mut Context<Self>) {
        let notes: Vec<Rc<LiveNotes>> = notes.into_iter().map(Rc::new).collect();
        if self.notes != notes {
            self.notes = notes;
            cx.notify();
        }
    }

    /// The loudest sample of each channel of the audio input at the last poll; none while it is
    /// closed.
    pub fn input_levels(&self) -> &[f32] {
        &self.input
    }

    /// Set at every poll, with no notify: what draws a level reads it at its own poll.
    pub fn set_input_levels(&mut self, levels: Vec<f32>) {
        self.input = levels;
    }

    /// A take ended: the window gives it to the code of its recorder's parent.
    pub fn finished(&mut self, take: FinishedTake, cx: &mut Context<Self>) {
        cx.emit(take);
    }
}

impl EventEmitter<FinishedTake> for Recording {}
