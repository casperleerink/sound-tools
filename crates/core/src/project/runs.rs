//! Runs: the edits of code that edits as it goes, such as a tool of the project whose control
//! loop changes its record every frame. A run is an [`Edit`] that publishes each change at once
//! and finishes by itself, as a drag does at mouse up.

use std::time::{Duration, Instant};

use super::editing::{Changes, Edit};
use super::instance::InstanceId;
use super::{Project, ProjectError};

/// A run ends when its editor has been quiet this long: one undo step, and one write of its
/// files, for a burst of edits that comes 30 times a second.
pub const RUN_QUIET: Duration = Duration::from_secs(1);

/// A run that goes on ends this long after it began, so the files, which an agent reads, are
/// never further behind than this.
pub const RUN_LONGEST: Duration = Duration::from_secs(5);

/// The open run of one editor and label.
pub(super) struct Run {
    by: InstanceId,
    label: String,
    edit: Edit,
    began: Instant,
    last: Instant,
}

impl Project {
    /// Publishes `changes` as an edit of `by` named `label`, into its open run of that label: a
    /// run is one undo step, written once when it finishes. The poll finishes a run once it is
    /// quiet for [`RUN_QUIET`] or older than [`RUN_LONGEST`] (see
    /// [`Self::finish_quiet_runs_at`]); so do an undo and a redo, so undo takes back what the
    /// composer just saw happen, the first publish of another edit, and a run of another label
    /// of the same editor.
    pub fn publish_run(
        &mut self,
        by: &InstanceId,
        label: &str,
        changes: Changes,
    ) -> Result<(), ProjectError> {
        self.publish_run_at(by, label, changes, Instant::now())
    }

    /// [`Self::publish_run`] at a given time, for tests of the grouping.
    pub fn publish_run_at(
        &mut self,
        by: &InstanceId,
        label: &str,
        changes: Changes,
        at: Instant,
    ) -> Result<(), ProjectError> {
        let open = (self.runs.iter()).position(|run| run.by == *by && run.label == label);
        if open.is_none() {
            // Another label of the same editor ends its run first, so each is its own undo
            // step, in order, even when both change one record.
            self.finish_runs_where(|run| run.by == *by)?;
        }
        let mut run = match open {
            Some(index) => self.runs.swap_remove(index),
            None => Run {
                by: by.clone(),
                label: label.to_string(),
                edit: self.begin(label),
                began: at,
                last: at,
            },
        };
        run.last = at;
        let published = self.publish_into(&mut run.edit, changes);
        self.runs.push(run);
        published
    }

    /// Finishes every run that is quiet for [`RUN_QUIET`] at `at`, or older than
    /// [`RUN_LONGEST`]. The poll calls it.
    pub fn finish_quiet_runs_at(&mut self, at: Instant) -> Result<(), ProjectError> {
        self.finish_runs_where(|run| {
            at.saturating_duration_since(run.last) >= RUN_QUIET
                || at.saturating_duration_since(run.began) >= RUN_LONGEST
        })
    }

    /// Finishes every run that no other open edit holds a record of.
    pub(super) fn finish_free_runs(&mut self) -> Result<(), ProjectError> {
        self.finish_runs_where(|_| true)
    }

    /// Finishes every open run, in the order they began.
    pub(super) fn finish_all_runs(&mut self) -> Result<(), ProjectError> {
        let runs = std::mem::take(&mut self.runs);
        self.finish_runs(runs)
    }

    /// Finishes each run `done` picks that no other open edit, such as a drag, holds a record
    /// of: its step would hold the middle of that edit.
    fn finish_runs_where(&mut self, done: impl Fn(&Run) -> bool) -> Result<(), ProjectError> {
        let runs = std::mem::take(&mut self.runs);
        let held_by_others = |run: &Run| {
            run.edit.records().any(|id| {
                let by_runs = runs
                    .iter()
                    .filter(|run| run.edit.records().any(|held| held == id));
                self.history.holders(id) > by_runs.count()
            })
        };
        let ends: Vec<bool> = (runs.iter())
            .map(|run| done(run) && !held_by_others(run))
            .collect();
        let (finished, open): (Vec<_>, Vec<_>) =
            runs.into_iter().zip(ends).partition(|(_, end)| *end);
        self.runs = open.into_iter().map(|(run, _)| run).collect();
        self.finish_runs(finished.into_iter().map(|(run, _)| run).collect())
    }

    fn finish_runs(&mut self, mut runs: Vec<Run>) -> Result<(), ProjectError> {
        runs.sort_by_key(|run| run.began);
        // Every one, also after one fails to write.
        let mut result = Ok(());
        for run in runs {
            match run.edit.is_empty() {
                // A run whose first publish was refused: no step, which would end the grouping
                // of outside changes.
                true => self.abandon(run.edit),
                false => result = result.and(self.finish(run.edit)),
            }
        }
        result
    }
}
