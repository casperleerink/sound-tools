//! Changes made to the project folder from outside: by an agent, an editor or git.
//!
//! The watcher only says which paths changed. This module reads those paths again, compares
//! them with the live project and applies the difference as one group. Event kinds are not
//! used, so creating, editing, deleting and moving files and folders all take the same road,
//! on every platform. Loading a project is the same road from an empty project.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::editing::{Change, Step};
use super::instance::InstanceId;
use super::storage::{self, OnDisk, PROJECT_FILE, PathTarget, STATE_FOLDER};
use super::{Project, ProjectError, ProjectEvent, ProjectFile, Source};

/// The undo label of a group of outside changes.
const OUTSIDE_LABEL: &str = "File change";

impl Project {
    /// Applies what changed at `paths` as one group and one undo step. This is what the
    /// watcher calls. Paths may be files or folders, inside the project folder, and may no
    /// longer exist. Returns how many records and project files changed.
    ///
    /// A file that does not load leaves the live state as it is, stays on disk, and is listed
    /// in [`Project::problems`]. The rest of the group still applies. The runtime's own writes
    /// change nothing here, because the files hold what the runtime last wrote.
    pub fn apply_outside_changes(&mut self, paths: &[PathBuf]) -> Result<usize, ProjectError> {
        self.apply_paths(paths, Source::Outside)
    }

    pub(crate) fn apply_paths(
        &mut self,
        paths: &[PathBuf],
        source: Source,
    ) -> Result<usize, ProjectError> {
        let mut queue = BTreeSet::new();
        let mut project_file_changed = false;
        for path in paths {
            match self.storage.target_of(path) {
                PathTarget::ProjectFile => project_file_changed = true,
                PathTarget::Record(id) => {
                    queue.insert(id);
                }
                PathTarget::Folder(id) => {
                    self.queue_inside(id.as_ref(), &mut queue);
                    queue.extend(id);
                }
                PathTarget::Ignored => {}
            }
        }

        let mut changes = Vec::new();
        let outside_project_file = if project_file_changed {
            self.read_outside_project_file()
        } else {
            None
        };
        if let Some((_, project_file)) = &outside_project_file {
            changes.push(Change::ProjectFile(project_file.clone()));
        }

        // Ids sort parents first, and a new instance only adds ids below itself.
        let mut observed = Vec::new();
        let mut created = BTreeSet::new();
        let mut deleted = BTreeSet::new();
        while let Some(id) = queue.pop_first() {
            self.clear_record_problems(&id);
            if id.ancestors().any(|ancestor| deleted.contains(&ancestor)) {
                continue;
            }
            let is_live = self.instances.contains_key(&id);
            let (form, bytes) = match self.storage.read_record(&id) {
                Ok(Some(found)) => found,
                Ok(None) => {
                    if is_live {
                        changes.push(Change::Delete(id.clone()));
                        deleted.insert(id);
                    }
                    continue;
                }
                Err(error) => {
                    self.report_storage_error(&error);
                    continue;
                }
            };
            let on_disk = OnDisk {
                form,
                fingerprint: storage::fingerprint(&bytes),
            };
            if is_live && self.storage.observed(&id) == Some(on_disk) {
                continue;
            }
            let path = self.storage.record_path(&id, form);
            let path = self.storage.display_path(&path);
            if let Some(parent) = id.parent()
                && !self.instances.contains_key(&parent)
                && !created.contains(&parent)
            {
                self.report_problem(
                    path,
                    format!("not loaded: its owner {parent} is not loaded"),
                );
                continue;
            }
            let project_file = match &outside_project_file {
                Some((_, project_file)) => project_file,
                None => &self.project_file,
            };
            match storage::decode_record(&bytes, &self.registry, project_file) {
                Ok(record) => {
                    changes.push(Change::Set(id.clone(), record));
                    observed.push((id.clone(), on_disk));
                    if !is_live {
                        // Its children may have been skipped before, while it did not load.
                        self.queue_inside(Some(&id), &mut queue);
                        created.insert(id);
                    }
                }
                Err(unloadable) => self.report_problem(path, unloadable.message()),
            }
        }

        let applied = match self.apply(changes, source) {
            Ok(applied) => applied,
            Err(error) => {
                // The files hold something the project does not. Look at them again next time.
                for (id, _) in &observed {
                    self.storage.forget(id);
                }
                for path in paths {
                    let path = self.storage.display_path(path);
                    self.report_problem(path, format!("not applied: {error}"));
                }
                return Err(error);
            }
        };
        for (id, on_disk) in observed {
            self.storage.observe(id, on_disk);
        }
        for change in &applied.records {
            if change.after.is_none() {
                self.storage.forget(&change.id);
            }
        }
        let mut write_project_file = false;
        if let Some((fingerprint, project_file)) = outside_project_file {
            self.storage.observe_project_file(Some(fingerprint));
            self.file_problems.remove(PROJECT_FILE);
            // A delete in the same group may have removed connections the file still lists.
            write_project_file = project_file != self.project_file;
        } else if applied.project_file.is_some() {
            write_project_file = true;
        }
        let count = applied.records.len() + usize::from(applied.project_file.is_some());
        if source == Source::Outside {
            let mut step = Step {
                label: OUTSIDE_LABEL.to_string(),
                ..Step::default()
            };
            step.absorb(applied);
            self.history.push(step);
            // The record files already hold the new state. Only `project.json` may not.
            self.write(std::iter::empty(), write_project_file)?;
        }
        Ok(count)
    }

    /// Queues everything inside an instance, live or on disk. `None` is the `state/` folder.
    fn queue_inside(&mut self, id: Option<&InstanceId>, queue: &mut BTreeSet<InstanceId>) {
        let prefix = match id {
            Some(id) => format!("{STATE_FOLDER}/{id}"),
            None => STATE_FOLDER.to_string(),
        };
        let before = self.file_problems.len();
        self.file_problems.retain(|path, _| {
            let inside = path.strip_prefix(&prefix);
            !inside.is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        });
        if self.file_problems.len() != before {
            self.events.push(ProjectEvent::ProblemsChanged);
        }

        let mut found = Vec::new();
        let mut problems = Vec::new();
        self.storage.scan(id, &mut found, &mut problems);
        for (path, message) in problems {
            self.report_problem(path, message);
        }
        queue.extend(found);
        match id {
            Some(id) => queue.extend(id.inside(&self.instances).map(|(id, _)| id.clone())),
            None => queue.extend(self.instances.keys().cloned()),
        }
    }

    /// `project.json` as it is on disk, when it holds something new that loads.
    fn read_outside_project_file(&mut self) -> Option<(u64, ProjectFile)> {
        let bytes = match self.storage.read_project_file() {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                let message = "the file is gone. The next edit of tempo or connections writes it";
                self.report_problem(PROJECT_FILE.to_string(), message.to_string());
                self.storage.observe_project_file(None);
                return None;
            }
            Err(error) => {
                self.report_storage_error(&error);
                return None;
            }
        };
        let fingerprint = storage::fingerprint(&bytes);
        if self.storage.project_file_fingerprint() == Some(fingerprint) {
            return None;
        }
        match storage::decode_json(&bytes) {
            Ok(project_file) => Some((fingerprint, project_file)),
            Err(message) => {
                self.report_problem(PROJECT_FILE.to_string(), message);
                None
            }
        }
    }

    fn report_storage_error(&mut self, error: &storage::StorageError) {
        let (storage::StorageError::Io { path, .. } | storage::StorageError::Encode { path, .. }) =
            error;
        self.report_problem(path.clone(), error.to_string());
    }
}
