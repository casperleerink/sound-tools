//! The editing service: staged changes, edits with gestures, and undo history.
//!
//! Interface edits, file changes, undo and redo all end in `Project::apply`. This module
//! holds what they need around it.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::file::{ProjectFile, SavedConnection};
use super::instance::{Instance, InstanceId, Record, State};
use super::{Project, ProjectError, Source};
use crate::clock::TempoMap;

#[derive(Clone)]
pub(crate) enum Change {
    /// Creates the instance or replaces its whole record.
    Set(InstanceId, Record),
    /// Deletes the instance, everything it owns, and the saved connections that name them.
    Delete(InstanceId),
    ProjectFile(ProjectFile),
    TempoMap(TempoMap),
    Connect(SavedConnection),
    Disconnect(SavedConnection),
}

/// A group of changes that apply together: one state application, one engine batch.
#[derive(Default)]
pub struct Changes {
    pub(crate) changes: Vec<Change>,
}

impl Changes {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an instance. The id decides the owner: `parent.child("name")?` is owned by
    /// `parent`, which must exist or be created earlier in the same group.
    pub fn create<S: State>(&mut self, id: InstanceId, state: S) -> Instance<S> {
        self.changes
            .push(Change::Set(id.clone(), Record::new(state)));
        Instance::new(id)
    }

    /// Replaces the whole state of an instance.
    pub fn set<S: State>(&mut self, instance: &Instance<S>, state: S) {
        let id = instance.id().clone();
        self.changes.push(Change::Set(id, Record::new(state)));
    }

    /// Deletes an instance with everything it owns and the saved connections that name them.
    /// Deleting an instance that does not exist does nothing.
    pub fn delete(&mut self, id: &InstanceId) {
        self.changes.push(Change::Delete(id.clone()));
    }

    /// Adds a connection to `project.json`.
    pub fn connect(&mut self, connection: SavedConnection) {
        self.changes.push(Change::Connect(connection));
    }

    pub fn disconnect(&mut self, connection: SavedConnection) {
        self.changes.push(Change::Disconnect(connection));
    }

    pub fn set_tempo_map(&mut self, tempo_map: TempoMap) {
        self.changes.push(Change::TempoMap(tempo_map));
    }
}

/// One record before and after a state application. `None` means the instance did not exist.
pub(crate) struct RecordChange {
    pub id: InstanceId,
    pub before: Option<Record>,
    pub after: Option<Record>,
}

/// What one state application changed.
#[derive(Default)]
pub(crate) struct Applied {
    pub records: Vec<RecordChange>,
    /// Before and after.
    pub project_file: Option<(ProjectFile, ProjectFile)>,
}

/// Everything one undo step touched, with the state before the first change and after the
/// last. Undo applies the before side as one group, redo the after side.
#[derive(Default)]
pub(crate) struct Step {
    pub label: String,
    pub records: BTreeMap<InstanceId, (Option<Record>, Option<Record>)>,
    pub project_file: Option<(ProjectFile, ProjectFile)>,
}

impl Step {
    pub fn absorb(&mut self, applied: Applied) {
        for change in applied.records {
            let entry = self
                .records
                .entry(change.id)
                .or_insert((change.before, None));
            entry.1 = change.after;
        }
        if let Some((before, after)) = applied.project_file {
            let entry = self.project_file.get_or_insert((before, after.clone()));
            entry.1 = after;
        }
    }

    fn is_empty(&self) -> bool {
        self.records.is_empty() && self.project_file.is_none()
    }

    /// The changes that bring back one side of the step.
    fn changes(&self, side: Side) -> Vec<Change> {
        let mut changes = Vec::new();
        if let Some((before, after)) = &self.project_file {
            changes.push(Change::ProjectFile(match side {
                Side::Before => before.clone(),
                Side::After => after.clone(),
            }));
        }
        for (id, (before, after)) in &self.records {
            let record = match side {
                Side::Before => before,
                Side::After => after,
            };
            changes.push(match record {
                Some(record) => Change::Set(id.clone(), record.clone()),
                None => Change::Delete(id.clone()),
            });
        }
        changes
    }
}

#[derive(Copy, Clone)]
enum Side {
    Before,
    After,
}

/// Groups of outside file changes that follow each other within this time are one undo step.
///
/// An agent writes the files of one request seconds apart, far more than the quiet window
/// that groups them for live apply. Without this, undo of a track that an agent added takes
/// one step per file. It is a heuristic: the second milestone replaces it with the real
/// boundaries of an agent request.
pub const OUTSIDE_UNDO_WINDOW: Duration = Duration::from_secs(15);

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// When the step on top of `undo` is an outside step and nothing came after it: when its
    /// last group was applied. An interface edit, an undo and a redo all clear it.
    last_outside: Option<Instant>,
}

impl History {
    /// A new step. What was undone before can no longer be redone.
    pub fn push(&mut self, step: Step) {
        self.last_outside = None;
        if !step.is_empty() {
            self.undo.push(step);
            self.redo.clear();
        }
    }

