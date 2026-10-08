//! Tool registration. Extensions register their tools here before a project opens.

use std::any::Any;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::sync::Arc;

use super::Project;
use super::binding::{BehaviourContext, BehaviourError};
use super::editing::Derived;
use super::instance::{Instance, InstanceId, JsonState, Record, State, is_valid_name};
use crate::clock::Ticks;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("a tool named {0:?} is already registered")]
    DuplicateTool(&'static str),
    #[error("an agent doc named {0:?} is already registered")]
    DuplicateAgentDoc(&'static str),
    #[error(
        "invalid agent doc name {0:?}: it becomes a file name, so it uses lowercase letters, digits, `-` and `_`"
    )]
    InvalidAgentDocName(&'static str),
    #[error(
        "invalid tool name {0:?}: a tool of the project names its doc file too, so it uses lowercase letters, digits, `-` and `_`"
    )]
    InvalidToolName(String),
}

/// A tool whose saved state is JSON checked by a function instead of a Rust type: a tool the
/// project defines for itself, such as one written in TypeScript. Its records load in every
/// project, with no extension to enable, and it may be defined again while the project is
/// open, see [`Project::define_json_tool`]. Its records live anywhere and own no children.
pub struct JsonTool {
    /// The `tool` of its records, and the name of its doc.
    pub name: String,
    /// Checks the `state` of a record on every path in, as [`State::validate`] does. The
    /// message names the field, as `state.rate: 25 is outside [0.1, 20]`.
    pub check: Arc<dyn Fn(&serde_json::Value) -> Result<(), String> + Send + Sync>,
    /// What [`ToolRegistration::behaviour`] is for a typed tool. It gets the `state`.
    pub behaviour:
        Box<dyn Fn(&serde_json::Value, &mut BehaviourContext<'_>) -> Result<(), BehaviourError>>,
    /// Its doc for agents, named as the tool.
    pub doc: Option<JsonToolDoc>,
}

/// See [`AgentDoc`], whose name is the tool's.
pub struct JsonToolDoc {
    pub when: String,
    pub markdown: String,
}

/// One doc for an agent that works in the project folder with only file access. The runtime
/// writes it as `agent-docs/<name>.md` and lists it in the map, `AGENTS.md`, so the agent
/// opens the doc its task needs and no other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentDoc {
    /// The file name under `agent-docs/`, without `.md`. Lowercase letters, digits and `-`.
    pub name: &'static str,
    /// The one line the map shows next to the doc: when to open it. No full stop needed.
    pub when: &'static str,
    /// The doc itself, starting with a `# ` heading.
    pub markdown: &'static str,
}

/// A doc as a project writes it: a view of a registered [`AgentDoc`], or of the doc of a tool
/// of the project, whose text changes while the project is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentDocText<'a> {
    pub name: &'a str,
    pub when: &'a str,
    pub markdown: &'a str,
}

/// An agent doc with the extension it belongs to. `None` is a doc every project gets.
pub(crate) struct RegisteredDoc {
    pub extension: Option<&'static str>,
    pub name: &'static str,
    /// Owned for the doc of a tool of the project, which is replaced at every save of it.
    pub when: Cow<'static, str>,
    pub markdown: Cow<'static, str>,
}

impl RegisteredDoc {
    fn of(extension: Option<&'static str>, doc: AgentDoc) -> Self {
        Self {
            extension,
            name: doc.name,
            when: Cow::Borrowed(doc.when),
            markdown: Cow::Borrowed(doc.markdown),
        }
    }

    fn text(&self) -> AgentDocText<'_> {
        AgentDocText {
            name: self.name,
            when: &self.when,
            markdown: &self.markdown,
        }
    }
}

