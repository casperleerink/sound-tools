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

/// The open run of one editor.
pub(super) struct Run {
    by: InstanceId,
    label: String,
    edit: Edit,
    began: Instant,
    last: Instant,
}

impl Project {
    /// Publishes `changes` as an edit of `by` named `label`, into its open run when that has the
    /// same label, is quiet for less than [`RUN_QUIET`] and began less than [`RUN_LONGEST`]
    /// ago; else the open run finishes and a new one begins. A run is one undo step, written
    /// once when it finishes: on its own, see [`Self::finish_quiet_runs_at`], or before an
    /// undo or a redo, so undo takes back what the composer just saw happen.
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
        let open = self.runs.iter().position(|run| run.by == *by);
        let goes_on = |run: &Run| {
            run.label == label
                && at.saturating_duration_since(run.last) < RUN_QUIET
                && at.saturating_duration_since(run.began) < RUN_LONGEST
        };
        let mut finished = Ok(());
        let mut run = match open.map(|index| self.runs.swap_remove(index)) {
            Some(run) if goes_on(&run) => run,
            ended => {
                if let Some(ended) = ended {
                    finished = self.finish(ended.edit);
                }
                Run {
                    by: by.clone(),
                    label: label.to_string(),
                    edit: self.begin(label),
                    began: at,
                    last: at,
                }
            }
        };
        run.last = at;
        let published = self.publish(&mut run.edit, changes);
        self.runs.push(run);
        published.and(finished)
    }

    /// Finishes every run that has been quiet for [`RUN_QUIET`] at `at`. The poll calls it.
    pub fn finish_quiet_runs_at(&mut self, at: Instant) -> Result<(), ProjectError> {
        let (quiet, open) = std::mem::take(&mut self.runs)
            .into_iter()
            .partition(|run| at.saturating_duration_since(run.last) >= RUN_QUIET);
        self.runs = open;
        self.finish_runs(quiet)
    }

    /// Finishes every open run, in the order they began.
    pub(super) fn finish_all_runs(&mut self) -> Result<(), ProjectError> {
        let runs = std::mem::take(&mut self.runs);
        self.finish_runs(runs)
    }

    fn finish_runs(&mut self, mut runs: Vec<Run>) -> Result<(), ProjectError> {
        runs.sort_by_key(|run| run.began);
        // Every one, also after one fails to write.
        let mut result = Ok(());
        for run in runs {
            result = result.and(self.finish(run.edit));
        }
        result
    }
}