    /// A group of outside changes, applied at `at`. It joins the step on top when that is an
    /// outside step from less than [`OUTSIDE_UNDO_WINDOW`] before, with nothing in between.
    /// The joined step keeps its older before side and takes the newer after side.
    pub fn push_outside(&mut self, label: &str, applied: Applied, at: Instant) {
        let recent = self
            .last_outside
            .is_some_and(|last| at.saturating_duration_since(last) < OUTSIDE_UNDO_WINDOW);
        match self.undo.last_mut().filter(|_| recent) {
            Some(step) => {
                step.absorb(applied);
                self.last_outside = Some(at);
            }
            None => {
                let mut step = Step {
                    label: label.to_string(),
                    ..Step::default()
                };
                step.absorb(applied);
                let is_empty = step.is_empty();
                self.push(step);
                // An empty group is no step, and then the top is not known to be an outside one.
                self.last_outside = (!is_empty).then_some(at);
            }
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// An edit in progress: a drag, or any other group of changes that becomes one undo step.
/// It remembers the state before its first change, so finishing and cancelling need no
/// reverse operation from the tool. Several edits can be open at once; last write wins.
#[must_use = "finish or cancel the edit"]
pub struct Edit {
    step: Step,
}

impl Project {
    /// Begins an edit. `label` names the undo step, for example "Change frequency".
    pub fn begin(&self, label: &str) -> Edit {
        Edit {
            step: Step {
                label: label.to_string(),
                ..Step::default()
            },
        }
    }

    /// Applies changes now, as one group. Sound and views follow, no file is written. Call it
    /// as often as the gesture moves. On an error nothing of the group is applied.
    pub fn publish(&mut self, edit: &mut Edit, changes: Changes) -> Result<(), ProjectError> {
        for change in &changes.changes {
            if let Change::Set(id, record) = change {
                record
                    .state
                    .validate()
                    .map_err(|message| ProjectError::InvalidState {
                        id: id.clone(),
                        message,
                    })?;
            }
        }
        let touches_project_file = changes
            .changes
            .iter()
            .any(|change| !matches!(change, Change::Set(..)));
        if touches_project_file {
            self.sync_project_file();
        }
        let applied = self.apply(changes.changes, Source::Interface)?;
        edit.step.absorb(applied);
        Ok(())
    }

    /// Publishes a change to one instance. `change` gets a copy of the current state.
    pub fn update<S: State>(
        &mut self,
        edit: &mut Edit,
        instance: &Instance<S>,
        change: impl FnOnce(&mut S),
    ) -> Result<(), ProjectError> {
        let mut state = self
            .state(instance)
            .ok_or_else(|| ProjectError::MissingInstance(instance.id().clone()))?
            .clone();
        change(&mut state);
        let mut changes = Changes::new();
        changes.set(instance, state);
        self.publish(edit, changes)
    }

    /// Ends the edit as one undo step and writes every record it touched, once. The step runs
    /// from the state before the edit to the state now, whoever wrote last.
    ///
    /// On a write error the edit stays applied and undoable, and the problem is reported.
    pub fn finish(&mut self, edit: Edit) -> Result<(), ProjectError> {
        let mut step = edit.step;
        for (id, (_, after)) in &mut step.records {
            *after = self.instances.get(id).cloned();
        }
        if let Some((_, after)) = &mut step.project_file {
            *after = self.project_file.clone();
        }
        step.records
            .retain(|_, (before, after)| match (before, after) {
                (Some(before), Some(after)) => !before.equals(after),
                (None, None) => false,
                _ => true,
            });
        step.project_file.take_if(|(before, after)| before == after);
        let written = self.write(step.records.keys(), step.project_file.is_some());
        self.history.push(step);
        written
    }

    /// Ends the edit by applying the state from before it, through the same path as every
    /// other change. No undo step.
    pub fn cancel(&mut self, edit: Edit) -> Result<(), ProjectError> {
        let step = edit.step;
        self.sync_project_file();
        let applied = self.apply(step.changes(Side::Before), Source::History)?;
        self.write_step(&step, &applied)
    }

    /// One finished edit: begin, publish, finish.
    pub fn commit(&mut self, label: &str, changes: Changes) -> Result<(), ProjectError> {
        let mut edit = self.begin(label);
        self.publish(&mut edit, changes)?;
        self.finish(edit)
    }

    /// Undoes the last step and writes the records. Returns its label, or `None` when there
    /// is nothing to undo. A step that can no longer apply is dropped with the error.
    pub fn undo(&mut self) -> Result<Option<String>, ProjectError> {
        let Some(step) = self.history.undo.pop() else {
            return Ok(None);
        };
        self.sync_project_file();
        self.history.last_outside = None;
        let applied = self.apply(step.changes(Side::Before), Source::History)?;
        let written = self.write_step(&step, &applied);
        let label = step.label.clone();
        self.history.redo.push(step);
        written.map(|()| Some(label))
    }

    pub fn redo(&mut self) -> Result<Option<String>, ProjectError> {
        let Some(step) = self.history.redo.pop() else {
            return Ok(None);
        };
        self.sync_project_file();
        self.history.last_outside = None;
        let applied = self.apply(step.changes(Side::After), Source::History)?;
        let written = self.write_step(&step, &applied);
        let label = step.label.clone();
        self.history.undo.push(step);
        written.map(|()| Some(label))
    }

    /// Writes what the step names and what applying it touched. The two differ when a delete
    /// took along instances that were created after the step.
    fn write_step(&mut self, step: &Step, applied: &Applied) -> Result<(), ProjectError> {
        let touched = applied.records.iter().map(|change| &change.id);
        let project_file = step.project_file.is_some() || applied.project_file.is_some();
        self.write(step.records.keys().chain(touched), project_file)
    }

    /// Forgets every undo and redo step. A new project starts like this: making its default
    /// content is not something to undo.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// The label of the step that `undo` would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo.last().map(|step| step.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo.last().map(|step| step.label.as_str())
    }
}
