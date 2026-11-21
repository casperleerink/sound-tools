//! Naming the versions of the history with the agent's small model, one at a time, once the
//! history has been still for a moment. Only while the agent is set up: without it a version
//! is "Version N".

use std::collections::HashSet;
use std::io;
use std::time::Duration;

use sound_core::{History, PointId, Project};

use super::{Chat, Done};
use crate::jobs::{self, Job};
use crate::naming::{self, Naming};

/// A program that takes longer is ended, so naming goes on.
const ASK_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct Namer {
    /// How long the history must be still, [`naming::QUIET`] but in a test.
    pub(super) quiet_for: Duration,
    /// The generation of the history the quiet wait started at.
    seen: Option<u64>,
    pub(super) quiet: Option<Job>,
    pub(super) asking: Option<Job>,
    /// Versions, by start and tip, the model gave no name for.
    failed: HashSet<(PointId, PointId)>,
}

impl Default for Namer {
    fn default() -> Self {
        Self {
            quiet_for: naming::QUIET,
            seen: None,
            quiet: None,
            asking: None,
            failed: HashSet::new(),
        }
    }
}

fn generation(project: &Project) -> Option<u64> {
    project.history().map(History::generation)
}

impl Chat {
    /// At every poll: the history changed when its generation did.
    pub(super) fn naming_heard(&mut self, project: &Project) {
        let generation = generation(project);
        if generation.is_some() && generation != self.namer.seen {
            self.wait_for_quiet(project);
        }
    }

    /// Names the next version once nothing changed for a while.
    pub(super) fn wait_for_quiet(&mut self, project: &Project) {
        let generation = generation(project);
        self.namer.seen = generation;
        let (outbox, quiet_for) = (self.inbox.outbox(), self.namer.quiet_for);
        self.namer.quiet = Some(jobs::spawn(async move {
            jobs::sleep(quiet_for).await;
            outbox.send(Done::Quiet(generation));
        }));
    }

    pub(super) fn quiet(&mut self, generation: Option<u64>, project: &Project) {
        // A wait of an older history, which a change replaced.
        if generation == self.namer.seen {
            self.namer.quiet = None;
            self.name_next(project);
        }
    }

    fn name_next(&mut self, project: &Project) {
        if self.namer.asking.is_some() {
            return;
        }
        let Some((provider, installed)) = self.ready_program() else {
            return;
        };
        let Some(history) = project.history() else {
            return;
        };
        let Naming {
            start,
            tip,
            description,
        } = match naming::next(history, &self.namer.failed) {
            Ok(Some(naming)) => naming,
            Ok(None) => return,
            Err(error) => {
                eprintln!("the versions to name are unreadable: {error}");
                return;
            }
        };
        let outbox = self.inbox.outbox();
        self.namer.asking = Some(jobs::spawn(async move {
            let asking = provider.ask_small_model(&installed, naming::INSTRUCTIONS, &description);
            let answer = jobs::within(ASK_TIMEOUT, asking).await;
            outbox.send(Done::Named { start, tip, answer });
        }));
    }

    pub(super) fn named(
        &mut self,
        start: PointId,
        tip: PointId,
        answer: Option<io::Result<String>>,
        project: &mut Project,
    ) {
        self.namer.asking = None;
        let answer = answer
            .unwrap_or_else(|| Err(io::Error::new(io::ErrorKind::TimedOut, "it took over 60 s")));
        let name = answer.and_then(|answer| {
            naming::name_in(&answer).ok_or_else(|| io::Error::other("its answer was empty"))
        });
        let stored = name.and_then(|name| {
            project
                .name_version(start, name, tip)
                .map_err(io::Error::other)
        });
        if let Err(error) = stored {
            eprintln!("a version was not named: {error}");
            self.namer.failed.insert((start, tip));
        }
        self.wait_for_quiet(project);
    }
}
