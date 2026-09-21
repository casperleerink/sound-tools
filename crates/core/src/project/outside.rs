//! Changes made to the project folder from outside: by an agent, an editor or git.
//!
//! The watcher only says which paths changed. This module reads those paths again, compares
//! them with the live project and applies the difference as one group. Event kinds are not
//! used, so creating, editing, deleting and moving files and folders all take the same road,
//! on every platform. Loading a project is the same road from an empty project.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Instant;

use super::editing::{Applied, Change};
use super::instance::{FOLDER_RECORD, InstanceId};
use super::storage::{self, Form, OnDisk, PROJECT_FILE, PathTarget, RecordOnDisk, STATE_FOLDER};
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
    ///
    /// Groups that follow each other within [`OUTSIDE_UNDO_WINDOW`], with no interface edit,
    /// undo or redo in between, are one undo step.
    ///
    /// [`OUTSIDE_UNDO_WINDOW`]: super::OUTSIDE_UNDO_WINDOW
    pub fn apply_outside_changes(&mut self, paths: &[PathBuf]) -> Result<usize, ProjectError> {
        self.apply_outside_changes_at(paths, Instant::now())
    }

    /// [`Self::apply_outside_changes`] with the time of the change given, for tests of the
    /// undo grouping.
    pub fn apply_outside_changes_at(
        &mut self,
        paths: &[PathBuf],
        at: Instant,
    ) -> Result<usize, ProjectError> {
        self.apply_paths(paths, Source::Outside, at)
    }

    pub(crate) fn apply_paths(
        &mut self,
        paths: &[PathBuf],
        source: Source,
        at: Instant,
    ) -> Result<usize, ProjectError> {
        // Ids to look at, with the one form a folder scan saw for it, if any.
        let mut queue = BTreeMap::new();
        let mut project_file_changed = false;
        for path in paths {
            match self.storage.target_of(path) {
                PathTarget::ProjectFile => project_file_changed = true,
                PathTarget::Record(id) => {
                    queue.entry(id).or_insert(None);
                }
                PathTarget::Folder(id) => {
                    self.queue_inside(id.as_ref(), &mut queue);
                    if let Some(id) = id {
                        queue.entry(id).or_insert(None);
                    }
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
        // New instances of this group: their tool, and whether it owns children.
        let mut created: BTreeMap<InstanceId, (&'static str, bool)> = BTreeMap::new();
        let mut deleted = BTreeSet::new();
        while let Some((id, seen)) = queue.pop_first() {
            self.clear_record_problems(&id);
            if id.ancestors().any(|ancestor| deleted.contains(&ancestor)) {
                continue;
            }
            let is_live = self.instances.contains_key(&id);
            let (form, bytes) = match self.storage.read_record(&id, seen) {
                Ok(RecordOnDisk::One(form, bytes)) => (form, bytes),
                Ok(RecordOnDisk::Missing) => {
                    if is_live {
                        changes.push(Change::Delete(id.clone()));
                        deleted.insert(id.clone());
                    }
                    // Files of children that are left behind are not live. Say so.
                    if self.storage.has_folder(&id) {
                        let folder = self.storage.folder_display_path(&id);
                        let message = storage::folder_without_record(&folder);
                        self.report_problem(folder, message);
                    }
                    continue;
                }
                Ok(RecordOnDisk::Both) => {
                    let file = self.storage.record_path(&id, Form::File);
                    let folder = self.storage.record_path(&id, Form::Folder);
                    let message = format!(
                        "not loaded: {} also exists for the instance {id}. Keep one: {FOLDER_RECORD}.json in the folder when the tool owns children, else the file",
                        self.storage.display_path(&folder),
                    );
                    self.report_problem(self.storage.display_path(&file), message);
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
            let mut owner_tool = None;
            if let Some(parent) = id.parent() {
                let owner = match (created.get(&parent), self.instances.get(&parent)) {
                    (Some(created), _) => Some(*created),
                    (None, Some(record)) => Some((record.tool, record.owns_children)),
                    (None, None) => None,
                };
                owner_tool = owner.map(|(tool, _)| tool);
                let message = match owner.map(|(_, owns_children)| owns_children) {
                    Some(true) => None,
                    Some(false) => Some(format!(
                        "not loaded: its owner {parent} is of a tool that owns no children"
                    )),
                    None => Some(format!("not loaded: its owner {parent} is not loaded")),
                };
                if let Some(message) = message {
                    self.report_problem(path, message);
                    continue;
                }
            }
            let project_file = match &outside_project_file {
                Some((_, project_file)) => project_file,
                None => &self.project_file,
            };
            let record = match storage::decode_record(&bytes, &self.registry, project_file) {
                Ok(record) => record,
                Err(unloadable) => {
                    self.report_problem(path, unloadable.message());
                    continue;
                }
            };
            if Form::of(&record) != form {
                let right = self.storage.record_path(&id, Form::of(&record));
                let message = format!(
                    "not loaded: the tool {:?} {}, so this record belongs at {}",
                    record.tool,
                    if record.owns_children {
                        "owns children"
                    } else {
                        "owns no children"
                    },
                    self.storage.display_path(&right),
                );
                self.report_problem(path, message);
                continue;
            }
            if let Some(message) = record.place.refuses(record.tool, owner_tool) {
                self.report_problem(path, format!("not loaded: {message}"));
                continue;
            }
            if !is_live {
                created.insert(id.clone(), (record.tool, record.owns_children));
                if record.owns_children {
                    // Its children may have been skipped before, while it did not load.
                    self.queue_inside(Some(&id), &mut queue);
                }
            }
            changes.push(Change::Set(id.clone(), record));
            observed.push((id, on_disk));
        }

        let applied = match self.apply_leniently(changes, source) {
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
        let observed = observed
            .into_iter()
            .filter(|(id, _)| self.instances.contains_key(id));
        for (id, on_disk) in observed {
            self.storage.observe(id, on_disk);
        }
        for change in &applied.records {
            // A file that is left behind, under an owner that went away, stays known. So undo
            // of the owner's delete takes it back instead of seeing a foreign file at its id.
            if change.after.is_none()
                && matches!(
                    self.storage.read_record(&change.id, None),
                    Ok(RecordOnDisk::Missing)
                )
            {
                self.storage.forget(&change.id);
            }
        }
        let mut write_project_file = false;
        if let Some((fingerprint, project_file)) = outside_project_file {
            self.storage.observe_project_file(Some(fingerprint));
            self.clear_problem(PROJECT_FILE);
            // A delete in the same group may have removed connections the file still lists.
            write_project_file = project_file != self.project_file;
        } else if applied.project_file.is_some() {
            write_project_file = true;
        }
        let count = applied.records.len() + usize::from(applied.project_file.is_some());
        if source == Source::Outside {
            // The record files hold what was written from outside, but not what a derive made
            // of it, so those records are written here. Everything else is already on disk.
            let derived: Vec<InstanceId> = applied.derived.clone();
            self.history.push_outside(OUTSIDE_LABEL, applied, at);
            self.write(derived.iter(), write_project_file)?;
        }
        Ok(count)
    }

    /// Applies the group. While loading, an instance whose behaviour fails is left out with
    /// everything it owns and reported, so that one bad record does not keep the project shut.
    /// At any other time the group is rejected whole, as for every edit.
    fn apply_leniently(
        &mut self,
        mut changes: Vec<Change>,
        source: Source,
    ) -> Result<Applied, ProjectError> {
        if source != Source::Load {
            return self.apply(changes, source);
        }
        loop {
            let failed = match self.apply(changes.clone(), source) {
                Err(ProjectError::Behaviour { instance, source }) => (instance, source),
                result => return result,
            };
            let (instance, error) = failed;
            let record = changes.iter().find_map(|change| match change {
                Change::Set(id, record) if *id == instance => Some(record),
                _ => None,
            });
            // Not part of this group: leaving nothing out would fail the same way again.
            let Some(record) = record else {
                return Err(ProjectError::Behaviour {
                    instance,
                    source: error,
                });
            };
            let path = self.storage.record_path(&instance, Form::of(record));
            let path = self.storage.display_path(&path);
            changes.retain(|change| match change {
                Change::Set(id, _) => *id != instance && !id.is_inside(&instance),
                _ => true,
            });
            self.report_problem(path, format!("not loaded: its behaviour failed: {error}"));
        }
    }

    /// Queues everything inside an instance, live or on disk. `None` is the `state/` folder.
    fn queue_inside(
        &mut self,
        id: Option<&InstanceId>,
        queue: &mut BTreeMap<InstanceId, Option<Form>>,
    ) {
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
            self.push_event(ProjectEvent::ProblemsChanged);
        }

        let mut found = Vec::new();
        let mut problems = Vec::new();
        self.storage.scan(id, &mut found, &mut problems);
        for (path, message) in problems {
            self.report_problem(path, message);
        }
        // Live instances first, so that what the scan saw replaces their empty entry.
        let live: Vec<InstanceId> = match id {
            Some(id) => id
                .inside(&self.instances)
                .map(|(id, _)| id.clone())
                .collect(),
            None => self.instances.keys().cloned().collect(),
        };
        queue.extend(live.into_iter().map(|id| (id, None)));
        queue.extend(found);
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
        match storage::decode_json::<ProjectFile>(&bytes) {
            // Turning an extension on or off means loading or unloading every record of its
            // tools. Opening the project does that, and extensions change with a rebuild and
            // a restart anyway. So a running project refuses the change and says what to do.
            Ok(project_file) if project_file.extensions != self.project_file.extensions => {
                let message = "`extensions` changed. Reopen the project to apply it. Until then this file is not applied";
                self.report_problem(PROJECT_FILE.to_string(), message.to_string());
                None
            }
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
