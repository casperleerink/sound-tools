//! The editing service: staged changes, edits with gestures, and undo and redo, which move
//! along the history of the folder (`history.rs`).
//!
//! An edit lives in memory until it finishes: then its files are written and the folder is a
//! new point. Cancelling reads its records back from the files, which never held the gesture.

use std::collections::BTreeSet;
use std::time::Instant;

use crate::history::ObjectId;

use super::file::{ProjectFile, SavedConnection};
use super::instance::{Instance, InstanceId, Record, State};
use super::storage::{self, Form};
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
}

/// An edit in progress: a drag, or any other group of changes that becomes one point of the
/// history. The files keep the state before it until it finishes, so cancelling needs no
/// reverse operation from the tool. Several edits can be open at once; last write wins.
#[must_use = "finish or cancel the edit"]
pub struct Edit {
    label: String,
    records: BTreeSet<InstanceId>,
    project_file: bool,
}

impl Edit {
    /// It published nothing yet.
    pub(super) fn is_empty(&self) -> bool {
        self.records.is_empty() && !self.project_file
    }

    /// The records it published over.
    pub(super) fn records(&self) -> impl Iterator<Item = &InstanceId> {
        self.records.iter()
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
    /// Begins a request: until it ends, every outside change joins one point named `label`,
    /// however far apart the changes are. For an agent that knows where a request begins and
    /// ends. An interface edit, an undo or a redo still ends the point, and the changes of the
    /// request after it make a new one with the same label. A request that is still open is
    /// replaced.
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
    /// long after its turn, must not let the outside changes of that moment join the point.
    pub fn end_request_at(&mut self, at: Instant) {
        self.history.end_request_at(at);
    }

    /// Begins an edit. `label` names the point, for example "Change frequency".
    pub fn begin(&self, label: &str) -> Edit {
        Edit {
            label: label.to_string(),
            records: BTreeSet::new(),
            project_file: false,
        }
    }

    /// Applies changes now, as one group. Sound and views follow, no file is written. Call it
    /// as often as the gesture moves. On an error nothing of the group is applied.
    ///
    /// The first publish of an edit first finishes the runs it may finish (see
    /// [`Self::publish_run`]), so a run's point holds none of this edit.
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
        for change in applied.records {
            if edit.records.insert(change.id.clone()) {
                self.history.hold(&change.id);
            }
        }
        edit.project_file |= applied.project_file.is_some();
        Ok(())
    }

    /// Applies changes to the sound and the state, and to nothing else: no file is written and
    /// there is no point. The one change a read-only project takes, for a
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

    /// Ends the edit: writes every record it touched, once, and the folder is a new point. The
    /// files take the state now, whoever wrote last.
    ///
    /// On a write error the edit stays applied, and the problem is reported.
    pub fn finish(&mut self, edit: Edit) -> Result<(), ProjectError> {
        self.release(&edit);
        let written = self.write(edit.records.iter(), edit.project_file);
        let recorded = self.history.record(&edit.label).map_err(ProjectError::from);
        written.and(recorded)
    }

    /// Ends the edit by reading its records back from the files, which still hold the state
    /// from before it, through the same path as a change from outside. No point. A record that
    /// a file change or an undo wrote during the edit takes what its file holds.
    pub fn cancel(&mut self, edit: Edit) -> Result<(), ProjectError> {
        self.release(&edit);
        let mut paths = Vec::new();
        for id in &edit.records {
            self.storage.forget(id);
            paths.push(self.storage.record_path(id, Form::File));
        }
        if edit.project_file {
            self.storage.observe_project_file(None);
            paths.push(self.storage.root().join(storage::PROJECT_FILE));
        }
        self.apply_paths(&paths, Source::Outside).map(drop)
    }

    /// Ends the edit without going back and without a point: what it published stays, and the
    /// next point takes it. For an edit whose records another edit has since deleted or
    /// replaced, as its last write.
    pub fn abandon(&mut self, edit: Edit) {
        self.release(&edit);
    }

    fn release(&mut self, edit: &Edit) {
        for id in &edit.records {
            self.history.release(id);
        }
    }

    /// One finished edit: begin, publish, finish.
    pub fn commit(&mut self, label: &str, changes: Changes) -> Result<(), ProjectError> {
        let mut edit = self.begin(label);
        self.publish(&mut edit, changes)?;
        self.finish(edit)
    }

    /// Goes back to the point the project came from. Returns the label of the change it takes
    /// back, or `None` when there is nothing to undo.
    pub fn undo(&mut self) -> Result<Option<String>, ProjectError> {
        self.finish_all_runs()?;
        let label = self.undo_label().map(str::to_string);
        let target = (self.history.store.as_ref()).and_then(|store| store.undo_target());
        match target {
            Some(target) => self.go_to(target).map(|()| label),
            None => Ok(None),
        }
    }