/// Not `Send`: a behaviour may keep control-side state that belongs to the thread the project
/// lives on, such as the plugin instances of a host, which CLAP requires on the main thread.
/// The project has always lived on one thread.
type ErasedBehaviour =
    Box<dyn Fn(&dyn Any, &mut BehaviourContext<'_>) -> Result<(), BehaviourError>>;

/// Not `Send`, for the same reason a behaviour is not: it may read something the tool
/// keeps on the thread the project lives on, such as a cache of a file it parsed.
type ErasedSummary = Box<dyn Fn(&Project, &InstanceId) -> String>;

type ErasedEnd = Box<dyn Fn(&Project, &InstanceId) -> Option<Ticks>>;

/// Not `Send`, like a behaviour: it may keep control-side state, such as a cache of a big file
/// it reads.
type ErasedDerive = Box<dyn Fn(&Project, &InstanceId, DerivedFrom<'_>, &mut Derived)>;

/// The untyped form of [`Was`], which only this crate can build.
pub(crate) enum DerivedFrom<'a> {
    Created,
    Changed(&'a Record),
    Unchanged,
}

/// What the record of a derived instance was before the group that is running its derive.
///
/// A derive that writes several things uses it to write only what really moved: the tempo fit
/// rewrites the notes of a clip when a field that decides where the beats are changed, and
/// only the tempo map when the steadiness did, so a hand edit of that clip survives.
pub enum Was<'a, S> {
    /// The record was created in this group. There is nothing it was.
    Created,
    /// The record changed in this group, from this state.
    Changed(&'a S),
    /// The record did not change. Something else this derive follows did, such as the
    /// project's time signature.
    Unchanged,
}

pub(crate) struct ToolDefinition {
    /// `None` for a tool of the project itself, which every project loads.
    pub extension: Option<&'static str>,
    /// Decodes and validates the `state` value of a record. The error names the field.
    pub decode: Box<dyn Fn(serde_json::Value) -> Result<Record, String>>,
    pub behaviour: Option<ErasedBehaviour>,
    pub derive: Option<ErasedDerive>,
    pub summary: Option<ErasedSummary>,
    pub end: Option<ErasedEnd>,
    pub owns_children: bool,
    /// Folders under `assets/` whose changes run the behaviour of an instance with a problem
    /// again. See [`ToolRegistration::rebinds_on_assets`].
    pub asset_folders: Vec<&'static str>,
}

impl ToolDefinition {
    /// Whether a project whose `project.json` enables `extensions` loads records of this tool.
    pub(crate) fn is_enabled_in(&self, extensions: &[String]) -> bool {
        match self.extension {
            Some(extension) => extensions.iter().any(|enabled| enabled == extension),
            None => true,
        }
    }
}

/// Every tool the compiled extensions offer. Registering makes a type available. It creates
/// no instance and no sound.
pub struct Registry {
    tools: BTreeMap<&'static str, ToolDefinition>,
    /// In the order they were registered, which is the order of the list in the map. The doc
    /// of `project.json` is the first, so every project has it and its name is taken.
    agent_docs: Vec<RegisteredDoc>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
            // `project.json` is core, not an extension, so the core brings its doc itself.
            agent_docs: vec![RegisteredDoc::of(None, super::generated::PROJECT_FILE_DOC)],
        }
    }

    /// Registers the tool whose saved state is `S`, under `S::TOOL`, as part of `extension`.
    /// A project loads its records only when it enables that extension.
    pub fn tool<S: State>(
        &mut self,
        extension: &'static str,
    ) -> Result<ToolRegistration<'_, S>, RegistryError> {
        if self.tools.contains_key(S::TOOL) {
            return Err(RegistryError::DuplicateTool(S::TOOL));
        }
        let definition = self.tools.entry(S::TOOL).or_insert(ToolDefinition {
            extension: Some(extension),
            decode: Box::new(decode::<S>),
            behaviour: None,
            derive: None,
            summary: None,
            end: None,
            owns_children: S::OWNS_CHILDREN,
            asset_folders: Vec::new(),
        });
        Ok(ToolRegistration {
            definition,
            state: PhantomData,
        })
    }

    /// A doc of an extension for an agent with only file access: how to read and write the
    /// records of its tools, or how to do one task with them. A project gets the doc when it
    /// enables the extension. An extension may register several, one per task.
    pub fn agent_doc(
        &mut self,
        extension: &'static str,
        doc: AgentDoc,
    ) -> Result<(), RegistryError> {
        self.add_agent_doc(Some(extension), doc)
    }

    /// A doc about the program that runs the project, for example how to call it for a
    /// summary. Every project gets it. Keep it free of paths and of anything else that
    /// differs between machines: the doc is a file in the project folder, which may be in git.
    pub fn runtime_agent_doc(&mut self, doc: AgentDoc) -> Result<(), RegistryError> {
        self.add_agent_doc(None, doc)
    }

    fn add_agent_doc(
        &mut self,
        extension: Option<&'static str>,
        doc: AgentDoc,
    ) -> Result<(), RegistryError> {
        // The name becomes a file name in the project folder. Without this, `../notes` would
        // write outside the docs folder.
        if !is_valid_name(doc.name) {
            return Err(RegistryError::InvalidAgentDocName(doc.name));
        }
        // Two docs of one name would write over each other's file. The doc of `project.json`
        // is in the list from the start, so its name is taken like any other.
        if self.agent_docs.iter().any(|other| other.name == doc.name) {
            return Err(RegistryError::DuplicateAgentDoc(doc.name));
        }
        self.agent_docs.push(RegisteredDoc::of(extension, doc));
        Ok(())
    }

    /// Registers a tool of the project, or replaces the one of that name. A typed tool of that
    /// name, or a doc of an extension, is an error: they come with the runtime.
    pub fn json_tool(&mut self, tool: JsonTool) -> Result<(), RegistryError> {
        let JsonTool {
            name,
            check,
            behaviour,
            doc,
        } = tool;
        let name = match self.tools.get_key_value(name.as_str()) {
            Some((existing, definition)) if definition.extension.is_some() => {
                return Err(RegistryError::DuplicateTool(existing));
            }
            Some((existing, _)) => *existing,
            None if !is_valid_name(&name) => return Err(RegistryError::InvalidToolName(name)),
            // Names are few and live as long as the program: a tool defined again keeps its
            // name, so a name is leaked once.
            None => &*Box::leak(name.into_boxed_str()),
        };
        let same_name = |registered: &RegisteredDoc| registered.name == name;
        if let Some(registered) = self.agent_docs.iter().find(|doc| same_name(doc))
            && registered.extension.is_some()
        {
            return Err(RegistryError::DuplicateAgentDoc(registered.name));
        }
        self.agent_docs.retain(|registered| !same_name(registered));
        if let Some(JsonToolDoc { when, markdown }) = doc {
            self.agent_docs.push(RegisteredDoc {
                extension: None,
                name,
                when: Cow::Owned(when),
                markdown: Cow::Owned(markdown),
            });
        }
        let decode_check = check.clone();
        let decode = move |value: serde_json::Value| {
            decode_check(&value)?;
            let check = decode_check.clone();
            Ok(Record::json(name, JsonState { value, check }))
        };
        let behaviour = move |state: &dyn Any, context: &mut BehaviourContext<'_>| {
            match state.downcast_ref::<JsonState>() {
                Some(state) => behaviour(&state.value, context),
                // The record of an instance always holds the state type of its tool.
                None => Ok(()),
            }
        };
        self.tools.insert(
            name,
            ToolDefinition {
                extension: None,
                decode: Box::new(decode),
                behaviour: Some(Box::new(behaviour)),
                derive: None,
                summary: None,
                end: None,
                owns_children: false,
                asset_folders: Vec::new(),
            },
        );
        Ok(())
    }

    /// The docs of a project that enables `enabled`, in registration order.
    pub(crate) fn agent_docs<'a>(
        &'a self,
        enabled: &'a [String],
    ) -> impl Iterator<Item = AgentDocText<'a>> + 'a {
        self.agent_docs
            .iter()
            .filter(|registered| match registered.extension {
                Some(extension) => enabled.iter().any(|enabled| enabled == extension),
                None => true,
            })
            .map(RegisteredDoc::text)
    }

    /// Every tool as (name, its definition), in name order.
    pub(crate) fn tools(&self) -> impl Iterator<Item = (&'static str, &ToolDefinition)> {
        self.tools
            .iter()
            .map(|(name, definition)| (*name, definition))
    }

    pub(crate) fn definition(&self, tool: &str) -> Option<&ToolDefinition> {
        self.tools.get(tool)
    }

    /// The extensions of the runtime, which `project.json` enables. A tool of the project has
    /// none.
    pub(crate) fn extensions(&self) -> BTreeSet<&'static str> {
        self.tools
            .values()
            .filter_map(|definition| definition.extension)
            .collect()
    }
}

