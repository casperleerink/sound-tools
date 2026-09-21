//! Tool registration. Extensions register their tools here before a project opens.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use super::Project;
use super::binding::{BehaviourContext, BehaviourError};
use super::instance::{Instance, InstanceId, Record, State, is_valid_name};
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

/// An agent doc with the extension it belongs to. `None` is a doc every project gets.
pub(crate) struct RegisteredDoc {
    pub extension: Option<&'static str>,
    pub doc: AgentDoc,
}

/// Not `Send`: a behaviour may keep control-side state that belongs to the thread the project
/// lives on, such as the plugin instances of a host, which CLAP requires on the main thread.
/// The project has always lived on one thread.
type ErasedBehaviour =
    Box<dyn Fn(&dyn Any, &mut BehaviourContext<'_>) -> Result<(), BehaviourError>>;

type ErasedSummary = Box<dyn Fn(&Project, &InstanceId) -> String + Send>;

type ErasedEnd = Box<dyn Fn(&Project, &InstanceId) -> Option<Ticks> + Send>;

pub(crate) struct ToolDefinition {
    pub extension: &'static str,
    /// Decodes and validates the `state` value of a record. The error names the field.
    pub decode: fn(serde_json::Value) -> Result<Record, String>,
    pub behaviour: Option<ErasedBehaviour>,
    pub summary: Option<ErasedSummary>,
    pub end: Option<ErasedEnd>,
    pub owns_children: bool,
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
            agent_docs: vec![RegisteredDoc {
                extension: None,
                doc: super::generated::PROJECT_FILE_DOC,
            }],
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
            extension,
            decode: decode::<S>,
            behaviour: None,
            summary: None,
            end: None,
            owns_children: S::OWNS_CHILDREN,
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
        if self
            .agent_docs
            .iter()
            .any(|other| other.doc.name == doc.name)
        {
            return Err(RegistryError::DuplicateAgentDoc(doc.name));
        }
        self.agent_docs.push(RegisteredDoc { extension, doc });
        Ok(())
    }

    /// The docs of a project that enables `enabled`, in registration order.
    pub(crate) fn agent_docs<'a>(
        &'a self,
        enabled: &'a [String],
    ) -> impl Iterator<Item = AgentDoc> + 'a {
        self.agent_docs
            .iter()
            .filter(|registered| match registered.extension {
                Some(extension) => enabled.iter().any(|enabled| enabled == extension),
                None => true,
            })
            .map(|registered| registered.doc)
    }

    /// Every tool as (name, extension, owns children), in name order.
    pub(crate) fn tools(&self) -> impl Iterator<Item = (&'static str, &'static str, bool)> {
        let tools = self.tools.iter();
        tools.map(|(name, definition)| (*name, definition.extension, definition.owns_children))
    }

    pub(crate) fn definition(&self, tool: &str) -> Option<&ToolDefinition> {
        self.tools.get(tool)
    }

    pub(crate) fn extensions(&self) -> BTreeSet<&'static str> {
        self.tools
            .values()
            .map(|definition| definition.extension)
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

    /// How an instance of this tool describes itself and what it owns in a project summary,
    /// as lines of plain text. [`Project::summary`] calls it. Without one, a summary lists the
    /// instance and everything inside it as plain records. An owner of many small records
    /// gives one here, so that an agent reads one summary and not every record.
    pub fn summary(
        self,
        summary: impl Fn(&Project, &Instance<S>) -> String + Send + 'static,
    ) -> Self {
        self.definition.summary = Some(Box::new(move |project, id| {
            match project.resolve::<S>(id) {
                Some(instance) => summary(project, &instance),
                // Only called for an instance of this tool.
                None => String::new(),
            }
        }));
        self
    }

    /// Where the content of an instance of this tool ends on the project timeline, `None`
    /// when it has none. [`Project::end`] is the latest of them. The core knows no clips, so
    /// this is how a transport shows a duration. A tool without one does not count: it runs
    /// without a set end.
    pub fn end(
        self,
        end: impl Fn(&Project, &Instance<S>) -> Option<Ticks> + Send + 'static,
    ) -> Self {
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
