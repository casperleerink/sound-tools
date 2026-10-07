//! The editing service: staged changes, edits with gestures, and undo history.
//!
//! Interface edits, file changes, undo and redo all end in `Project::apply`. This module
//! holds what they need around it.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::file::{ProjectFile, SavedConnection};
use super::instance::{Instance, InstanceId, Record, State};
use super::{GROUPING_WINDOW, Project, ProjectError, Source};
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

/// What a derive gives back: the changes it adds to the group it runs in, and what it could
/// not compute. See [`ToolRegistration::derive`](super::ToolRegistration::derive).
#[derive(Default)]
pub struct Derived {
    pub(crate) changes: Changes,
    pub(crate) problems: Vec<String>,
}

impl Derived {
    /// The changes this derive adds to the group.
    pub fn changes(&mut self) -> &mut Changes {
        &mut self.changes
    }

    /// Says that part of the derived state could not be made, without failing the edit. The
    /// message is listed in [`Project::problems`](super::Project::problems) on the record's
    /// path until this derive runs again without it, exactly as
    /// [`BehaviourContext::problem`](super::BehaviourContext::problem) does for a behaviour.
    pub fn problem(&mut self, message: impl Into<String>) {
        self.problems.push(message.into());
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
    /// The records a derive wrote. They are in `records` like any other change; this says
    /// which files a group of outside changes still has to write, because those files hold
    /// what an agent wrote and not what a derive made of it.
    pub derived: Vec<InstanceId>,
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
    pub(super) fn absorb(&mut self, applied: Applied) {
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

    /// Forgets what ended where it began, for example a record that was made and deleted.
    fn drop_unchanged(&mut self) {
        self.records
            .retain(|_, (before, after)| match (before, after) {
                (Some(before), Some(after)) => !before.equals(after),
                (None, None) => false,
                _ => true,
            });
        self.project_file.take_if(|(before, after)| before == after);
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

/// Groups of outside file changes that follow each other within this time are one undo step,
/// when no request is open (see [`Project::begin_request`]).
///
/// An agent writes the files of one request seconds apart, far more than the quiet window
/// that groups them for live apply. Without this, undo of a track that an agent added takes
/// one step per file. It is a heuristic for an agent that cannot say where its request begins
/// and ends, such as one in a terminal.
pub const OUTSIDE_UNDO_WINDOW: Duration = Duration::from_secs(15);

/// A request of [`Project::begin_request`], until the watcher is quiet after its end.
struct Request {
    label: String,
    /// When [`Project::end_request`] was called.
    ended: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// When the step on top of `undo` is an outside step and nothing came after it: when its
    /// last group was heard, see [`Project::apply_outside_changes_at`]. An interface edit, an
    /// undo and a redo all clear it.
    last_outside: Option<Instant>,
    request: Option<Request>,
    /// The committed state of every record that an open edit has published over: what it was
    /// before the first publish, or what a file change, an undo or a redo made of it since.
    /// The live state of such a record is the middle of a gesture, which no undo step may
    /// hold: nobody ever saw it as a result.
    committed: BTreeMap<InstanceId, Option<Record>>,
    /// The same for `project.json`, which a tempo drag changes per mouse move.
    committed_project_file: Option<ProjectFile>,
}

impl History {
    /// Keeps `committed` current after a state application. A change from a file or from
    /// history to a record under an open edit gets the committed state as its before side, in
    /// place of the state in the middle of the gesture.
    pub(super) fn note_committed(&mut self, change: &mut RecordChange, source: Source) {
        match source {
            Source::Load => {}
            Source::Interface => {
                let before = || change.before.clone();
                self.committed
                    .entry(change.id.clone())
                    .or_insert_with(before);
            }
            Source::Outside | Source::History => {
                if let Some(committed) = self.committed.get_mut(&change.id) {
                    change.before = std::mem::replace(committed, change.after.clone());
                }
            }
        }
    }

    /// The same for `project.json`. A tempo drag publishes a whole tempo map per mouse move,
    /// so without this a file change during the drag would take the tempo of one mouse move as
    /// its before side, and undo would land there.
    pub(super) fn note_committed_project_file(
        &mut self,
        change: &mut Option<(ProjectFile, ProjectFile)>,
        source: Source,
    ) {
        let Some((before, after)) = change else {
            return;
        };
        match source {
            Source::Load => {}
            Source::Interface => {
                self.committed_project_file
                    .get_or_insert_with(|| before.clone());
            }
            Source::Outside | Source::History => {
                if let Some(committed) = &mut self.committed_project_file {
                    *before = std::mem::replace(committed, after.clone());
                }
            }
        }
    }

    /// The edit of `step` ends. Its before side becomes what was committed under it, so that
    /// the step starts where the step before it ended.
    fn close(&mut self, step: &mut Step) {
        for (id, (before, _)) in &mut step.records {
            if let Some(committed) = self.committed.remove(id) {
                *before = committed;
            }
        }
        if let Some((before, _)) = &mut step.project_file
            && let Some(committed) = self.committed_project_file.take()
        {
            *before = committed;
        }
    }

    /// A new step. What was undone before can no longer be redone.
    pub(super) fn push(&mut self, step: Step) {
        self.last_outside = None;
        if !step.is_empty() {
            self.undo.push(step);
            self.redo.clear();
        }
    }

    /// A group of outside changes, heard at `at`. It joins the step on top when that is an
    /// outside step with nothing in between, of the open request or from less than
    /// [`OUTSIDE_UNDO_WINDOW`] before. The joined step keeps its older before side and takes
    /// the newer after side.
    pub(super) fn push_outside(&mut self, label: &str, applied: Applied, at: Instant) {
        let quiet_since_end = self
            .request
            .as_ref()
            .and_then(|request| request.ended)
            .is_some_and(|ended| at.saturating_duration_since(ended) >= GROUPING_WINDOW);
        if quiet_since_end {
            self.request = None;
            self.last_outside = None;
        }
        let recent = self.last_outside.is_some_and(|last| {
            self.request.is_some() || at.saturating_duration_since(last) < OUTSIDE_UNDO_WINDOW
        });
        match self.undo.last_mut().filter(|_| recent) {
            Some(step) => {
                step.absorb(applied);
                step.drop_unchanged();
                self.last_outside = Some(at);
                // An agent that takes back what it just wrote leaves nothing to undo.
                if step.is_empty() {
                    self.undo.pop();
                    self.last_outside = None;
                }
            }
            None => {
                let label = self
                    .request
                    .as_ref()
                    .map_or(label, |request| request.label.as_str());
                let mut step = Step {
                    label: label.to_string(),
                    ..Step::default()
                };
                step.absorb(applied);
                // A file may bring back what was committed under an open edit.
                step.drop_unchanged();
                let is_empty = step.is_empty();
                self.push(step);
                // An empty group is no step, and then the top is not known to be an outside one.
                self.last_outside = (!is_empty).then_some(at);
            }
        }
    }

    pub(super) fn clear(&mut self) {
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

/// Every record of `changes` is one its tool accepts.
fn validate(changes: &Changes) -> Result<(), ProjectError> {
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
    Ok(())
}

impl Project {
    /// Begins a request: until it ends, every outside change joins one undo step named
    /// `label`, however far apart the changes are. For an agent that knows where a request
    /// begins and ends. An interface edit, an undo or a redo still ends the step, and the
    /// changes of the request after it make a new step with the same label. A request that is
    /// still open is replaced.
    pub fn begin_request(&mut self, label: &str) {
        self.history.request = Some(Request {
            label: label.to_string(),
            ended: None,
        });
        // The request starts its own step, even right after other outside changes.
        self.history.last_outside = None;
    }

    /// Ends the request. Outside changes heard less than [`GROUPING_WINDOW`] later still join
    /// its step, because the watcher may hear the last write of a request only after the
    /// request has ended. Later changes follow [`OUTSIDE_UNDO_WINDOW`] again, in a new step.
    pub fn end_request(&mut self) {
        self.end_request_at(Instant::now());
    }

    /// [`Self::end_request`] with the time of the end given, for tests of the undo grouping.
    ///
    /// The first end counts: an end said again later, such as when the agent's process exits
    /// long after its turn, must not let the outside changes of that moment join the step.
    pub fn end_request_at(&mut self, at: Instant) {
        if let Some(request) = &mut self.history.request {
            request.ended.get_or_insert(at);
        }
    }

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
        validate(&changes)?;
        // A derive may rewrite the tempo map, so a change to a record that has one touches
        // `project.json` as surely as a tempo edit does.
        let touches_project_file = changes.changes.iter().any(|change| match change {
            Change::Set(_, record) => self.derives(record.tool),
            _ => true,
        });
        if touches_project_file {
            self.sync_project_file();
        }
        let applied = self.apply(changes.changes, Source::Interface)?;
        edit.step.absorb(applied);
        Ok(())
    }

    /// Applies changes to the sound and the state, and to nothing else: no file is written, no
    /// derive runs and there is no undo step. The one change a read-only project takes, for a
    /// render that plays it other than its files say, such as with one track soloed. A project
    /// that writes refuses with [`ProjectError::WritesFiles`]: its next edit of these records
    /// would save them, and the folder would no longer be the truth.
    pub fn apply_in_memory(&mut self, changes: Changes) -> Result<(), ProjectError> {
        if !self.read_only {
            return Err(ProjectError::WritesFiles);
        }
        validate(&changes)?;
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

    /// Ends the edit as one undo step and writes every record it touched, once. The step runs
    /// from the committed state before it to the state now, whoever wrote last. That is the
    /// state before the edit, or what a file change, an undo or a redo wrote during it.
    ///
    /// On a write error the edit stays applied and undoable, and the problem is reported.
    pub fn finish(&mut self, edit: Edit) -> Result<(), ProjectError> {
        let mut step = edit.step;
        self.history.close(&mut step);
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
        self.history.close(&mut step);
        self.sync_project_file();
        let applied = self.apply(step.changes(Side::Before), Source::History)?;
        self.write_step(&step, &applied)
    }

    /// Ends the edit with no undo step and without going back: what it published stays. For an
    /// edit whose records another edit has since deleted or replaced, as its last write: that
    /// edit's step already starts from the state before this one, so finishing this one too
    /// would be a second step for the same change, and cancelling would undo the newer write.
    pub fn abandon(&mut self, edit: Edit) {
        let mut step = edit.step;
        self.history.close(&mut step);
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
