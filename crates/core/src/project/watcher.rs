//! Watches the project folder and groups what changed together.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use notify::RecursiveMode;

use super::storage::PROJECT_FILE;
use super::{Project, ProjectError};

/// File changes with less than this between them apply together, as one undo step. The group
/// applies once the folder has been quiet for this long, so it is also the delay before an
/// outside change is heard.
pub const GROUPING_WINDOW: Duration = Duration::from_millis(100);

pub(crate) struct Watcher {
    /// Dropping it stops the watcher thread.
    watcher: Box<dyn notify::Watcher + Send>,
    /// What it watches: `project.json` in the project folder, `state/` and `assets/`.
    project_file: PathBuf,
    state: PathBuf,
    assets: PathBuf,
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
    /// Starts `watcher` on `project.json` in `root`, `state` and, once it exists, `assets`.
    /// Its events wait in `events` until [`Self::take_group`].
    fn new(
        mut watcher: Box<dyn notify::Watcher + Send>,
        events: Receiver<notify::Result<notify::Event>>,
        root: &Path,
        state: PathBuf,
        assets: PathBuf,
    ) -> notify::Result<Self> {
        watcher.watch(root, RecursiveMode::NonRecursive)?;
        watcher.watch(&state, RecursiveMode::Recursive)?;
        Ok(Self {
            watcher,
            project_file: root.join(PROJECT_FILE),
            state,
            assets,
            assets_watched: false,
            events,
            pending: BTreeSet::new(),
            last_event: Instant::now(),
        })
    }

    /// Collects what the system said changed. Once the folder has been quiet for
    /// [`GROUPING_WINDOW`], returns those paths as one group, with the time its last path was
    /// heard.
    fn take_group(&mut self) -> Result<Option<(Vec<PathBuf>, Instant)>, ProjectError> {
        // `assets/` from the moment it exists. What is in it by then counts as changed, since
        // no event said so.
        if !self.assets_watched && self.assets.is_dir() {
            // Tried once: a watch that fails is reported and not tried on every poll.
            self.assets_watched = true;
            self.watcher.watch(&self.assets, RecursiveMode::Recursive)?;
            self.hear([self.assets.clone()]);
        }
        let mut rescan = false;
        loop {
            match self.events.try_recv() {
                Ok(Ok(event)) if event.need_rescan() => rescan = true,
                Ok(Ok(event)) => {
                    // Reading a file changes nothing the project cares about.
                    if !event.kind.is_access() {
                        self.hear(event.paths);
                    }
                }
                Ok(Err(error)) => return Err(error.into()),
                Err(TryRecvError::Empty) => break,
                // notify's thread ended.
                Err(TryRecvError::Disconnected) => return Err(ProjectError::WatcherStopped),
            }
        }
        // The system dropped events, such as in the burst of a branch switch, so any file may
        // have changed. Reading them all again finds the ones that did.
        if rescan {
            let paths = [&self.project_file, &self.state, &self.assets];
            self.hear(paths.map(PathBuf::clone));
            watch_again(self.watcher.as_mut(), &self.state)?;
            if self.assets_watched {
                watch_again(self.watcher.as_mut(), &self.assets)?;
            }
        }
        if self.pending.is_empty() || self.last_event.elapsed() < GROUPING_WINDOW {
            return Ok(None);
        }
        let paths = std::mem::take(&mut self.pending).into_iter().collect();
        Ok(Some((paths, self.last_event)))
    }

    fn hear(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.pending.extend(paths);
        self.last_event = Instant::now();
    }
}

/// Watches a folder from scratch. inotify watches every folder on its own, so a folder made
/// while the system dropped events is not watched until then.
fn watch_again(watcher: &mut dyn notify::Watcher, folder: &Path) -> notify::Result<()> {
    watcher.unwatch(folder)?;
    watcher.watch(folder, RecursiveMode::Recursive)
}

impl Project {
    /// Starts watching `project.json`, `state/` and `assets/`. Events wait in a channel until
    /// `poll`.
    pub fn watch(&mut self) -> Result<(), ProjectError> {
        let (sender, events) = channel();
        self.watch_with(Box::new(notify::recommended_watcher(sender)?), events)
    }

