//! A drag of the composer's hand is one gesture of the session and one undo step. It opens at
//! its first move, so a press that changes nothing is no step; every move publishes into it, so
//! the sound follows the hand; it ends as one step, or puts back the state from before.

use serde_json::Value;
use sound_core::{InstanceId, Project, ProjectEdit, ProjectError, ProjectEvent};
use sound_typescript::path;

use super::Session;

/// One drag, from its first move to its let-go, named in the history by its label. The session
/// keeps the edit, so no view leaves one open; this says which gesture is the drag's.
#[must_use = "a drag ends with `Session::finish` or `Session::cancel`"]
#[derive(Debug)]
pub struct Drag {
    label: String,
    /// The number of the session's gesture this drag opened, once it moved.
    gesture: Option<u64>,
}

impl Drag {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            gesture: None,
        }
    }
}

/// A move that would change numbers a lane moves, by their paths: the lane decides them.
#[derive(Debug, PartialEq, Eq)]
pub struct Held(pub Vec<String>);

impl std::fmt::Display for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = if self.0.len() == 1 {
            "this number"
        } else {
            "these numbers"
        };
        write!(
            f,
            "A lane plays {what}: turn on Write automation and play to write into it."
        )
    }
}

impl Session {
    /// One move of `drag`: `publish` runs on the open gesture, which the first move opens. A
    /// gesture another closed meanwhile opens again. `None` on an error, which is the notice.
    pub fn publish<R>(
        &mut self,
        drag: &mut Drag,
        publish: impl FnOnce(&mut Project, &mut ProjectEdit) -> Result<R, ProjectError>,
    ) -> Option<R> {
        if !self.owns(drag) {
            self.finish_open();
            self.gestures += 1;
            self.gesture = Some((self.gestures, self.project.begin(&drag.label)));
            drag.gesture = Some(self.gestures);
        }
        let (number, mut edit) = self.gesture.take()?;
        let result = self.edit(|project| publish(project, &mut edit));
        self.gesture = Some((number, edit));
        if result.is_some() {
            self.follow(true);
        }
        result
    }

    /// One move of `drag` that plays `values` into numbers of `instance` by their paths, as
    /// `drag.plays` names them: `cutoff_hz`, `bands[0].gain_db`, or `t01/mix.gain_db` of an
    /// instance inside it. A number a lane holds refuses the move, and ends the drag's gesture
    /// as the step it was so far.
    pub fn play(
        &mut self,
        drag: &mut Drag,
        instance: &InstanceId,
        values: &[(&str, f64)],
    ) -> Result<(), Held> {
        let writing = self.writes_now();
        let resolved: Vec<_> = (values.iter())
            .map(|(path, value)| (*path, number_at(&self.project, instance, path), *value))
            .collect();
        // A move may still come in the frame that lost the record.
        if (resolved.iter()).any(|(_, (id, _), _)| self.project.tool_of(id).is_none()) {
            return Ok(());
        }
        let held: Vec<String> = (resolved.iter())
            .filter(|(_, (id, number), _)| is_held(&self.project, id, number, writing))
            .map(|(path, _, _)| (*path).to_string())
            .collect();
        if !held.is_empty() {
            if self.owns(drag) {
                self.finish_open();
            }
            return Err(Held(held));
        }
        self.publish(drag, |project, edit| {
            for (_, (id, number), value) in resolved {
                let value = Some(json_number(value));
                project.update_json(edit, &id, |state| path::set(state, number, value))?;
            }
            Ok(())
        });
        Ok(())
    }

    /// A typed value, a key step or a reset: a drag that goes there and lets go at once, so
    /// writing automation follows it too.
    pub fn set(
        &mut self,
        label: impl Into<String>,
        instance: &InstanceId,
        values: &[(&str, f64)],
    ) -> Result<(), Held> {
        let mut drag = Drag::new(label);
        let played = self.play(&mut drag, instance, values);
        self.finish(drag);
        played
    }