/// Attaches the optional capabilities of a tool that was just registered.
pub struct ToolRegistration<'a, S> {
    definition: &'a mut ToolDefinition,
    state: PhantomData<fn(S)>,
}

impl<S: State> ToolRegistration<'_, S> {
    /// How the state of an instance becomes processors, updates, ports and connections. See
    /// [`BehaviourContext`]. A tool that only holds data for its parent needs none.
    pub fn behaviour(
        self,
        behaviour: impl Fn(&S, &mut BehaviourContext<'_>) -> Result<(), BehaviourError> + 'static,
    ) -> Self {
        self.definition.behaviour = Some(Box::new(move |state, context| {
            match state.downcast_ref::<S>() {
                Some(state) => behaviour(state, context),
                // The record of an instance always holds the state type of its tool.
                None => Ok(()),
            }
        }));
        self
    }

    /// What this instance decides about other records and about the tempo map: state that is
    /// computed from its own, never edited by hand, and always rebuilt from it.
    ///
    /// It runs inside the same state application as the change that asked for it, so the
    /// record, what it decides and the tempo map are one group, one engine batch and one undo
    /// step. It runs when a record of this tool changed in the group, and when the project's
    /// time signature changed, which is the other thing a musical grid is made of. It does not
    /// run while the project loads, for undo, for redo or for a cancel: the files and the undo
    /// step already hold what it would compute, and a read-only project therefore writes
    /// nothing. Its own changes never start another round, so it cannot loop.
    ///
    /// Keep it a pure function of the project: it may be called several times for one gesture,
    /// once per mouse move. Say in your agent doc that the records it writes are derived, so an
    /// agent edits the input and not the result.
    ///
    /// [`Was`] is what the record was before this group, so a derive that writes several
    /// things can write only what really moved.
    pub fn derive(
        self,
        derive: impl Fn(&Project, &Instance<S>, Was<'_, S>, &mut Derived) + 'static,
    ) -> Self {
        self.definition.derive = Some(Box::new(move |project, id, from, derived| {
            let was = match from {
                DerivedFrom::Created => Was::Created,
                DerivedFrom::Unchanged => Was::Unchanged,
                // The record of an instance always holds the state type of its tool.
                DerivedFrom::Changed(record) => match record.state::<S>() {
                    Some(state) => Was::Changed(state),
                    None => Was::Unchanged,
                },
            };
            // Only called for a live instance of this tool.
            if let Some(instance) = project.resolve::<S>(id) {
                derive(project, &instance, was, derived);
            }
        }));
        self
    }

    /// How an instance of this tool describes itself and what it owns in a project summary,
    /// as lines of plain text. [`Project::summary`] calls it. Without one, a summary lists the
    /// instance and everything inside it as plain records. An owner of many small records
    /// gives one here, so that an agent reads one summary and not every record.
    pub fn summary(self, summary: impl Fn(&Project, &Instance<S>) -> String + 'static) -> Self {
        self.definition.summary = Some(Box::new(move |project, id| {
            match project.resolve::<S>(id) {
                Some(instance) => summary(project, &instance),
                // Only called for an instance of this tool.
                None => String::new(),
            }
        }));
        self
    }

    /// When a file under `assets/<folder>/` is added, changed or removed, every instance of
    /// this tool that has a problem runs its behaviour again, with the record it has.
    ///
    /// For a tool whose record names an asset that may arrive after the record, such as an
    /// audio clip whose file an agent copies in afterwards. Only instances with a problem run,
    /// so what plays is never touched, and only for the folders a tool names, so the assets
    /// the runtime writes itself, such as the state of a plugin, wake nothing else.
    pub fn rebinds_on_assets(self, folder: &'static str) -> Self {
        self.definition.asset_folders.push(folder);
        self
    }

    /// Where the content of an instance of this tool ends on the project timeline, `None`
    /// when it has none. [`Project::end`] is the latest of them. The core knows no clips, so
    /// this is how a transport shows a duration. A tool without one does not count: it runs
    /// without a set end.
    pub fn end(self, end: impl Fn(&Project, &Instance<S>) -> Option<Ticks> + 'static) -> Self {
        self.definition.end = Some(Box::new(move |project, id| {
            // Only called for an instance of this tool.
            end(project, &project.resolve::<S>(id)?)
        }));
        self
    }
}

fn decode<S: State>(value: serde_json::Value) -> Result<Record, String> {
    let state: S = serde_path_to_error::deserialize(value).map_err(|error| {
        let path = error.path().to_string();
        let field = if path == "." {
            "state".to_string()
        } else {
            format!("state.{path}")
        };
        format!("{field}: {}", error.inner())
    })?;
    state
        .validate()
        .map_err(|message| format!("state: {message}"))?;
    Ok(Record::new(state))
}
