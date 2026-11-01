//! The editing service: staged changes, edits with gestures, and undo and redo. The steps
//! themselves are kept in `history.rs`.
//!
//! Interface edits, file changes, undo and redo all end in `Project::apply`. This module
//! holds what they need around it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use super::file::{ProjectFile, SavedConnection};
use super::history::{Side, Step};
use super::instance::{Instance, InstanceId, Record, State};
use super::{Project, ProjectError, Source};
use crate::clock::TempoMap;
use crate::sections::Sections;

#[derive(Clone)]
pub(crate) enum Change {
    /// Creates the instance or replaces its whole record.
    Set(InstanceId, Record),
    /// Deletes the instance, everything it owns, and the saved connections that name them.
    Delete(InstanceId),
    ProjectFile(ProjectFile),
    TempoMap(TempoMap),
    Sections(Sections),
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

    pub fn set_sections(&mut self, sections: Sections) {
        self.changes.push(Change::Sections(sections));
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
    pub unloaded: Unloaded,
}

/// Record files that do not load, by path: their bytes before and after. `None` is no such
/// file, or one that loads.
pub(crate) type Unloaded = BTreeMap<PathBuf, (Option<Vec<u8>>, Option<Vec<u8>>)>;

/// An edit in progress: a drag, or any other group of changes that becomes one undo step.
/// It remembers the state before its first change, so finishing and cancelling need no
/// reverse operation from the tool. Several edits can be open at once; last write wins.
#[must_use = "finish or cancel the edit"]
pub struct Edit {
    step: Step,
}

impl Edit {
    /// It published nothing yet.
    pub(super) fn is_empty(&self) -> bool {
        self.step.is_empty()
    }

