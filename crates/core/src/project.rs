//! The live project folder: typed instances in an ownership tree, one editing path with undo,
//! storage that follows every finished edit, a watcher for outside changes, and the binding
//! that turns instance state into engine processors.
//!
//! The project lives on one thread, the control thread. Nothing here locks. `README.md` in
//! this crate is the guide for extension authors.

mod apply;
mod assets;
mod binding;
mod editing;
mod fields;
mod file;
mod generated;
mod history;
mod instance;
mod live;
mod open_edits;
mod outside;
mod registry;
mod runs;
mod storage;
mod watcher;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub use assets::{ASSETS_FOLDER, AssetError, AssetName, Assets, InvalidAssetName};
pub use binding::{BehaviourContext, BehaviourError, InputEndpoint, OutputEndpoint, RecordStamp};
pub use editing::{Changes, Edit};
pub use fields::{FieldDoc, FieldRange, RecordSchema, ranges, type_doc, unit_of};
pub use file::{
    FORMAT, PortReference, ProjectFile, SavedConnection, SavedDestination, SavedSource,
};
pub use generated::{
    AGENT_DOC_FILE, AGENT_DOCS_FOLDER, INSTRUCTIONS_FILE, NO_PROBLEMS, PROBLEMS_FILE,
};
pub use history::{
    FileChange, HISTORY_FOLDER, History, Line, OUTSIDE_POINT_WINDOW, Point, PointId,
};
pub use instance::{Instance, InstanceId, InvalidInstanceId, Place, State};
pub use registry::{
    AgentDoc, AgentDocText, JsonTool, JsonToolDoc, Registry, RegistryError, ToolRegistration,
    ToolSchema, parse_state,
};
pub use runs::{RUN_LONGEST, RUN_QUIET};
pub use storage::StorageError;
pub use watcher::GROUPING_WINDOW;

use binding::{BindError, Bindings};
use editing::{Change, RecordChange};
use instance::{JsonState, Record};
use live::LiveSources;
use open_edits::OpenEdits;
use outside::NotApplied;
use storage::{Form, Locked, RecordOnDisk, Storage};
use watcher::Watcher;

