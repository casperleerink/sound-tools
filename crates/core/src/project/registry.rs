//! Tool registration. Extensions register their tools here before a project opens.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use super::Project;
use super::binding::{BehaviourContext, BehaviourError};
use super::instance::{Instance, InstanceId, Record, State};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("a tool named {0:?} is already registered")]
    DuplicateTool(&'static str),
}

type ErasedBehaviour =
    Box<dyn Fn(&dyn Any, &mut BehaviourContext<'_>) -> Result<(), BehaviourError> + Send>;

type ErasedSummary = Box<dyn Fn(&Project, &InstanceId) -> String + Send>;

pub(crate) struct ToolDefinition {
    pub extension: &'static str,
    /// Decodes and validates the `state` value of a record. The error names the field.
    pub decode: fn(serde_json::Value) -> Result<Record, String>,
    pub behaviour: Option<ErasedBehaviour>,
    pub summary: Option<ErasedSummary>,
    pub owns_children: bool,
}

/// Every tool the compiled extensions offer. Registering makes a type available. It creates
/// no instance and no sound.
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<&'static str, ToolDefinition>,
    /// By extension name.
    agent_docs: BTreeMap<&'static str, &'static str>,
    runtime_agent_doc: Option<&'static str>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
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
            owns_children: S::OWNS_CHILDREN,
        });
        Ok(ToolRegistration {
            definition,
            state: PhantomData,
        })
    }

    /// The section of an extension in the agent doc that the runtime writes into every
    /// project folder, as markdown that starts with a `## ` heading. It tells an agent with
    /// only file access how to read and write the records of the extension's tools. A project
    /// gets the section when it enables the extension. Registering again replaces the text.
    pub fn agent_doc(&mut self, extension: &'static str, markdown: &'static str) {
        self.agent_docs.insert(extension, markdown);
    }

    /// A last section of the agent doc about the program that runs the project, for example
    /// how to call it for a summary. Every project gets it. Keep it free of paths and of
    /// anything else that differs between machines: the doc is a file in the project folder,
    /// which may be in git.
    pub fn runtime_agent_doc_section(&mut self, markdown: &'static str) {
        self.runtime_agent_doc = Some(markdown);
    }

    pub(crate) fn runtime_agent_doc(&self) -> Option<&'static str> {
        self.runtime_agent_doc
    }

    pub(crate) fn agent_doc_of(&self, extension: &str) -> Option<&'static str> {
        self.agent_docs.get(extension).copied()
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
        behaviour: impl Fn(&S, &mut BehaviourContext<'_>) -> Result<(), BehaviourError> + Send + 'static,
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
