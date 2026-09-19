//! The live project folder: typed instances in an ownership tree, one editing path with undo,
//! storage that follows every finished edit, a watcher for outside changes, and the binding
//! that turns instance state into engine processors.
//!
//! The project lives on one thread, the control thread. Nothing here locks. The rules are in
//! ARCHITECTURE.md "Project storage" and "Editing and system services". `README.md` in this
//! crate is the guide for extension authors.

mod binding;
mod editing;
mod file;
mod instance;
mod outside;
mod registry;
mod storage;
mod watcher;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub use binding::{BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint};
pub use editing::{Changes, Edit};
pub use file::{FORMAT, PortReference, ProjectFile, SavedConnection, SavedDestination};
pub use instance::{Instance, InstanceId, InvalidInstanceId, State};
pub use registry::{Registry, RegistryError, ToolRegistration};
pub use storage::StorageError;
pub use watcher::GROUPING_WINDOW;

use binding::{BindError, Bindings, EngineChange, children_of};
use editing::{Applied, Change, History, RecordChange};
use instance::Record;
use storage::{Form, Locked, Storage};
use watcher::Watcher;

use crate::control::EngineControl;
use crate::graph::GraphError;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("the project {0} is already open in another runtime")]
    AlreadyOpen(String),
    #[error("the project is open read-only")]
    ReadOnly,
    /// `project.json` could not be loaded when the project opened.
    #[error("{path}: {message}")]
    InvalidProjectFile { path: String, message: String },
    #[error("instance {0} does not exist")]
    MissingInstance(InstanceId),
    #[error("instance {0} cannot be created: its parent does not exist")]
    MissingParent(InstanceId),
    #[error("instance {0} cannot be created: a record that is not loaded already has this id")]
    IdTaken(InstanceId),
    #[error("tool {0:?} is not registered, or its extension is not enabled in project.json")]
    UnknownTool(&'static str),
    #[error("instance {id}: {message}")]
    InvalidState { id: InstanceId, message: String },
    #[error("the behaviour of {instance} failed: {source}")]
    Behaviour {
        instance: InstanceId,
        source: BehaviourError,
    },
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("the file watcher failed: {0}")]
    Watcher(#[from] notify::Error),
}

impl From<BindError> for ProjectError {
    fn from(error: BindError) -> Self {
        match error {
            BindError::Behaviour { instance, source } => Self::Behaviour { instance, source },
            BindError::Graph(error) => Self::Graph(error),
        }
    }
}

/// What changed, for a UI layer. Drain it after every call that can change the project and
/// refresh only what is named. It carries no state: read that from the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectEvent {
    Created(InstanceId),
    /// The record changed. Owners are not named: a view of a parent that shows its children
    /// checks `is_inside`.
    Changed(InstanceId),
    Deleted(InstanceId),
    /// Tempo map, connections or extensions.
    ProjectFileChanged,
    ProblemsChanged,
}

/// Something in the project folder that is not live, and why. It stays listed until the file
/// loads or goes away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Relative to the project folder, for example `state/tone-a.json`.
    pub path: String,
    pub message: String,
}

/// Where a state application comes from. The differences are small on purpose.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// Opening the project: applying everything from empty.
    Load,
    Interface,
    /// The files already hold this state.
    Outside,
    /// Undo, redo and cancel.
    History,
}

pub struct Project {
    registry: Registry,
    storage: Storage,
    read_only: bool,
    engine: EngineControl,
    instances: BTreeMap<InstanceId, Record>,
    project_file: ProjectFile,
    bindings: Bindings,
    history: History,
    events: Vec<ProjectEvent>,
    /// By path relative to the project folder.
    file_problems: BTreeMap<String, String>,
    watcher: Option<Watcher>,
}

