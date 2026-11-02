//! The editing service: staged changes, edits with gestures, and the moves through history:
//! undo, redo, going to a point and merging. The points themselves are kept in `history.rs`.
//!
//! Interface edits, file changes, undo and redo all end in `Project::apply`. This module
//! holds what they need around it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use super::file::{ProjectFile, SavedConnection};
use super::history::{Going, History, Origin, PointId};
use super::instance::{Instance, InstanceId, Record, State};
use super::outside::OUTSIDE_LABEL;
use super::{Project, ProjectError, ProjectEvent, Source};
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

/// An edit in progress: a drag, or any other group of changes that becomes one point.
/// It remembers the state before its first change, so finishing and cancelling need no
/// reverse operation from the tool. Several edits can be open at once; last write wins.
#[must_use = "finish or cancel the edit"]
pub struct Edit {
    label: String,
    /// Each record it published over, as it was before. `None`: there was none.
    pub(super) records: BTreeMap<InstanceId, Option<Record>>,
    /// `project.json` before, once it changed it.
    pub(super) project_file: Option<ProjectFile>,
}

impl Edit {
    /// It published nothing yet.
    pub(super) fn is_empty(&self) -> bool {
        self.records.is_empty() && self.project_file.is_none()
    }

    fn absorb(&mut self, applied: Applied) {
        for change in applied.records {
            self.records.entry(change.id).or_insert(change.before);
        }
        if let Some((before, _)) = applied.project_file {
            self.project_file.get_or_insert(before);
        }
    }