    /// The records it published over.
    pub(super) fn records(&self) -> impl Iterator<Item = &InstanceId> {
        self.step.records.keys()
    }
}

/// Every record of `changes` is one its tool accepts.
pub(super) fn validate(changes: &[Change]) -> Result<(), ProjectError> {
    for change in changes {
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
    Ok(())
}

impl Project {
    /// Begins a request: until it ends, every outside change joins one undo step named
    /// `label`, however far apart the changes are. For an agent that knows where a request
    /// begins and ends. An interface edit, an undo or a redo still ends the step, and the
    /// changes of the request after it make a new step with the same label. A request that is
    /// still open is replaced.
    pub fn begin_request(&mut self, label: &str) {
        self.history.begin_request(label);
    }

    /// Ends the request. Outside changes heard less than
    /// [`GROUPING_WINDOW`](super::GROUPING_WINDOW) later still join its step, because the
    /// watcher may hear the last write of a request only after the request has ended. Later
    /// changes follow [`OUTSIDE_UNDO_WINDOW`](super::OUTSIDE_UNDO_WINDOW) again, in a new step.
    pub fn end_request(&mut self) {
        self.end_request_at(Instant::now());
    }

    /// [`Self::end_request`] with the time of the end given, for tests of the undo grouping.
    ///
    /// The first end counts: an end said again later, such as when the agent's process exits
    /// long after its turn, must not let the outside changes of that moment join the step.
    pub fn end_request_at(&mut self, at: Instant) {
        self.history.end_request_at(at);
    }

    /// Begins an edit. `label` names the undo step, for example "Change frequency".
    pub fn begin(&self, label: &str) -> Edit {
        Edit {
            step: Step::new(label),
        }
    }

    /// Applies changes now, as one group. Sound and views follow, no file is written. Call it
    /// as often as the gesture moves. On an error nothing of the group is applied.
    ///
    /// The first publish of an edit first finishes the runs it may finish (see
    /// [`Self::publish_run`]), so a run's step holds none of this edit.
    pub fn publish(&mut self, edit: &mut Edit, changes: Changes) -> Result<(), ProjectError> {
        let finished = match edit.is_empty() {
            true => self.finish_free_runs(),
            false => Ok(()),
        };
        self.publish_into(edit, changes).and(finished)
    }

    /// [`Self::publish`], with the runs left open.
    pub(super) fn publish_into(
        &mut self,
        edit: &mut Edit,
        changes: Changes,
    ) -> Result<(), ProjectError> {
        validate(&changes.changes)?;
        let touches_project_file =
            (changes.changes.iter()).any(|change| !matches!(change, Change::Set(..)));
        if touches_project_file {
            self.sync_project_file();
        }
        let applied = self.apply(changes.changes, Source::Interface)?;
        self.history.hold(&edit.step, &applied);
        edit.step.absorb(applied);
        Ok(())
    }

    /// Applies changes to the sound and the state, and to nothing else: no file is written and
    /// there is no undo step. The one change a read-only project takes, for a
    /// render that plays it other than its files say, such as with one track soloed. A project
    /// that writes refuses with [`ProjectError::WritesFiles`]: its next edit of these records
    /// would save them, and the folder would no longer be the truth.
    pub fn apply_in_memory(&mut self, changes: Changes) -> Result<(), ProjectError> {
        if !self.read_only {
            return Err(ProjectError::WritesFiles);
        }
        validate(&changes.changes)?;
        self.apply(changes.changes, Source::Load)?;
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

    /// [`Self::update`] for code that knows the record only as JSON, such as a view written in
    /// TypeScript. `change` gets the `state` of the record; what it leaves is decoded and
    /// validated as a file of the tool would be, and an error names the field.
    pub fn update_json(
        &mut self,
        edit: &mut Edit,
        id: &InstanceId,
        change: impl FnOnce(&mut serde_json::Value),
    ) -> Result<(), ProjectError> {
        let record = self
            .instances
            .get(id)
            .ok_or_else(|| ProjectError::MissingInstance(id.clone()))?;
        let tool = record.tool;
        let invalid = |message: String| ProjectError::InvalidState {
            id: id.clone(),
            message,
        };
        let json = (record.state.to_json()).map_err(|error| invalid(error.to_string()))?;
        let mut state = serde_json::from_str(&json).map_err(|error| invalid(error.to_string()))?;
        change(&mut state);
        let mut changes = Changes::new();
        self.set_json(&mut changes, id.clone(), tool, state)?;
        self.publish(edit, changes)
    }

    /// Puts a record of `tool` with this `state` at `id` into `changes`, for code that knows the
    /// tool only by name, such as an offer of a tool the project wrote. The state is decoded
    /// and checked here, as a file of the tool would be.
    pub fn set_json(
        &self,
        changes: &mut Changes,
        id: InstanceId,
        tool: &str,
        state: serde_json::Value,
    ) -> Result<(), ProjectError> {
        let definition =
            self.registry
                .definition(tool)
                .ok_or_else(|| ProjectError::InvalidState {
                    id: id.clone(),
                    message: format!("tool {tool:?} is not registered"),
                })?;
        let record = (definition.decode)(state).map_err(|message| ProjectError::InvalidState {
            id: id.clone(),
            message,
        })?;
        changes.changes.push(Change::Set(id, record));
        Ok(())
    }

    /// Ends the edit as one undo step and writes every record it touched, once. The step runs
    /// from the committed state before it to the state now, whoever wrote last. That is the
    /// state before the edit, or what a file change, an undo or a redo wrote during it.
    ///
    /// On a write error the edit stays applied and undoable, and the problem is reported.
    pub fn finish(&mut self, edit: Edit) -> Result<(), ProjectError> {
        let mut step = edit.step;
        let instances = &self.instances;
        self.history
            .close(&mut step, |id| instances.get(id).cloned());
        for (id, (_, after)) in &mut step.records {
            *after = self.instances.get(id).cloned();
        }
        if let Some((_, after)) = &mut step.project_file {
            *after = self.project_file.clone();
        }
        step.drop_unchanged();
        let written = self.write(step.records.keys(), step.project_file.is_some());
        self.history.push(step);
        written
    }

    /// Ends the edit by applying the state from before it, through the same path as every
    /// other change. No undo step. A record that a file change, an undo or a redo wrote during
    /// the edit goes back to that state: it is what was committed, and what its file holds.
    pub fn cancel(&mut self, edit: Edit) -> Result<(), ProjectError> {
        let mut step = edit.step;
        let instances = &self.instances;
        self.history
            .close(&mut step, |id| instances.get(id).cloned());
        self.sync_project_file();
        let applied = self.apply(step.changes(Side::Before), Source::History)?;
        self.write_step(&step, Side::Before, &applied)
    }

    /// Ends the edit with no undo step and without going back: what it published stays. For an
    /// edit whose records another edit has since deleted or replaced, as its last write: that
    /// edit's step already starts from the state before this one, so finishing this one too
    /// would be a second step for the same change, and cancelling would undo the newer write.
    pub fn abandon(&mut self, edit: Edit) {
        let mut step = edit.step;
        let instances = &self.instances;
        self.history
            .close(&mut step, |id| instances.get(id).cloned());
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
        self.bring_back(Side::Before)
    }

    pub fn redo(&mut self) -> Result<Option<String>, ProjectError> {
        self.bring_back(Side::After)
    }

    /// Applies one side of the next step of undo (before) or redo (after) and writes it.
    fn bring_back(&mut self, side: Side) -> Result<Option<String>, ProjectError> {
        self.finish_all_runs()?;
        let Some(step) = self.history.take(side) else {
            return Ok(None);
        };
        self.sync_project_file();
        self.history.end_outside_step();
        let applied = self.apply(step.changes(side), Source::History)?;
        let written = self.write_step(&step, side, &applied);
        let label = step.label.clone();
        self.history.put_back(side, step);
        written.map(|()| Some(label))
    }

    /// Writes what the step names and what applying it touched. The two differ when a delete
    /// took along instances that were created after the step.
    fn write_step(
        &mut self,
        step: &Step,
        side: Side,
        applied: &Applied,
    ) -> Result<(), ProjectError> {
        let touched = applied.records.iter().map(|change| &change.id);
        let project_file = step.project_file.is_some() || applied.project_file.is_some();
        let (gone, back): (Vec<_>, Vec<_>) =
            (step.unloaded(side)).partition(|(_, bytes)| bytes.is_none());
        // First, so that the folders of deleted records are empty and go too.
        let removed = self.write_unloaded(gone);
        let written = self.write(step.records.keys().chain(touched), project_file);
        let restored = self.write_unloaded(back);
        removed.and(written).and(restored)
    }

    /// Forgets every undo and redo step. A new project starts like this: making its default
    /// content is not something to undo.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// A number that changes whenever an undo step is made, grows, or is undone or redone:
    /// whenever what the files hold became a result someone saw. For the history.
    pub fn steps_changed(&self) -> u64 {
        self.history.generation()
    }

    /// The label of the step that `undo` would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.history.label(Side::Before)
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.label(Side::After)
    }
}