    /// Lets go of `drag`: its gesture is one undo step, written to the files.
    pub fn finish(&mut self, drag: Drag) {
        if self.owns(&drag) {
            self.finish_open();
        }
    }

    /// [`Self::finish`] with the step named `label`, the name code gave the update that ends it.
    pub fn finish_as(&mut self, drag: Drag, label: &str) {
        if let (true, Some((_, edit))) = (self.owns(&drag), &mut self.gesture) {
            edit.set_label(label);
        }
        self.finish(drag);
    }

    /// Escape: ends `drag` and puts back the state from before it.
    pub fn cancel(&mut self, drag: Drag) {
        if !self.owns(&drag) {
            return;
        }
        let Some((_, edit)) = self.gesture.take() else {
            return;
        };
        let mut writer = std::mem::take(&mut self.writer);
        // The writer lets go first, so what it held plays before the state before the drag.
        self.edit(|project| {
            let followed = writer.cancel(project);
            project.cancel(edit).and(followed)
        });
        self.writer = writer;
    }

    pub fn gesture_open(&self) -> bool {
        self.gesture.is_some()
    }

    fn owns(&self, drag: &Drag) -> bool {
        drag.gesture.is_some() && drag.gesture == self.gesture.as_ref().map(|(n, _)| *n)
    }

    pub(super) fn finish_open(&mut self) {
        let Some((_, mut edit)) = self.gesture.take() else {
            return;
        };
        let mut writer = std::mem::take(&mut self.writer);
        self.edit(|project| {
            let followed = writer.finish(project, &mut edit);
            project.finish(edit).and(followed)
        });
        self.writer = writer;
    }

    /// The touch writer samples the open gesture: after each publish and on each poll.
    pub(super) fn follow(&mut self, published: bool) {
        let Some((_, edit)) = &self.gesture else {
            return;
        };
        let writes = self.transport.automation;
        let status = self.status;
        if let Err(error) = (self.writer).follow(&mut self.project, edit, status, published, writes)
        {
            self.report(error);
        }
    }

    /// A record deleted from outside during its drag: the delete was the last write, so the
    /// gesture finishes; a cancel would bring the record back.
    pub(super) fn finish_if_deleted(&mut self, events: &[ProjectEvent]) {
        let Some((_, edit)) = &self.gesture else {
            return;
        };
        let deleted = |id: &InstanceId| {
            (events.iter()).any(|event| matches!(event, ProjectEvent::Deleted(gone) if gone == id))
        };
        if edit.published().any(deleted) {
            self.finish_open();
        }
    }
}

/// The instance a path of `instance` names a number of, and that number: an instance inside it
/// when the path starts with one, else `instance` itself (`pads.36.pan` of a kit is its own).
pub(super) fn number_at<'a>(
    project: &Project,
    instance: &InstanceId,
    path: &'a str,
) -> (InstanceId, &'a str) {
    if let Some((inside, number)) = path.split_once('.')
        && let Ok(id) = instance.child(inside)
        && project.tool_of(&id).is_some()
    {
        return (id, number);
    }
    (instance.clone(), path)
}

/// Whether a lane decides `number` of `id`. While `writing`, a lane a hand writes into does not.
fn is_held(project: &Project, id: &InstanceId, number: &str, writing: bool) -> bool {
    let Some(lanes) = project.lanes(id) else {
        return false;
    };
    let mut values = Vec::new();
    super::numbers_held(lanes, writing, &mut values);
    values.iter().any(|(name, _)| **name == *number)
}

/// A number as a record holds it: a whole one as an integer, which every number field reads,
/// and silence as `"-inf"`, which JSON has no number for.
fn json_number(value: f64) -> Value {
    if value == f64::NEG_INFINITY {
        return Value::from("-inf");
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return Value::from(value as i64);
    }
    Value::from(value)
}