    /// The changes that bring back the state before the edit.
    fn changes_back(&self) -> Vec<Change> {
        let project_file = self.project_file.clone().map(Change::ProjectFile);
        let records = self.records.iter().map(|(id, before)| match before {
            Some(record) => Change::Set(id.clone(), record.clone()),
            None => Change::Delete(id.clone()),
        });
        project_file.into_iter().chain(records).collect()
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
    /// request after it make a new point with the same label. A request that is still open is
    /// replaced.
    pub fn begin_request(&mut self, label: &str) {
        if let Some(history) = &mut self.history {
            history.begin_request(label);
        }
    }

    /// Ends the request. Outside changes heard less than
    /// [`GROUPING_WINDOW`](super::GROUPING_WINDOW) later still join its point, because the
    /// watcher may hear the last write of a request only after the request has ended. Later
    /// changes follow [`OUTSIDE_POINT_WINDOW`](super::OUTSIDE_POINT_WINDOW) again, in a new
    /// point.
    pub fn end_request(&mut self) {
        self.end_request_at(Instant::now());
    }

    /// [`Self::end_request`] with the time of the end given, for tests of the undo grouping.
    ///
    /// The first end counts: an end said again later, such as when the agent's process exits
    /// long after its turn, must not let the outside changes of that moment join the step.
    pub fn end_request_at(&mut self, at: Instant) {
        if let Some(history) = &mut self.history {
            history.end_request_at(at);
        }
    }

    /// Begins an edit. `label` names its point, for example "Change frequency".
    pub fn begin(&self, label: &str) -> Edit {
        Edit {
            label: label.to_string(),
            records: BTreeMap::new(),
            project_file: None,
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
        self.open_edits.hold(edit, &applied);
        edit.absorb(applied);
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

    /// Ends the edit and writes every record it touched, once, and saves the folder as a
    /// point. The edit runs from the committed state before it to the state now, whoever wrote
    /// last. That is the state before the edit, or what a file change, an undo or a redo wrote
    /// during it.
    ///
    /// On a write error the edit stays applied, and the problem is reported.
    pub fn finish(&mut self, mut edit: Edit) -> Result<(), ProjectError> {
        let instances = &self.instances;
        self.open_edits
            .close(&mut edit, |id| instances.get(id).cloned());
        // What ended where it began, such as a record made and deleted, is not written.
        edit.records
            .retain(|id, before| match (before, self.instances.get(id)) {
                (Some(before), Some(now)) => !before.equals(now),
                (None, None) => false,
                _ => true,
            });
        let project_file =
            (edit.project_file.as_ref()).is_some_and(|before| *before != self.project_file);
        let written = self.write(edit.records.keys(), project_file);
        let saved = self.save_point(Origin::Edit, &edit.label);
        written.and(saved)
    }

    /// Ends the edit by applying the state from before it, through the same path as every
    /// other change. A record that a file change, an undo or a redo wrote during the edit goes
    /// back to that state: it is what was committed, and what its file holds. So nothing is
    /// written, but the files of instances the edit's delete takes along, made by a finished
    /// edit after it began.
    pub fn cancel(&mut self, mut edit: Edit) -> Result<(), ProjectError> {
        let instances = &self.instances;
        self.open_edits
            .close(&mut edit, |id| instances.get(id).cloned());
        self.sync_project_file();
        let applied = self.apply(edit.changes_back(), Source::Outside)?;
        let touched = applied.records.iter().map(|change| &change.id);
        let gone: Vec<InstanceId> = (edit.records.keys().chain(touched))
            .filter(|id| !self.instances.contains_key(*id))
            .cloned()
            .collect();
        let written = self.write(gone.iter(), false);
        let saved = self.save_point(Origin::Edit, &edit.label);
        written.and(saved)
    }

    /// Ends the edit with no point and without going back: what it published stays. For an
    /// edit whose records another edit has since deleted or replaced, as its last write: that
    /// edit's point already starts from the state before this one, so finishing this one too
    /// would be a second point for the same change, and cancelling would undo the newer write.
    pub fn abandon(&mut self, mut edit: Edit) {
        let instances = &self.instances;
        self.open_edits
            .close(&mut edit, |id| instances.get(id).cloned());
    }

    /// One finished edit: begin, publish, finish.
    pub fn commit(&mut self, label: &str, changes: Changes) -> Result<(), ProjectError> {
        let mut edit = self.begin(label);
        self.publish(&mut edit, changes)?;
        self.finish(edit)
    }

    /// The history of the folder. `None` for a project open read-only, or one whose history
    /// could not be opened, which its problem says.
    pub fn history(&self) -> Option<&History> {
        self.history.as_ref()
    }

    /// Undo: the files go back to the point the current one came from, and the project follows
    /// them as it follows an outside change. No new point. Returns the label of the point it
    /// took back, or `None` at the start.
    pub fn undo(&mut self) -> Result<Option<String>, ProjectError> {
        self.undo_going(Going::Undo)
    }

    /// Undo that keeps what it takes back as a version, as going back in the history does.
    pub fn undo_keeping_version(&mut self) -> Result<Option<String>, ProjectError> {
        self.undo_going(Going::Back)
    }

    fn undo_going(&mut self, going: Going) -> Result<Option<String>, ProjectError> {
        self.before_going()?;
        let label = self.undo_label();
        let target = self.history.as_ref().map(History::undo_target).transpose();
        match target.map_err(ProjectError::History)?.flatten() {
            Some(target) => self.go(target, going).map(|()| label),
            None => Ok(None),
        }
    }

    /// Redo: the files go to the newest point that came from the current one. Returns its
    /// label, or `None` when there is none.
    pub fn redo(&mut self) -> Result<Option<String>, ProjectError> {
        self.before_going()?;
        let Some(history) = &self.history else {
            return Ok(None);
        };
        let target = history.newest_child(history.current());
        match target.map_err(ProjectError::History)? {
            Some(target) => {
                let label = self.redo_label();
                self.go(target, Going::Redo).map(|()| label)
            }
            None => Ok(None),
        }
    }

    /// Goes to any point, as undo does. The next change after it starts a version.
    pub fn go_to(&mut self, point: PointId) -> Result<(), ProjectError> {
        self.before_going()?;
        if self
            .history
            .as_ref()
            .is_none_or(|history| history.current() == point)
        {
            return Ok(());
        }
        self.go(point, Going::Back)?;
        self.push_event(ProjectEvent::WentBack(point));
        Ok(())
    }

    /// Merges the version that begins at `start` into the current point: a new point with two
    /// parents. Per file, what only that version changed comes over; where both changed a
    /// file, that version wins, or with `keep_mine` the current one does.
    pub fn merge(&mut self, start: PointId, keep_mine: bool) -> Result<(), ProjectError> {
        self.before_going()?;
        let Some(history) = &mut self.history else {
            return Ok(());
        };
        let paths = history
            .merge(start, keep_mine)
            .map_err(ProjectError::History)?;
        self.follow_files(&paths)
    }

    /// Makes undone work a version.
    pub fn keep_as_version(&mut self, start: PointId) -> Result<(), ProjectError> {
        self.change_history(|history| history.keep_as_version(start))
    }

    /// Folds a version away by hand, or brings it back.
    pub fn set_aside(&mut self, start: PointId, aside: bool) -> Result<(), ProjectError> {
        self.change_history(|history| history.set_aside(start, aside))
    }

    /// Names a version by hand. `None` gives naming back.
    pub fn rename_version(
        &mut self,
        start: PointId,
        name: Option<String>,
    ) -> Result<(), ProjectError> {
        self.change_history(|history| history.rename(start, name))
    }

    /// A name found for a version from what it changed up to its tip `at`. Never over a name
    /// given by hand.
    pub fn name_version(
        &mut self,
        start: PointId,
        name: String,
        at: PointId,
    ) -> Result<(), ProjectError> {
        self.change_history(|history| history.name(start, name, at))
    }

    fn change_history(
        &mut self,
        change: impl FnOnce(&mut History) -> std::io::Result<()>,
    ) -> Result<(), ProjectError> {
        match &mut self.history {
            Some(history) => change(history).map_err(ProjectError::History),
            None => Ok(()),
        }
    }

    /// Before going anywhere: what the folder holds that history has not seen yet, such as an
    /// agent's write the watcher has not delivered, is saved as a point, and then open runs
    /// finish. In that order, so a run's write cannot replace those bytes unseen.
    fn before_going(&mut self) -> Result<(), ProjectError> {
        self.save_point(Origin::Outside(Instant::now()), OUTSIDE_LABEL)?;
        self.finish_all_runs()
    }

    fn go(&mut self, to: PointId, going: Going) -> Result<(), ProjectError> {
        let Some(history) = &mut self.history else {
            return Ok(());
        };
        let paths = history.go(to, going).map_err(ProjectError::History)?;
        self.follow_files(&paths)
    }

    /// The live project follows files history wrote, as it follows an outside change.
    fn follow_files(&mut self, paths: &[PathBuf]) -> Result<(), ProjectError> {
        self.apply_paths(paths, Source::Outside).map(drop)
    }

    pub(crate) fn save_point(&mut self, origin: Origin, label: &str) -> Result<(), ProjectError> {
        match &mut self.history {
            Some(history) => (history.save(origin, label).map(drop)).map_err(ProjectError::History),
            None => Ok(()),
        }
    }

    /// Starts the history again from the folder as it is. A new project starts like this:
    /// making its default content is not something to undo.
    pub fn clear_history(&mut self) -> Result<(), ProjectError> {
        if self.history.take().is_none() {
            return Ok(());
        }
        let folder = self.storage.root().join(super::history::HISTORY_FOLDER);
        let reopened = std::fs::remove_dir_all(&folder)
            .and_then(|()| History::open(self.storage.root(), self.registry.kept_out_of_history()));
        self.history = Some(reopened.map_err(ProjectError::History)?);
        Ok(())
    }

    /// The label of the point that `undo` would take back.
    pub fn undo_label(&self) -> Option<String> {
        self.history.as_ref()?.undo_label()
    }

    pub fn redo_label(&self) -> Option<String> {
        self.history.as_ref()?.redo_label()
    }
}