impl Project {
    /// Opens the project folder and takes its lock. A missing or empty folder becomes an
    /// empty project with every registered extension enabled. Loading applies all state from
    /// empty as one group, so the engine gets one batch.
    ///
    /// Records that do not load are listed in [`Self::problems`] and the project still opens.
    /// An invalid `project.json` fails the open, so that it is fixed before anything is written.
    pub fn open(
        folder: &Path,
        registry: Registry,
        engine: EngineControl,
    ) -> Result<Self, ProjectError> {
        let storage = match Storage::open_exclusive(folder) {
            Ok(Locked::Yes(storage)) => storage,
            Ok(Locked::No) => return Err(ProjectError::AlreadyOpen(folder.display().to_string())),
            Err(source) => return Err(io_error(folder, source)),
        };
        Self::load(storage, registry, engine, false)
    }

    /// Opens without the lock and never writes, so it works next to a running runtime. Every
    /// editing call fails with [`ProjectError::ReadOnly`]. For inspecting and offline rendering.
    pub fn open_read_only(
        folder: &Path,
        registry: Registry,
        engine: EngineControl,
    ) -> Result<Self, ProjectError> {
        let storage = Storage::open_read_only(folder).map_err(|source| io_error(folder, source))?;
        Self::load(storage, registry, engine, true)
    }

    fn load(
        storage: Storage,
        registry: Registry,
        engine: EngineControl,
        read_only: bool,
    ) -> Result<Self, ProjectError> {
        let extensions = registry.extensions().into_iter().map(String::from);
        let mut project = Self {
            project_file: ProjectFile::new(extensions.collect()),
            registry,
            storage,
            read_only,
            engine,
            instances: BTreeMap::new(),
            bindings: Bindings::default(),
            history: History::default(),
            events: Vec::new(),
            file_problems: BTreeMap::new(),
            watcher: None,
        };
        let mut changes = Vec::new();
        match project.storage.read_project_file()? {
            Some(bytes) => {
                let project_file = storage::decode_json(&bytes).map_err(|message| {
                    ProjectError::InvalidProjectFile {
                        path: storage::PROJECT_FILE.to_string(),
                        message,
                    }
                })?;
                let fingerprint = storage::fingerprint(&bytes);
                project.storage.observe_project_file(Some(fingerprint));
                changes.push(Change::ProjectFile(project_file));
            }
            None if read_only => {
                return Err(ProjectError::InvalidProjectFile {
                    path: storage::PROJECT_FILE.to_string(),
                    message: "the file does not exist".to_string(),
                });
            }
            None => project.storage.write_project_file(&project.project_file)?,
        }
        // The project file first: it says which extensions are enabled.
        project.apply(changes, Source::Load)?;
        let state_folder = project.storage.state_folder();
        project.apply_paths(&[state_folder], Source::Load)?;
        project.events.clear();
        Ok(project)
    }

    /// The project folder, canonical.
    pub fn root(&self) -> &Path {
        self.storage.root()
    }

    /// The engine, for the transport and for `poll`. Change the tempo map through an edit, so
    /// that it is saved and undoable.
    pub fn engine(&mut self) -> &mut EngineControl {
        &mut self.engine
    }

    pub fn project_file(&self) -> &ProjectFile {
        &self.project_file
    }

    /// Every live instance with its tool name, parents before children.
    pub fn instances(&self) -> impl Iterator<Item = (&InstanceId, &'static str)> {
        self.instances.iter().map(|(id, record)| (id, record.tool))
    }

    /// Resolves a saved reference. `None` when the instance does not exist or belongs to
    /// another tool. A reference owns nothing and keeps nothing alive.
    pub fn resolve<S: State>(&self, id: &InstanceId) -> Option<Instance<S>> {
        self.instances.get(id)?.state::<S>()?;
        Some(Instance::new(id.clone()))
    }

    /// The current state. `None` once the instance is deleted.
    pub fn state<S: State>(&self, instance: &Instance<S>) -> Option<&S> {
        self.instances.get(instance.id())?.state()
    }

    /// The owned children of `parent` that hold state of type `S`, in name order.
    pub fn children<S: State>(
        &self,
        parent: &InstanceId,
    ) -> impl Iterator<Item = (Instance<S>, &S)> {
        children_of(&self.instances, parent)
            .filter_map(|(id, record)| Some((Instance::new(id.clone()), record.state::<S>()?)))
    }

    /// The state of any instance as compact JSON, for summaries.
    pub fn state_json(&self, id: &InstanceId) -> Option<String> {
        self.instances.get(id)?.state.to_json().ok()
    }

