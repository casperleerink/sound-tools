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
    watcher: RecommendedWatcher,
    /// Whether `assets/` is watched. It is watched from when it exists, which may be after the
    /// project opened: a tool may wait for a file there, see
    /// [`ToolRegistration::rebinds_on_assets`](super::ToolRegistration::rebinds_on_assets).
    assets_watched: bool,
    events: Receiver<notify::Result<notify::Event>>,
    pending: BTreeSet<PathBuf>,
    /// When the last path of `pending` was heard. The group counts from then for undo, not
    /// from when it applies a grouping window later, so a write heard just after a request
    /// ended joins it.
    last_event: Instant,
}

impl Watcher {
    fn hear(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.pending.extend(paths);
        self.last_event = Instant::now();
    }
}

impl Project {
    /// Starts watching `project.json`, `state/` and `assets/`. Events wait in a channel until
    /// `poll`.
    pub fn watch(&mut self) -> Result<(), ProjectError> {
        let (sender, events) = channel();
        let mut watcher = notify::recommended_watcher(sender)?;
        watcher.watch(self.storage.root(), RecursiveMode::NonRecursive)?;
        watcher.watch(&self.storage.state_folder(), RecursiveMode::Recursive)?;
        self.watcher = Some(Watcher {
            watcher,
            assets_watched: false,
            events,
            pending: BTreeSet::new(),
            last_event: Instant::now(),
        });
        Ok(())
    }

    /// Call this regularly, like `EngineControl::poll`. It collects what the watcher saw and,
    /// once the folder has been quiet for [`GROUPING_WINDOW`], applies it as one group through
    /// [`Project::apply_outside_changes_at`], at the time its last path was heard. Returns how
    /// many records and project files changed.
    ///
    /// It also brings the generated files up to date, `problems.txt` first of all, so an agent
    /// with only file access sees what the runtime made of its edit.
    pub fn poll(&mut self) -> Result<usize, ProjectError> {
        let changed = self.poll_watcher();
        let written = self.write_generated_files();
        changed.and_then(|changed| written.map(|()| changed))
    }

    fn poll_watcher(&mut self) -> Result<usize, ProjectError> {
        let assets = self.assets().path_of_folder();
        let Some(watcher) = &mut self.watcher else {
            return Ok(0);
        };
        // `assets/` from the moment it exists. What is in it by then counts as changed, since
        // no event said so.
        if !watcher.assets_watched && assets.is_dir() {
            // Tried once: a watch that fails is reported and not tried on every poll.
            watcher.assets_watched = true;
            watcher.watcher.watch(&assets, RecursiveMode::Recursive)?;
            watcher.hear([assets]);
        }
        loop {
            match watcher.events.try_recv() {
                Ok(Ok(event)) => {
                    // Reading a file changes nothing the project cares about.
                    if !event.kind.is_access() {
                        watcher.hear(event.paths);
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
        let heard = watcher.last_event;
        self.apply_outside_changes_at(&paths, heard)
    }
}