use crate::automation::PlayedLanes;
use crate::clock::{Clock, Ticks};
use crate::control::EngineControl;
use crate::graph::GraphError;
use crate::parameter::AutomatedNumber;
use crate::peaks::Peaks;
use crate::processor::Processor;
use crate::scope::Scope;
use crate::watch::Watch;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("the project {0} is already open in another runtime")]
    AlreadyOpen(String),
    #[error("the project is open read-only")]
    ReadOnly,
    /// [`Project::apply_in_memory`] on a project that writes, whose next edit would save it.
    #[error("only a read-only project takes changes in memory: this one writes its files")]
    WritesFiles,
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
    #[error("the behaviour of {instance} failed: {source}")]
    Behaviour {
        instance: InstanceId,
        source: BehaviourError,
    },
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("the history of the project: {0}")]
    History(#[source] std::io::Error),
    #[error("the file watcher failed: {0}")]
    Watcher(#[from] notify::Error),
    #[error(
        "the file watcher stopped: changes to the project files from outside the app no longer apply. Open the project again"
    )]
    WatcherStopped,
    #[error(transparent)]
    Registry(#[from] RegistryError),
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
    /// The composer went back to this point in the history.
    WentBack(PointId),
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
    /// Opening the project: applying everything from empty. Also a change only in memory, see
    /// [`Project::apply_in_memory`]: like loading, it writes nothing, derives nothing and is no
    /// point.
    Load,
    Interface,
    /// The files already hold this state: an outside change, undo, redo.
    Outside,
    /// Cancelling an edit.
    Cancel,
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
    open_edits: OpenEdits,
    /// `None` when read-only, or when it could not be opened, which a problem says.
    history: Option<History>,
    events: Vec<ProjectEvent>,
    /// By path relative to the project folder.
    file_problems: BTreeMap<String, String>,
    /// Rejected groups of outside changes, see [`NotApplied`].
    not_applied: Vec<NotApplied>,
    /// The generated files may no longer match the project. See `generated.rs`.
    generated_are_stale: bool,
    watcher: Option<Watcher>,
    live_sources: LiveSources,
    /// The open run of each editor that has one, see [`Self::publish_run`].
    runs: Vec<runs::Run>,
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
    /// editing call but [`Self::apply_in_memory`] fails with [`ProjectError::ReadOnly`]. For
    /// inspecting and offline rendering.
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
            open_edits: OpenEdits::default(),
            history: None,
            events: Vec::new(),
            file_problems: BTreeMap::new(),
            not_applied: Vec::new(),
            generated_are_stale: true,
            watcher: None,
            live_sources: LiveSources::default(),
            runs: Vec::new(),
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
        project.write_generated_files()?;
        if !read_only {
            let kept_out = project.registry.kept_out_of_history();
            match History::open(project.storage.root(), kept_out) {
                Ok(history) => project.history = Some(history),
                Err(error) => {
                    let message = format!("the history is off: {error}");
                    project.report_problem(history::HISTORY_FOLDER.to_string(), message);
                }
            }
        }
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

    /// The sample rate of the engine, which is that of the output device.
    pub fn sample_rate(&self) -> u32 {
        self.engine.config().sample_rate
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

    /// The names of the sound inputs and outputs that the behaviour of `instance` named the
    /// last time it ran, such as `audio` and `sidechain`.
    pub fn sound_ports(&self, instance: &InstanceId) -> (Vec<&str>, Vec<&str>) {
        self.bindings.sound_ports(instance)
    }

    /// The number `field` that `instance` takes automation for, as its behaviour named it the
    /// last time it ran. `None` while it takes none of that name. For an owner that edits its
    /// lanes, such as a view that shows what a lane plays.
    pub fn automation(&self, instance: &InstanceId, field: &str) -> Option<AutomatedNumber> {
        self.bindings.automation(instance, field)
    }

    /// The names of every number `instance` takes automation for, in the order its behaviour
    /// named them the last time it ran, each of which [`Self::automation`] gives. For an owner
    /// that offers to automate them, such as a menu that adds a lane.
    pub fn automatable(&self, instance: &InstanceId) -> impl Iterator<Item = &str> + '_ {
        self.bindings.automatable(instance)
    }

    /// The lanes that play into the numbers of `instance`, as its owner showed them the last
    /// time its behaviour ran. `None` while no lane moves a number of it. For a view, whose
    /// knob of an automated number shows the value that plays at the playhead.
    pub fn lanes(&self, instance: &InstanceId) -> Option<&dyn PlayedLanes> {
        self.bindings.lanes(instance)
    }

    /// The peaks that the behaviour of `instance` keeps under `name`, for a meter: take from
    /// them once per frame. `None` while the instance keeps none of that name. They stay the
    /// same while the behaviour declares them, so a view may keep them, and asks again when
    /// the instance is created again.
    pub fn peaks(&self, instance: &InstanceId, name: &str) -> Option<Peaks> {
        self.bindings.peaks(instance, name).cloned()
    }

    /// The scope that the behaviour of `instance` keeps under `name`, see
    /// [`BehaviourContext::scope`]. It stays the same while the behaviour declares it.
    pub fn scope(&self, instance: &InstanceId, name: &str) -> Option<Scope> {
        self.bindings.scope(instance, name).cloned()
    }

    /// The tap of `T` the behaviour of `instance` keeps under `name`, see
    /// [`BehaviourContext::tap`].
    pub fn tap<T: crate::Tappable + 'static>(
        &self,
        instance: &InstanceId,
        name: &str,
    ) -> Option<crate::EventTap<T>> {
        self.bindings.tap(instance, name)
    }

    /// Every watch the behaviour of `instance` keeps, by name, see [`BehaviourContext::watch`].
    pub fn watches(&self, instance: &InstanceId) -> Vec<(String, Watch)> {
        self.bindings.watches(instance)
    }

    /// The value of the watch `name` of `instance`, see [`BehaviourContext::watch`].
    pub fn watched(&self, instance: &InstanceId, name: &str) -> Option<f32> {
        self.bindings.watch(instance, name).map(Watch::get)
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

    /// The `state` of an instance of a JSON tool, as its file holds it.
    pub(crate) fn json_state(&self, id: &InstanceId) -> Option<&serde_json::Value> {
        let state = self.instances.get(id)?.state.as_any();
        Some(&state.downcast_ref::<JsonState>()?.value)
    }

    /// What changes when the record of `id` does, to keep what was made of it until then.
    pub fn stamp(&self, id: &InstanceId) -> Option<RecordStamp> {
        Some(RecordStamp::of(&self.instances.get(id)?.state))
    }

    /// The state of any instance as compact JSON, for summaries.
    pub fn state_json(&self, id: &InstanceId) -> Option<String> {
        self.instances.get(id)?.state.to_json().ok()
    }

    /// Runs the behaviour of one instance again, with the record it already has.
    ///
    /// It is not an edit: nothing is written, nothing is undoable, and the record is untouched.
    /// It is for when something outside the record is ready now that was not before: the plugin
    /// scan found a plugin a record waited for, or a plugin asked to be loaded again; a Drum
    /// pad's sounds finished rendering; a Sampler's instrument or a sample finished loading; an
    /// asset folder a tool watches changed; the code of a project tool changed. A behaviour that
    /// fails here leaves the instance as it was, as any failed group does.
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

    /// Defines tools of the project while it is open, or defines them again because their
    /// code changed. Their records are read again, those of live instances and those that did
    /// not load, so one that waited for its tool loads and one the new check refuses is a
    /// problem, and every instance of them runs the new behaviour. Where that fails, what
    /// plays stays as it is and the record says why, as for a failed file. Nothing is written
    /// and nothing is an undo step: no record changed. Every other file is left to the
    /// watcher, so an outside edit of it is applied as one. All the tools of one save come in
    /// one call, which reads their records once. A tool the registry refuses, such as one
    /// named as a tool of the runtime, is left out and returned with why.
    pub fn define_json_tools(
        &mut self,
        tools: impl IntoIterator<Item = JsonTool>,
    ) -> Result<Vec<(String, RegistryError)>, ProjectError> {
        let mut names = BTreeSet::new();
        let mut refused = Vec::new();
        for tool in tools {
            let name = tool.name.clone();
            match self.registry.json_tool(tool) {
                Ok(()) => _ = names.insert(name),
                Err(error) => refused.push((name, error)),
            }
        }
        self.generated_are_stale = true;
        let live: Vec<InstanceId> = (self.instances.iter())
            .filter(|(_, record)| names.contains(record.tool))
            .map(|(id, _)| id.clone())
            .collect();
        // Read again even where the file did not change: the check did.
        for id in &live {
            self.storage.forget(id);
        }
        let record_path = |project: &Self, id: &InstanceId| {
            let form = project.instances.get(id).map_or(Form::File, Form::of);
            project.storage.record_path(id, form)
        };
        let mut paths: Vec<PathBuf> = (live.iter()).map(|id| record_path(self, id)).collect();
        // Records of these tools that did not load: one waited for its tool, or the old check
        // or behaviour refused it. Not the whole folder: an outside edit the watcher has not
        // delivered yet would apply here with no undo step and no derive, and then look like
        // no change when it is delivered. A file that does not read keeps its problem.
        let state_folder = self.storage.state_folder();
        let not_loaded = (self.file_problems.keys())
            .map(|path| self.storage.root().join(path))
            .filter(|path| path.starts_with(&state_folder))
            .filter(|path| storage::tool_named_in(path).is_some_and(|tool| names.contains(&tool)));
        paths.extend(not_loaded);
        self.apply_paths(&paths, Source::Load)?;
        for id in live {
            let path = record_path(self, &id);
            let path = self.storage.display_path(&path);
            // The file did not load, such as because the new check refuses it: it plays on
            // as it was, under its problem.
            if self.file_problems.contains_key(&path) {
                continue;
            }
            if let Err(error) = self.rebind(&id) {
                self.report_problem(path, format!("its behaviour failed: {error}"));
            }
        }
        Ok(refused)
    }

    /// Says what is wrong with files outside `state/` that the runtime reads, such as the code
    /// of the project's own tools under `extensions/`. It replaces every problem whose path
    /// starts with `folder`, so a fixed file drops off the list.
    pub fn set_problems_in(&mut self, folder: &str, problems: Vec<Problem>) {
        let before = self.file_problems.len();
        self.file_problems
            .retain(|path, _| !path.starts_with(folder));
        let cleared = self.file_problems.len() != before;
        for problem in &problems {
            self.file_problems
                .insert(problem.path.clone(), problem.message.clone());
        }
        if cleared || !problems.is_empty() {
            self.push_event(ProjectEvent::ProblemsChanged);
        }
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
        let connections = (self.bindings.connection_problems().iter().cloned())
            .chain(self.live_input_problems())
            .map(|message| Problem {
                path: storage::PROJECT_FILE.to_string(),
                message,
            });
        // What a behaviour said about its own instance while it ran. The record is
        // live and untouched; part of what it asks for is not.
        files
            .chain(connections)
            .chain(self.instance_problems())
            .collect()
    }

    /// What every behaviour reported about its own instance, on the path of its record.
    fn instance_problems(&self) -> Vec<Problem> {
        self.bindings
            .instance_problems()
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
        self.report_write_errors(results)
    }

    /// Reports every failure as a problem and returns the first.
    fn report_write_errors(
        &mut self,
        results: Vec<Result<(), StorageError>>,
    ) -> Result<(), ProjectError> {
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
        for form in Form::BOTH {
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