    /// Takes the events since the last call.
    pub fn drain_events(&mut self) -> Vec<ProjectEvent> {
        std::mem::take(&mut self.events)
    }

    /// Files that are not live, and `project.json` connections that are not in the graph.
    pub fn problems(&self) -> Vec<Problem> {
        let files = self.file_problems.iter().map(|(path, message)| Problem {
            path: path.clone(),
            message: message.clone(),
        });
        let connections = self
            .bindings
            .connection_problems()
            .iter()
            .map(|message| Problem {
                path: storage::PROJECT_FILE.to_string(),
                message: message.clone(),
            });
        files.chain(connections).collect()
    }

    /// The one state application. Interface edits, file changes, loading, undo, redo and
    /// cancel all come through here. The group applies whole, as one engine batch, or not at
    /// all. It writes nothing: the caller knows whether files need writing.
    pub(crate) fn apply(
        &mut self,
        changes: Vec<Change>,
        source: Source,
    ) -> Result<Applied, ProjectError> {
        if self.read_only && source != Source::Load {
            return Err(ProjectError::ReadOnly);
        }
        let project_file_before = self.project_file.clone();
        let problems_before = self.bindings.connection_problems().to_vec();
        let mut records = Vec::new();
        let result = self
            .stage(changes, source, &mut records)
            .and_then(|()| self.bind(&records));
        if let Err(error) = result {
            for change in records.into_iter().rev() {
                match change.before {
                    Some(before) => self.instances.insert(change.id, before),
                    None => self.instances.remove(&change.id),
                };
            }
            self.project_file = project_file_before;
            return Err(error);
        }

        self.engine
            .set_tempo_map(self.project_file.tempo_map.clone());
        for change in &records {
            let id = change.id.clone();
            self.events.push(match (&change.before, &change.after) {
                (None, _) => ProjectEvent::Created(id),
                (_, None) => ProjectEvent::Deleted(id),
                _ => ProjectEvent::Changed(id),
            });
        }
        let project_file = (project_file_before != self.project_file)
            .then(|| (project_file_before, self.project_file.clone()));
        if project_file.is_some() {
            self.events.push(ProjectEvent::ProjectFileChanged);
        }
        if problems_before != self.bindings.connection_problems() {
            self.events.push(ProjectEvent::ProblemsChanged);
        }
        Ok(Applied {
            records,
            project_file,
        })
    }

    /// Changes `instances` and `project_file`, and notes every record change so that a
    /// failure can be rolled back.
    fn stage(
        &mut self,
        changes: Vec<Change>,
        source: Source,
        records: &mut Vec<RecordChange>,
    ) -> Result<(), ProjectError> {
        for change in changes {
            match change {
                Change::Set(id, record) => {
                    self.check_tool(record.tool)?;
                    if !self.instances.contains_key(&id) {
                        if let Some(parent) = id.parent()
                            && !self.instances.contains_key(&parent)
                        {
                            return Err(ProjectError::MissingParent(id));
                        }
                        // A record of an unknown tool or with an error may sit at this id.
                        if source == Source::Interface && self.storage.read_record(&id)?.is_some() {
                            return Err(ProjectError::IdTaken(id));
                        }
                    }
                    let before = self.instances.insert(id.clone(), record.clone());
                    let unchanged = before.as_ref().is_some_and(|before| before.equals(&record));
                    if !unchanged {
                        records.push(RecordChange {
                            id,
                            before,
                            after: Some(record),
                        });
                    }
                }
                Change::Delete(id) => {
                    let inside = id.inside(&self.instances);
                    let mut deleted: Vec<InstanceId> = inside.map(|(id, _)| id.clone()).collect();
                    deleted.insert(0, id.clone());
                    for id in deleted.into_iter().rev() {
                        if let Some(before) = self.instances.remove(&id) {
                            records.push(RecordChange {
                                id,
                                before: Some(before),
                                after: None,
                            });
                        }
                    }
                    self.project_file
                        .connections
                        .retain(|connection| !connection.touches(&id));
                }
                Change::ProjectFile(project_file) => self.project_file = project_file,
                Change::TempoMap(tempo_map) => self.project_file.tempo_map = tempo_map,
                Change::Connect(connection) => {
                    if !self.project_file.connections.contains(&connection) {
                        self.project_file.connections.push(connection);
                    }
                }
                Change::Disconnect(connection) => self
                    .project_file
                    .connections
                    .retain(|saved| *saved != connection),
            }
        }
        Ok(())
    }

