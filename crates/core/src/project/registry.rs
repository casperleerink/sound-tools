//! Tool registration. Extensions register their tools here before a project opens.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use super::binding::{BehaviourContext, BehaviourError};
use super::instance::{Record, State};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("a tool named {0:?} is already registered")]
    DuplicateTool(&'static str),
}

type ErasedBehaviour =
    Box<dyn Fn(&dyn Any, &mut BehaviourContext<'_>) -> Result<(), BehaviourError> + Send>;

pub(crate) struct ToolDefinition {
    pub extension: &'static str,
    /// Decodes and validates the `state` value of a record. The error names the field.
    pub decode: fn(serde_json::Value) -> Result<Record, String>,
    pub behaviour: Option<ErasedBehaviour>,
}

/// Every tool the compiled extensions offer. Registering makes a type available. It creates
/// no instance and no sound.
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<&'static str, ToolDefinition>,
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
        });
        Ok(ToolRegistration {
            definition,
            state: PhantomData,
        })
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
    ) {
        self.definition.behaviour = Some(Box::new(move |state, context| {
            match state.downcast_ref::<S>() {
                Some(state) => behaviour(state, context),
                // The record of an instance always holds the state type of its tool.
                None => Ok(()),
            }
        }));
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