    /// [`Self::watch`] with `watcher`, whose events come on `events`.
    fn watch_with(
        &mut self,
        watcher: Box<dyn notify::Watcher + Send>,
        events: Receiver<notify::Result<notify::Event>>,
    ) -> Result<(), ProjectError> {
        let state = self.storage.state_folder();
        let assets = self.assets.path_of_folder();
        let root = self.storage.root();
        self.watcher = Some(Watcher::new(watcher, events, root, state, assets)?);
        Ok(())
    }

    /// Call this regularly, like `EngineControl::poll`. It collects what the watcher saw and,
    /// once the folder has been quiet for [`GROUPING_WINDOW`], applies it as one group through
    /// [`Project::apply_outside_changes_at`], at the time its last path was heard. Returns how
    /// many records and project files changed.
    ///
    /// It also finishes the runs that went quiet (see [`Project::publish_run`]) and brings the
    /// generated files up to date, `problems.txt` first of all, so an agent with only file
    /// access sees what the runtime made of its edit.
    pub fn poll(&mut self) -> Result<usize, ProjectError> {
        let changed = self.poll_watcher();
        let finished = self.finish_quiet_runs_at(std::time::Instant::now());
        let written = self.write_generated_files();
        changed.and_then(|changed| finished.and(written).map(|()| changed))
    }

    fn poll_watcher(&mut self) -> Result<usize, ProjectError> {
        let Some(watcher) = &mut self.watcher else {
            return Ok(0);
        };
        match watcher.take_group() {
            Ok(Some((paths, heard))) => self.apply_outside_changes_at(&paths, heard),
            Ok(None) => Ok(0),
            Err(error) => {
                // Taken out so it is reported once, not on every poll. Forgotten, not dropped:
                // notify's inotify watcher unwraps a send to its ended thread when dropped,
                // and its FSEvents watcher joins that thread with `expect`.
                if matches!(error, ProjectError::WatcherStopped) {
                    std::mem::forget(self.watcher.take());
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::sync::mpsc::channel;

    use notify::NullWatcher;
    use notify::event::{Event, EventKind, Flag};
    use serde::{Deserialize, Serialize};

    use super::GROUPING_WINDOW;
    use crate::{Engine, EngineConfig, Project, ProjectError, Registry, State};

    #[derive(Clone, PartialEq, Serialize, Deserialize)]
    struct Note {
        pitch: u8,
    }

    impl State for Note {
        const TOOL: &'static str = "test.note";
    }

    #[test]
    fn files_whose_events_the_system_dropped_apply_on_its_rescan() {
        let folder = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        registry.tool::<Note>("test").unwrap();
        let (control, _engine) = Engine::new(EngineConfig::new(48_000, 1));
        let mut project = Project::open(folder.path(), registry, control).unwrap();
        // The test speaks for the system, which drops the event of the write.
        let (system, events) = channel();
        project.watch_with(Box::new(NullWatcher), events).unwrap();

        let record = r#"{"tool": "test.note", "state": {"pitch": 60}}"#;
        std::fs::write(project.root().join("state/note.json"), record).unwrap();
        let rescan = Event::new(EventKind::Other).set_flag(Flag::Rescan);
        system.send(Ok(rescan)).unwrap();
        let heard = project.poll().unwrap();
        std::thread::sleep(GROUPING_WINDOW);
        assert_eq!(heard + project.poll().unwrap(), 1);
    }

    #[test]
    fn a_watcher_that_stops_is_reported_once() {
        let folder = tempfile::tempdir().unwrap();
        let (control, _engine) = Engine::new(EngineConfig::new(48_000, 1));
        let mut project = Project::open(folder.path(), Registry::new(), control).unwrap();
        // The system's end of the channel is gone, as when notify's thread ends.
        project
            .watch_with(Box::new(NullWatcher), channel().1)
            .unwrap();

        assert!(matches!(project.poll(), Err(ProjectError::WatcherStopped)));
        assert_eq!(project.poll().unwrap(), 0);
    }
}
