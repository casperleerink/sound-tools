//! Open edits: what each touched, and the committed state under them, so finishing writes the
//! right records and cancelling goes back to what was committed.

use std::collections::{BTreeMap, BTreeSet};

use super::Source;
use super::editing::{Applied, Change, RecordChange};
use super::file::ProjectFile;
use super::instance::{InstanceId, Record};

/// Everything one edit touched, with the state before its first change and after its last.
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

    pub(super) fn is_empty(&self) -> bool {
        self.records.is_empty() && self.project_file.is_none()
    }

    /// The changes that bring back the state before the step.
    pub(super) fn changes_back(&self) -> Vec<Change> {
        let mut changes = Vec::new();
        if let Some((before, _)) = &self.project_file {
            changes.push(Change::ProjectFile(before.clone()));
        }
        for (id, (before, _)) in &self.records {
            changes.push(match before {
                Some(record) => Change::Set(id.clone(), record.clone()),
                None => Change::Delete(id.clone()),
            });
        }
        changes
    }
}

#[derive(Default)]
pub(super) struct OpenEdits {
    /// The committed state of every record that an open edit has published over: what it was
    /// before the first publish, or what a file change, an undo or a redo made of it since.
    /// The live state of such a record is the middle of a gesture, which no undo step may
    /// hold: nobody ever saw it as a result.
    committed: BTreeMap<InstanceId, Option<Record>>,
    /// How many open edits have published over each record of `committed`.
    holders: BTreeMap<InstanceId, usize>,
    /// The same for `project.json`, which a tempo drag changes per mouse move.
    committed_project_file: Option<ProjectFile>,
}

impl OpenEdits {
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
            Source::Outside | Source::Cancel => {
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
            Source::Outside | Source::Cancel => {
                if let Some(committed) = &mut self.committed_project_file {
                    *before = std::mem::replace(committed, after.clone());
                }
            }
        }
    }

    /// The open edit of `step` publishes `applied`: it holds each record it publishes over
    /// until it closes.
    pub(super) fn hold(&mut self, step: &Step, applied: &Applied) {
        // A record replaced in one publish, deleted then written, has two changes.
        let new: BTreeSet<&InstanceId> = (applied.records.iter())
            .map(|change| &change.id)
            .filter(|id| !step.records.contains_key(*id))
            .collect();
        for id in new {
            *self.holders.entry(id.clone()).or_default() += 1;
        }
    }

    /// How many open edits hold the record `id`.
    pub(super) fn holders(&self, id: &InstanceId) -> usize {
        self.holders.get(id).copied().unwrap_or(0)
    }

    /// The edit of `step` ends. Its before side becomes what was committed under it, so that
    /// the step starts where the step before it ended. For another open edit on the same
    /// record, the record as it is `now` is committed from here.
    pub(super) fn close(&mut self, step: &mut Step, now: impl Fn(&InstanceId) -> Option<Record>) {
        for (id, (before, _)) in &mut step.records {
            let others = self
                .holders
                .get(id)
                .map_or(0, |holders| holders.saturating_sub(1));
            if others == 0 {
                self.holders.remove(id);
                if let Some(committed) = self.committed.remove(id) {
                    *before = committed;
                }
            } else {
                self.holders.insert(id.clone(), others);
                if let Some(committed) = self.committed.get_mut(id) {
                    *before = std::mem::replace(committed, now(id));
                }
            }
        }
        if let Some((before, _)) = &mut step.project_file
            && let Some(committed) = self.committed_project_file.take()
        {
            *before = committed;
        }
    }
}
