//! Watches the project folder and groups what changed together.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

use super::{Project, ProjectError};

/// File changes with less than this between them apply together, as one undo step. The group
/// applies once the folder has been quiet for this long, so it is also the delay before an
/// outside change is heard.
pub const GROUPING_WINDOW: Duration = Duration::from_millis(100);

pub(crate) struct Watcher {
    /// Dropping it stops the watcher thread.
    _watcher: RecommendedWatcher,
    events: Receiver<notify::Result<notify::Event>>,
    pending: BTreeSet<PathBuf>,
    last_event: Instant,
}

impl Project {
    /// Starts watching `project.json` and `state/`. Events wait in a channel until `poll`.
    pub fn watch(&mut self) -> Result<(), ProjectError> {
        let (sender, events) = channel();
        let mut watcher = notify::recommended_watcher(sender)?;
        watcher.watch(self.storage.root(), RecursiveMode::NonRecursive)?;
        watcher.watch(&self.storage.state_folder(), RecursiveMode::Recursive)?;
        self.watcher = Some(Watcher {
            _watcher: watcher,
            events,
            pending: BTreeSet::new(),
            last_event: Instant::now(),
        });
        Ok(())
    }

    /// Call this regularly, like `EngineControl::poll`. It collects what the watcher saw and,
    /// once the folder has been quiet for [`GROUPING_WINDOW`], applies it as one group through
    /// [`Project::apply_outside_changes`]. Returns how many records and project files changed.
    ///
    /// It also brings the generated files up to date, `problems.txt` first of all, so an agent
    /// with only file access sees what the runtime made of its edit.
    pub fn poll(&mut self) -> Result<usize, ProjectError> {
        let changed = self.poll_watcher();
        let written = self.write_generated_files();
        changed.and_then(|changed| written.map(|()| changed))
    }

    fn poll_watcher(&mut self) -> Result<usize, ProjectError> {
        let Some(watcher) = &mut self.watcher else {
            return Ok(0);
        };
        loop {
            match watcher.events.try_recv() {
                Ok(Ok(event)) => {
                    // Reading a file changes nothing the project cares about.
                    if !event.kind.is_access() {
                        watcher.pending.extend(event.paths);
                        watcher.last_event = Instant::now();
                    }
                }
                Ok(Err(error)) => return Err(error.into()),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if watcher.pending.is_empty() || watcher.last_event.elapsed() < GROUPING_WINDOW {
            return Ok(0);
        }
        let paths: Vec<PathBuf> = std::mem::take(&mut watcher.pending).into_iter().collect();
        self.apply_outside_changes(&paths)
    }
}
