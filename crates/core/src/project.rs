//! The live project folder: typed instances in an ownership tree, one editing path with undo,
//! storage that follows every finished edit, a watcher for outside changes, and the binding
//! that turns instance state into engine processors.
//!
//! The project lives on one thread, the control thread. Nothing here locks. The rules are in
//! ARCHITECTURE.md "Project storage" and "Editing and system services". `README.md` in this
//! crate is the guide for extension authors.

mod assets;
mod binding;
mod editing;
mod file;
mod generated;
mod instance;
mod outside;
mod registry;
mod storage;
mod watcher;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub use assets::{ASSETS_FOLDER, AssetError, AssetName, Assets, InvalidAssetName};
pub use binding::{BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint};
pub use editing::{Changes, Derived, Edit, OUTSIDE_UNDO_WINDOW};
pub use file::{FORMAT, PortReference, ProjectFile, SavedConnection, SavedDestination};
pub use generated::{AGENT_DOC_FILE, AGENT_DOCS_FOLDER, NO_PROBLEMS, PROBLEMS_FILE};
pub use instance::{Instance, InstanceId, InvalidInstanceId, Place, State};
pub use registry::{AgentDoc, Registry, RegistryError, ToolRegistration, Was};
pub use storage::StorageError;
pub use watcher::GROUPING_WINDOW;

use binding::{BindError, Bindings, EngineChange};
use editing::{Applied, Change, History, RecordChange};
use instance::Record;
use registry::DerivedFrom;
use storage::{Form, Locked, RecordOnDisk, Storage};
use watcher::Watcher;