    fn check_tool(&self, tool: &'static str) -> Result<(), ProjectError> {
        let enabled = self.registry.definition(tool).is_some_and(|definition| {
            let extensions = &self.project_file.extensions;
            extensions.iter().any(|it| it == definition.extension)
        });
        if enabled {
            Ok(())
        } else {
            Err(ProjectError::UnknownTool(tool))
        }
    }

    /// Runs the behaviours the change concerns: each changed instance and all its owners.
    fn bind(&mut self, records: &[RecordChange]) -> Result<(), ProjectError> {
        let mut dirty = BTreeSet::new();
        let mut unbound = Vec::new();
        for change in records {
            let tool_changed = match (&change.before, &change.after) {
                (Some(before), Some(after)) => before.tool != after.tool,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if tool_changed {
                unbound.push(change.id.clone());
            }
            dirty.extend(change.id.ancestors());
            dirty.insert(change.id.clone());
        }
        let change = EngineChange {
            registry: &self.registry,
            instances: &self.instances,
            unbound: &unbound,
            dirty,
            connections: &self.project_file.connections,
        };
        Ok(self.bindings.apply(&mut self.engine, change)?)
    }

    /// Makes the files of these instances match the live state: parents before children, then
    /// deleted records, then `project.json`. It goes on after a failure, reports every failure
    /// as a problem and returns the first.
    pub(crate) fn write<'a>(
        &mut self,
        ids: impl Iterator<Item = &'a InstanceId>,
        project_file: bool,
    ) -> Result<(), ProjectError> {
        let ids: BTreeSet<&InstanceId> = ids.collect();
        let mut results = Vec::new();
        for id in &ids {
            self.clear_record_problems(id);
            let Some(record) = self.instances.get(*id) else {
                continue;
            };
            // A parent that gets its first child moves into its folder before the child is
            // written, so the folder is the instance from the first moment.
            if let Some(parent) = id.parent()
                && let Some(parent_record) = self.instances.get(&parent)
                && self.storage.observed(&parent).map(|on_disk| on_disk.form) == Some(Form::File)
            {
                results.push(self.storage.write_record(&parent, parent_record, true));
            }
            let has_children = children_of(&self.instances, id).next().is_some();
            results.push(self.storage.write_record(id, record, has_children));
        }
        for id in ids.iter().rev() {
            if !self.instances.contains_key(*id) {
                results.push(self.storage.delete_record(id));
            }
        }
        if project_file {
            results.push(self.storage.write_project_file(&self.project_file));
        }

        let mut first = Ok(());
        for error in results.into_iter().filter_map(Result::err) {
            let (StorageError::Io { path, .. } | StorageError::Encode { path, .. }) = &error;
            self.report_problem(path.clone(), format!("not written: {error}"));
            if first.is_ok() {
                first = Err(ProjectError::Storage(error));
            }
        }
        first
    }
}

impl Project {
    /// Forgets what was reported about the record files of `id`.
    pub(crate) fn clear_record_problems(&mut self, id: &InstanceId) {
        for form in [Form::File, Form::Folder] {
            let path = self.storage.record_path(id, form);
            let path = self.storage.display_path(&path);
            if self.file_problems.remove(&path).is_some() {
                self.events.push(ProjectEvent::ProblemsChanged);
            }
        }
    }

    pub(crate) fn report_problem(&mut self, path: String, message: String) {
        self.file_problems.insert(path, message);
        self.events.push(ProjectEvent::ProblemsChanged);
    }
}

fn io_error(path: &Path, source: std::io::Error) -> ProjectError {
    ProjectError::Storage(StorageError::Io {
        path: path.display().to_string(),
        source,
    })
}
