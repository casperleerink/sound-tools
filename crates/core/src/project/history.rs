//! The undo history: undo and redo steps, the committed state under open edits, and how
//! outside changes group into steps.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::editing::{Applied, Change, RecordChange};
use super::file::ProjectFile;
use super::instance::{InstanceId, Record};
use super::{GROUPING_WINDOW, Source};

/// Everything one undo step touched, with the state before the first change and after the
/// last. Undo applies the before side as one group, redo the after side.
#[derive(Default)]
pub(super) struct Step {
    pub label: String,
    pub records: BTreeMap<InstanceId, (Option<Record>, Option<Record>)>,
    pub project_file: Option<(ProjectFile, ProjectFile)>,
}

impl Step {
    pub(super) fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            ..Self::default()
        }
    }

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
    pub(super) fn drop_unchanged(&mut self) {
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
    pub(super) fn changes(&self, side: Side) -> Vec<Change> {
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

/// One side of a step. Undo brings back the before side, redo the after side.
#[derive(Copy, Clone)]
pub(super) enum Side {
    Before,
    After,
}

/// Groups of outside file changes that follow each other within this time are one undo step,
/// when no request is open (see [`Project::begin_request`](super::Project::begin_request)).
///
/// An agent writes the files of one request seconds apart, far more than the quiet window
/// that groups them for live apply. Without this, undo of a track that an agent added takes
/// one step per file. It is a heuristic for an agent that cannot say where its request begins
/// and ends, such as one in a terminal.
pub const OUTSIDE_UNDO_WINDOW: Duration = Duration::from_secs(15);

/// A request of [`Project::begin_request`](super::Project::begin_request), until the watcher is
/// quiet after its end.
struct Request {
    label: String,
    /// When [`Project::end_request`](super::Project::end_request) was called.
    ended: Option<Instant>,
}

#[derive(Default)]
pub(super) struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// When the step on top of `undo` is an outside step and nothing came after it: when its
    /// last group was heard, see
    /// [`Project::apply_outside_changes_at`](super::Project::apply_outside_changes_at). An
    /// interface edit, an undo and a redo all clear it.
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
    pub(super) fn close(&mut self, step: &mut Step) {
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
                let mut step = Step::new(label);
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

    /// See [`Project::begin_request`](super::Project::begin_request).
    pub(super) fn begin_request(&mut self, label: &str) {
        self.request = Some(Request {
            label: label.to_string(),
            ended: None,
        });
        // The request starts its own step, even right after other outside changes.
        self.last_outside = None;
    }

    /// See [`Project::end_request_at`](super::Project::end_request_at).
    pub(super) fn end_request_at(&mut self, at: Instant) {
        if let Some(request) = &mut self.request {
            request.ended.get_or_insert(at);
        }
    }

    /// Takes the step whose `side` undo (before) or redo (after) brings back next.
    pub(super) fn take(&mut self, side: Side) -> Option<Step> {
        match side {
            Side::Before => self.undo.pop(),
            Side::After => self.redo.pop(),
        }
    }

    /// Puts a step whose `side` was brought back where it can go the other way.
    pub(super) fn put_back(&mut self, side: Side, step: Step) {
        match side {
            Side::Before => self.redo.push(step),
            Side::After => self.undo.push(step),
        }
    }

    /// Outside changes from now on make a new step.
    pub(super) fn end_outside_step(&mut self) {
        self.last_outside = None;
    }

    /// The label of the step that [`Self::take`] gives for `side`.
    pub(super) fn label(&self, side: Side) -> Option<&str> {
        let steps = match side {
            Side::Before => &self.undo,
            Side::After => &self.redo,
        };
        steps.last().map(|step| step.label.as_str())
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}