use crate::clock::{Clock, Ticks, TimeSignature};
use crate::control::EngineControl;
use crate::graph::GraphError;
use crate::processor::Processor;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("the project {0} is already open in another runtime")]
    AlreadyOpen(String),
    #[error("the project is open read-only")]
    ReadOnly,
    /// `project.json` could not be loaded when the project opened.
    #[error("{path}: {message}")]
    InvalidProjectFile { path: String, message: String },
    #[error(transparent)]
    InvalidId(#[from] InvalidInstanceId),
    #[error("instance {0} does not exist")]
    MissingInstance(InstanceId),
    #[error("instance {instance} has no processor {name:?} of this type")]
    MissingProcessor { instance: InstanceId, name: String },
    #[error("instance {0} cannot be created: its parent does not exist")]
    MissingParent(InstanceId),
    #[error(
        "instance {id} cannot be created: its parent is a {parent_tool:?}, which owns no children"
    )]
    ParentOwnsNoChildren {
        id: InstanceId,
        parent_tool: &'static str,
    },
    #[error("instance {id} cannot be created here: {message}")]
    WrongPlace { id: InstanceId, message: String },
    #[error("instance {0} cannot be created: a file that is not loaded already has this id")]
    IdTaken(InstanceId),
    #[error("tool {0:?} is not registered, or its extension is not enabled in project.json")]
    UnknownTool(&'static str),
    #[error("instance {id}: {message}")]
    InvalidState { id: InstanceId, message: String },
    #[error(
        "{path} holds a change that did not load, and {id} decides what is in it, so this cannot be applied: a record and what it derives are saved together or not at all. Fix {path} first"
    )]
    DerivesIntoAStaleProjectFile { id: InstanceId, path: String },
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
    assets: Assets,
    read_only: bool,
    engine: EngineControl,
    instances: BTreeMap<InstanceId, Record>,
    project_file: ProjectFile,
    bindings: Bindings,
    history: History,
    events: Vec<ProjectEvent>,
    /// By path relative to the project folder.
    file_problems: BTreeMap<String, String>,
    /// What the derive of an instance could not compute, by instance. It stays until that
    /// derive runs again, like what a behaviour reports about its own instance.
    derive_problems: BTreeMap<InstanceId, Vec<String>>,
    /// The generated files may no longer match the project. See `generated.rs`.
    generated_are_stale: bool,
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
            assets: Assets::new(storage.root()),
            registry,
            storage,
            read_only,
            engine,
            instances: BTreeMap::new(),
            bindings: Bindings::default(),
            history: History::default(),
            events: Vec::new(),
            file_problems: BTreeMap::new(),
            derive_problems: BTreeMap::new(),
            generated_are_stale: true,
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
        project.apply_paths(&[state_folder], Source::Load, std::time::Instant::now())?;
        project.events.clear();
        project.write_generated_files()?;
        Ok(project)
    }

    /// The project folder, canonical.
    pub fn root(&self) -> &Path {
        self.storage.root()
    }

    /// The `assets/` folder: opaque files an extension owns, such as a raw take or the state
    /// of a hosted plugin. The core never reads inside one. See [`Assets`].
    pub fn assets(&self) -> &Assets {
        &self.assets
    }

    /// The engine, for the transport and for `poll`. Change the tempo map through an edit, so
    /// that it is saved and undoable.
    pub fn engine(&mut self) -> &mut EngineControl {
        &mut self.engine
    }

    /// Sends one update to the processor that the behaviour of `instance` declared under
    /// `name`: something from an interface that should happen now, such as a preview note. It
    /// is not an edit: nothing is saved and there is no undo step. It applies at the start of
    /// the next block.
    pub fn send<P: Processor>(
        &mut self,
        instance: &InstanceId,
        name: &str,
        update: P::Update,
    ) -> Result<(), ProjectError> {
        let node = self.bindings.node::<P>(instance, name);
        let node = node.ok_or_else(|| ProjectError::MissingProcessor {
            instance: instance.clone(),
            name: name.to_string(),
        })?;
        Ok(self.engine.update(node, update)?)
    }

    /// The input port that the behaviour of `instance` named, for code below the tools that
    /// plays into an instance from outside the project: MIDI input into the `notes` port of an
    /// instrument, wired by the window. `None` while the instance has no such port.
    ///
    /// The endpoint is a place in the engine graph and changes when the processor behind it is
    /// built again. Read it again after every change instead of keeping it.
    pub fn input_port(&self, instance: &InstanceId, port: &str) -> Option<InputEndpoint> {
        self.bindings.input(instance, port)
    }

    /// The clock the engine plays by, for conversions while reading, such as ticks to seconds.
    pub fn clock(&self) -> &Clock {
        self.engine.clock()
    }

    pub fn project_file(&self) -> &ProjectFile {
        &self.project_file
    }

    /// Every live instance with its tool name, parents before children.
    pub fn instances(&self) -> impl Iterator<Item = (&InstanceId, &'static str)> {
        self.instances.iter().map(|(id, record)| (id, record.tool))
    }

    /// The tool name of an instance.
    pub fn tool_of(&self, id: &InstanceId) -> Option<&'static str> {
        self.instances.get(id).map(|record| record.tool)
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
        parent
            .children_in(&self.instances)
            .filter_map(|(id, record)| Some((Instance::new(id.clone()), record.state::<S>()?)))
    }

    /// How the tool of `id` describes the instance and what it owns, as lines of text. `None`
    /// when the instance does not exist or its tool registered no summary.
    pub fn summary(&self, id: &InstanceId) -> Option<String> {
        let record = self.instances.get(id)?;
        let summary = self.registry.definition(record.tool)?.summary.as_ref()?;
        Some(summary(self, id))
    }

    /// Where the project ends: the latest end that a tool gives for one of its instances, see
    /// [`ToolRegistration::end`]. `None` for a project that runs without a set end.
    pub fn end(&self) -> Option<Ticks> {
        let ends = self.instances.iter().filter_map(|(id, record)| {
            let end = self.registry.definition(record.tool)?.end.as_ref()?;
            end(self, id)
        });
        ends.max()
    }

    /// An id that is free: `wanted`, or else `wanted-2`, `wanted-3` and so on. Free means no
    /// live instance has it and no file or folder sits at its place, loaded or not.
    pub fn free_id(&self, wanted: &InstanceId) -> Result<InstanceId, ProjectError> {
        let mut candidate = wanted.clone();
        let mut number = 1;
        loop {
            let on_disk = self.storage.read_record(&candidate, None)?;
            let taken = self.instances.contains_key(&candidate)
                || !matches!(on_disk, RecordOnDisk::Missing)
                || self.storage.has_folder(&candidate);
            if !taken {
                return Ok(candidate);
            }
            number += 1;
            candidate = InstanceId::numbered(wanted, number);
        }
    }

    /// The state of any instance as compact JSON, for summaries.
    pub fn state_json(&self, id: &InstanceId) -> Option<String> {
        self.instances.get(id)?.state.to_json().ok()
    }

    /// Runs the behaviour of one instance again, with the record it already has.
    ///
    /// It is not an edit: nothing is written, nothing is undoable, and the record is untouched.
    /// It is for a service outside the project that can do more later than it could before, so
    /// far only the plugin host: a scan that was still running when a plugin record was applied
    /// has found the plugin, and the behaviour now hands the engine that plugin and stops
    /// reporting it. A behaviour that fails here leaves the instance as it was, as any failed
    /// group does.
    ///
    /// `Ok(false)` says there is no such instance, which is what a record that went away while
    /// something waited for it looks like.
    pub fn rebind(&mut self, id: &InstanceId) -> Result<bool, ProjectError> {
        let Some(record) = self.instances.get(id).cloned() else {
            return Ok(false);
        };
        // Not the one state application: that one drops a change whose record is the one that
        // is already there, which is exactly this. The behaviour is run directly instead, as
        // it would be for a record that really changed, owners included.
        let instance_problems_before = self.instance_problems();
        let connection_problems_before = self.bindings.connection_problems().to_vec();
        self.bind(&[RecordChange {
            id: id.clone(),
            before: Some(record.clone()),
            after: Some(record),
        }])?;
        self.push_event(ProjectEvent::Changed(id.clone()));
        // Both sets, as the one state application does: a behaviour that now declares the port
        // a saved connection names takes that connection's problem away too.
        if instance_problems_before != self.instance_problems()
            || connection_problems_before != self.bindings.connection_problems()
        {
            self.push_event(ProjectEvent::ProblemsChanged);
        }
        Ok(true)
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
        // What a behaviour or a derive said about its own instance while it ran. The record is
        // live and untouched; part of what it asks for is not.
        files
            .chain(connections)
            .chain(self.instance_problems())
            .collect()
    }

    /// What every behaviour and every derive reported about its own instance, on the path of
    /// its record.
    fn instance_problems(&self) -> Vec<Problem> {
        let derived = self
            .derive_problems
            .iter()
            .flat_map(|(id, messages)| messages.iter().map(move |message| (id, message)));
        let problems = self.bindings.instance_problems().chain(derived);
        problems
            .filter_map(|(id, message)| {
                let record = self.instances.get(id)?;
                let path = self.storage.record_path(id, Form::of(record));
                Some(Problem {
                    path: self.storage.display_path(&path),
                    message: message.clone(),
                })
            })
            .collect()
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
        // A group that brings `project.json` itself is the one that makes it load again, so a
        // derive in it is not writing into a file the runtime has lost track of.
        let project_file_arrives = changes
            .iter()
            .any(|change| matches!(change, Change::ProjectFile(_)));
        let project_file_before = self.project_file.clone();
        let problems_before = self.bindings.connection_problems().to_vec();
        // What behaviours said last time, so that `problems.txt` and the views follow a
        // behaviour that starts or stops reporting. Empty in a project with nothing to report.
        let instance_problems_before = self.instance_problems();
        let time_signature_before = project_file_before.tempo_map.time_signature();
        let mut records = Vec::new();
        let mut derived = Vec::new();
        let result = self
            .stage(changes, source, &mut records)
            .and_then(|()| {
                self.stage_derived(
                    source,
                    time_signature_before,
                    project_file_arrives,
                    &mut records,
                    &mut derived,
                )
            })
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
        for change in &mut records {
            self.history.note_committed(change, source);
        }
        let mut project_file = (project_file_before != self.project_file)
            .then(|| (project_file_before, self.project_file.clone()));
        self.history
            .note_committed_project_file(&mut project_file, source);
        if project_file.is_some() {
            self.push_event(ProjectEvent::ProjectFileChanged);
        }
        if problems_before != self.bindings.connection_problems()
            || instance_problems_before != self.instance_problems()
        {
            self.push_event(ProjectEvent::ProblemsChanged);
        }
        Ok(Applied {
            records,
            project_file,
            derived,
        })
    }

    /// Runs the derives this group asks for and stages what they give, as part of the same
    /// group. See [`ToolRegistration::derive`].
    ///
    /// A derive runs when a record of its tool changed in this group, or when the project's
    /// time signature changed. What a derive gives is staged and nothing more: it starts no
    /// second round, so this cannot loop. It does not run while the project loads, nor for
    /// undo, redo or a cancel, because the files and the undo step already hold what it would
    /// compute. So a read-only project never derives and never writes.
    fn stage_derived(
        &mut self,
        source: Source,
        time_signature_before: TimeSignature,
        project_file_arrives: bool,
        records: &mut Vec<RecordChange>,
        derived: &mut Vec<InstanceId>,
    ) -> Result<(), ProjectError> {
        if !matches!(source, Source::Interface | Source::Outside) {
            return Ok(());
        }
        let derives = |project: &Self, id: &InstanceId| {
            let tool = project.instances.get(id)?.tool;
            project.registry.definition(tool)?.derive.as_ref()?;
            Some(())
        };
        let signature_changed =
            self.project_file.tempo_map.time_signature() != time_signature_before;
        let mut ids: BTreeSet<InstanceId> = BTreeSet::new();
        if signature_changed {
            let live = self.instances.keys();
            ids.extend(live.filter(|id| derives(self, id).is_some()).cloned());
        }
        let changed = records.iter().map(|change| &change.id);
        ids.extend(changed.filter(|id| derives(self, id).is_some()).cloned());
        let Some(first) = ids.first().cloned() else {
            return Ok(());
        };
        // A derive writes its record and the tempo map together, and `project.json` is not
        // written while it holds an outside change that did not load. Applying the record
        // alone would leave a project that plays one thing and opens as another, because a
        // derive does not run on load. So the whole group waits for that file.
        if !project_file_arrives && !self.project_file_on_disk_is_known() {
            return Err(ProjectError::DerivesIntoAStaleProjectFile {
                id: first,
                path: storage::PROJECT_FILE.to_string(),
            });
        }

        let mut changes = Vec::new();
        let mut reported = Vec::new();
        for id in ids {
            // Both borrows are shared: the definition and the project the derive reads.
            let Some(tool) = self.instances.get(&id).map(|record| record.tool) else {
                continue;
            };
            let derive = self
                .registry
                .definition(tool)
                .and_then(|it| it.derive.as_ref());
            let Some(derive) = derive else { continue };
            // What the record was, so a derive can write only what really moved.
            let from = match records.iter().find(|change| change.id == id) {
                None => DerivedFrom::Unchanged,
                Some(change) => match &change.before {
                    None => DerivedFrom::Created,
                    Some(record) => DerivedFrom::Changed(record),
                },
            };
            let mut result = Derived::default();
            derive(self, &id, from, &mut result);
            changes.extend(result.changes.changes);
            reported.push((id, result.problems));
        }
        for (id, problems) in reported {
            match problems.is_empty() {
                true => self.derive_problems.remove(&id),
                false => self.derive_problems.insert(id, problems),
            };
        }
        // What a derive said about an instance that is gone, or that is now a record of
        // another tool, would otherwise be shown on that record's path for ever.
        let stale: Vec<InstanceId> = self
            .derive_problems
            .keys()
            .filter(|id| derives(self, id).is_none())
            .cloned()
            .collect();
        for id in stale {
            self.derive_problems.remove(&id);
        }
        for change in &changes {
            if let Change::Set(id, record) = change {
                record
                    .state
                    .validate()
                    .map_err(|message| ProjectError::InvalidState {
                        id: id.clone(),
                        message,
                    })?;
            }
        }
        let before = records.len();
        self.stage(changes, source, records)?;
        derived.extend(records[before..].iter().map(|change| change.id.clone()));
        Ok(())
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
                    let owner = id.parent().and_then(|parent| self.instances.get(&parent));
                    let owner = owner.map(|owner| owner.tool);
                    if let Some(message) = record.place.refuses(record.tool, &id, owner)
                        && id.parent().is_none_or(|_| owner.is_some())
                    {
                        return Err(ProjectError::WrongPlace { id, message });
                    }
                    if !self.instances.contains_key(&id) {
                        if let Some(parent) = id.parent() {
                            let Some(parent) = self.instances.get(&parent) else {
                                return Err(ProjectError::MissingParent(id));
                            };
                            if !parent.owns_children {
                                return Err(ProjectError::ParentOwnsNoChildren {
                                    id,
                                    parent_tool: parent.tool,
                                });
                            }
                        }
                        // A file the runtime did not load may sit at this id: a record of an
                        // unknown tool, or one with an error. Undo must not write over it
                        // either. A file the runtime knows is its own, from before a delete
                        // that is not written yet.
                        let writes = matches!(source, Source::Interface | Source::History);
                        if writes
                            && self.storage.observed(&id).is_none()
                            && !matches!(
                                self.storage.read_record(&id, None)?,
                                RecordOnDisk::Missing
                            )
                        {
                            return Err(ProjectError::IdTaken(id));
                        }
                    }
                    // The id now belongs to a tool that owns nothing: what it owned goes.
                    if !record.owns_children {
                        self.stage_delete_inside(&id, records);
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
                    self.stage_delete_inside(&id, records);
                    if let Some(before) = self.instances.remove(&id) {
                        records.push(RecordChange {
                            id: id.clone(),
                            before: Some(before),
                            after: None,
                        });
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

    /// Removes everything `id` owns, children before parents.
    fn stage_delete_inside(&mut self, id: &InstanceId, records: &mut Vec<RecordChange>) {
        let inside = id.inside(&self.instances);
        let inside: Vec<InstanceId> = inside.map(|(id, _)| id.clone()).collect();
        for id in inside.into_iter().rev() {
            if let Some(before) = self.instances.remove(&id) {
                records.push(RecordChange {
                    id,
                    before: Some(before),
                    after: None,
                });
            }
        }
    }

    /// Whether the records of this tool decide state of their own, see
    /// [`ToolRegistration::derive`].
    pub(crate) fn derives(&self, tool: &'static str) -> bool {
        let definition = self.registry.definition(tool);
        definition.is_some_and(|definition| definition.derive.is_some())
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
        Ok(self
            .bindings
            .apply(&mut self.engine, &self.assets, change)?)
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
            results.push(self.storage.write_record(id, record));
        }
        for id in ids.iter().rev() {
            if !self.instances.contains_key(*id) {
                results.push(self.storage.delete_record(id));
            }
        }
        if project_file && self.project_file_is_ours() {
            results.push(self.storage.write_project_file(&self.project_file));
            self.clear_problem(storage::PROJECT_FILE);
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
    /// Forgets what was reported about the record files of `id` and about its folder.
    pub(crate) fn clear_record_problems(&mut self, id: &InstanceId) {
        for form in [Form::File, Form::Folder] {
            let path = self.storage.record_path(id, form);
            self.clear_problem(&self.storage.display_path(&path));
        }
        self.clear_problem(&self.storage.folder_display_path(id));
    }

    pub(crate) fn clear_problem(&mut self, path: &str) {
        if self.file_problems.remove(path).is_some() {
            self.push_event(ProjectEvent::ProblemsChanged);
        }
    }

    /// Brings in a `project.json` that changed on disk and that the watcher has not delivered
    /// yet, before an edit changes tempo or connections. So the edit lands on top of the
    /// outside change instead of writing over it.
    pub(crate) fn sync_project_file(&mut self) {
        if self.read_only || self.project_file_on_disk_is_known() {
            return;
        }
        let path = self.storage.root().join(storage::PROJECT_FILE);
        // A file that does not load is listed as a problem there, and `write` holds back.
        if let Err(error) = self.apply_outside_changes(&[path]) {
            self.report_problem(storage::PROJECT_FILE.to_string(), error.to_string());
        }
    }

    fn project_file_on_disk_is_known(&self) -> bool {
        match self.storage.read_project_file() {
            Ok(Some(bytes)) => {
                self.storage.project_file_fingerprint() == Some(storage::fingerprint(&bytes))
            }
            // Gone: nothing of an outside writer can be lost by writing it.
            Ok(None) => true,
            Err(_) => false,
        }
    }

    /// Whether `project.json` may be written. Not while the file holds an outside change that
    /// did not load: that file stays for correction. The edit is live, and the problem says
    /// that it is not saved.
    fn project_file_is_ours(&mut self) -> bool {
        const HELD_BACK: &str =
            "Tempo and connection edits are live but not written until this file loads";
        if self.project_file_on_disk_is_known() {
            return true;
        }
        let path = storage::PROJECT_FILE.to_string();
        let message = match self.file_problems.get(&path) {
            Some(existing) if existing.contains(HELD_BACK) => existing.clone(),
            Some(existing) => format!("{existing}. {HELD_BACK}"),
            None => format!("changed on disk. {HELD_BACK}"),
        };
        self.report_problem(path, message);
        false
    }

    pub(crate) fn report_problem(&mut self, path: String, message: String) {
        self.file_problems.insert(path, message);
        self.push_event(ProjectEvent::ProblemsChanged);
    }

    /// The generated files follow the problems and `project.json`.
    pub(crate) fn push_event(&mut self, event: ProjectEvent) {
        self.generated_are_stale |= matches!(
            event,
            ProjectEvent::ProblemsChanged | ProjectEvent::ProjectFileChanged
        );
        self.events.push(event);
    }
}

fn io_error(path: &Path, source: std::io::Error) -> ProjectError {
    ProjectError::Storage(StorageError::Io {
        path: path.display().to_string(),
        source,
    })
}