    /// Goes to the newest point that came from the one now.
    pub fn redo(&mut self) -> Result<Option<String>, ProjectError> {
        self.finish_all_runs()?;
        let label = self.redo_label().map(str::to_string);
        let target = (self.history.store.as_ref()).and_then(|store| store.redo_target());
        match target {
            Some(target) => self.go_to(target).map(|()| label),
            None => Ok(None),
        }
    }

    /// Makes the folder what it was at `point`, and the project follows it as it follows a
    /// change from outside. The next change starts a version from there. What the folder holds
    /// that the history has not seen yet, such as a write of an agent the watcher has not
    /// delivered, is saved as a point first, so nothing is lost.
    pub fn go_to(&mut self, point: ObjectId) -> Result<(), ProjectError> {
        self.finish_all_runs()?;
        self.store()?;
        self.history.record(super::outside::OUTSIDE_LABEL)?;
        let files = self.store()?.go_to(point)?;
        self.history.moved();
        self.write_and_apply(files)
    }

    /// Merges the version whose latest point is `tip` into where the project is now, as a new
    /// point named `label`, see [`crate::history::History::merge`].
    pub fn merge(
        &mut self,
        tip: ObjectId,
        keep_mine: bool,
        label: &str,
    ) -> Result<(), ProjectError> {
        self.finish_all_runs()?;
        let files = self.store()?.merge(tip, keep_mine)?;
        let applied = self.write_and_apply(files);
        self.history.record(label)?;
        applied
    }

    /// Names the version that starts at `start`, for its latest point `tip`.
    pub fn name_version(
        &mut self,
        start: ObjectId,
        name: &str,
        tip: ObjectId,
    ) -> Result<(), ProjectError> {
        Ok(self.store()?.set_name(start, name, tip)?)
    }

    /// Sets the version that starts at `start` aside, so it folds away, or brings it back.
    pub fn set_aside(&mut self, start: ObjectId, aside: bool) -> Result<(), ProjectError> {
        Ok(self.store()?.set_aside(start, aside)?)
    }

    /// The history of the folder, once [`Self::keep_history`] opened it.
    pub fn history(&self) -> Option<&crate::history::History> {
        self.history.store.as_ref()
    }

    fn store(&mut self) -> Result<&mut crate::history::History, ProjectError> {
        if self.read_only {
            return Err(ProjectError::ReadOnly);
        }
        self.history.store.as_mut().ok_or(ProjectError::NoHistory)
    }

    /// Writes these files, relative to the project folder, or removes them for `None`, and
    /// applies them as a change from outside.
    fn write_and_apply(&mut self, files: crate::history::Files) -> Result<(), ProjectError> {
        let root = self.storage.root().to_path_buf();
        let mut paths = Vec::new();
        for (relative, bytes) in files {
            let path = root.join(&relative);
            let written = match bytes {
                Some(bytes) => storage::write_atomically(&path, &bytes),
                None => remove_with_empty_folders(&path, &root),
            };
            written.map_err(|source| {
                ProjectError::Storage(storage::StorageError::Io {
                    path: relative.display().to_string(),
                    source,
                })
            })?;
            paths.push(path);
        }
        self.apply_paths(&paths, Source::Outside).map(drop)
    }

    /// Starts the history again from the folder as it is: nothing before can be undone. A new
    /// project starts like this: making its default content is not something to undo.
    pub fn clear_history(&mut self) {
        if let Some(store) = &mut self.history.store
            && let Err(error) = store.start_again(super::START_LABEL)
        {
            self.report_problem(
                crate::history::HISTORY_FOLDER.to_string(),
                error.to_string(),
            );
        }
    }

    /// The label of the change that `undo` would take back.
    pub fn undo_label(&self) -> Option<&str> {
        self.history
            .label(|store| store.now().filter(|_| store.undo_target().is_some()))
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.history.label(crate::history::History::redo_target)
    }
}

/// Removes the file, then each folder above it that it leaves empty, below the folders at the
/// top, such as `state/`, which the watcher watches.
fn remove_with_empty_folders(
    path: &std::path::Path,
    root: &std::path::Path,
) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    let mut folder = path.parent();
    while let Some(current) =
        folder.filter(|folder| folder.parent().is_some_and(|parent| parent != root))
    {
        // A folder that still holds something stays, and so does every one above it.
        if std::fs::remove_dir(current).is_err() {
            break;
        }
        folder = current.parent();
    }
    Ok(())
}
