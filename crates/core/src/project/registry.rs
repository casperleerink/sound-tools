//! Tool registration. Extensions register their tools here before a project opens.

use std::any::Any;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::sync::Arc;

use schemars::JsonSchema;

use super::Project;
use super::binding::{BehaviourContext, BehaviourError};
use super::instance::{Instance, InstanceId, JsonState, Record, State, is_valid_name};
use crate::clock::Ticks;
use crate::parameter::{Parameter, ParameterInfo};

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
/// open, see [`Project::define_json_tools`]. Its records live anywhere.
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
    /// Folders under `assets/` whose changes run its behaviour again where it had a problem,
    /// as [`ToolRegistration::rebinds_on_assets`] does for a typed tool.
    pub asset_folders: Vec<&'static str>,
    /// What [`State::OWNS_CHILDREN`] is for a typed tool.
    pub owns_children: bool,
    /// What [`ToolRegistration::end`] is for a typed tool. It gets the project and the `state`.
    pub end: Option<Box<dyn Fn(&Project, &serde_json::Value) -> Option<Ticks>>>,
}

/// A built-in tool as the code of a tool of the project sees it, see
/// [`ToolRegistration::schema`].
#[derive(Clone, Debug)]
pub struct ToolSchema {
    pub tool: &'static str,
    /// The JSON Schema of its `state`, made from its type: a doc comment is a description.
    pub schema: serde_json::Value,
    /// The numbers a cable or a signal moves, each with its default, by their paths in the
    /// record.
    pub numbers: Vec<(ParameterInfo, f32)>,
    /// The other numbers a parent plays it from, such as `launch` of the timeline.
    pub live: &'static [&'static str],
    /// The numbers it sends out, which a parent connects, such as `value` of a lane.
    pub outputs: &'static [&'static str],
    /// Checks a `state` as a record of the tool is checked. The message names the field.
    pub check: fn(serde_json::Value) -> Result<(), String>,
}

impl ToolSchema {
    /// The schema of the records of `S`, whose `numbers` a cable or a signal moves.
    pub fn of<S: State + JsonSchema>(numbers: &[&Parameter<S>]) -> Self {
        Self {
            tool: S::TOOL,
            schema: schemars::schema_for!(S).to_value(),
            numbers: (numbers.iter())
                .map(|parameter| (parameter.info(), parameter.default))
                .collect(),
            live: &[],
            outputs: &[],
            check: |state| decode::<S>(state).map(drop),
        }
    }
}

/// See [`AgentDoc`], whose name is the tool's.
pub struct JsonToolDoc {
    pub group: &'static str,
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
    /// The heading the map lists the doc under. Groups follow the order of their first doc.
    pub group: &'static str,
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
    pub group: &'a str,
    pub when: &'a str,
    pub markdown: &'a str,
}

/// An agent doc with the extension it belongs to. `None` is a doc every project gets.
pub(crate) struct RegisteredDoc {
    pub extension: Option<&'static str>,
    pub name: &'static str,
    pub group: &'static str,
    /// Owned for the doc of a tool of the project, which is replaced at every save of it.
    pub when: Cow<'static, str>,
    pub markdown: Cow<'static, str>,
}

impl RegisteredDoc {
    fn of(extension: Option<&'static str>, doc: AgentDoc) -> Self {
        Self {
            extension,
            name: doc.name,
            group: doc.group,
            when: Cow::Borrowed(doc.when),
            markdown: Cow::Borrowed(doc.markdown),
        }
    }

    fn text(&self) -> AgentDocText<'_> {
        AgentDocText {
            name: self.name,
            group: self.group,
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

type ErasedEnd = Box<dyn Fn(&Project, &InstanceId) -> Option<Ticks>>;

pub(crate) struct ToolDefinition {
    /// `None` for a tool of the project itself, which every project loads.
    pub extension: Option<&'static str>,
    /// Decodes and validates the `state` value of a record. The error names the field.
    pub decode: Box<dyn Fn(serde_json::Value) -> Result<Record, String>>,
    pub behaviour: Option<ErasedBehaviour>,
    pub end: Option<ErasedEnd>,
    pub owns_children: bool,
    /// Folders under `assets/` whose changes run the behaviour of an instance with a problem
    /// again. See [`ToolRegistration::rebinds_on_assets`].
    pub asset_folders: Vec<&'static str>,
    pub schema: Option<ToolSchema>,
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
    /// See [`Self::keep_out_of_history`].
    kept_out_of_history: Vec<&'static str>,
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
            kept_out_of_history: Vec::new(),
        }
    }

    /// Files and folders at the top of `extensions/` that the runtime writes itself, such as
    /// the SDK of the project's tools. The history of a project leaves them out.
    pub fn keep_out_of_history(&mut self, names: &[&'static str]) {
        self.kept_out_of_history.extend_from_slice(names);
    }

    pub(crate) fn kept_out_of_history(&self) -> Vec<&'static str> {
        self.kept_out_of_history.clone()
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
            end: None,
            owns_children: S::OWNS_CHILDREN,
            asset_folders: Vec::new(),
            schema: None,
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
        // is in the list from the start, so its name is taken like any other. A tool of the
        // project names its doc as itself.
        let project_tool = self.tools.get(doc.name);
        if self.agent_docs.iter().any(|other| other.name == doc.name)
            || project_tool.is_some_and(|tool| tool.extension.is_none())
        {
            return Err(RegistryError::DuplicateAgentDoc(doc.name));
        }
        self.agent_docs.push(RegisteredDoc::of(extension, doc));
        Ok(())
    }

    /// Registers a tool of the project, or replaces the one of that name. A typed tool or
    /// another doc of that name is an error: they come with the runtime.
    pub fn json_tool(&mut self, tool: JsonTool) -> Result<(), RegistryError> {
        let JsonTool {
            name,
            check,
            behaviour,
            doc,
            asset_folders,
            owns_children,
            end,
        } = tool;
        let name = match self.tools.get_key_value(name.as_str()) {
            Some((existing, definition)) if definition.extension.is_some() => {
                return Err(RegistryError::DuplicateTool(existing));
            }
            Some((existing, _)) => *existing,
            None if !is_valid_name(&name) => return Err(RegistryError::InvalidToolName(name)),
            None => {
                // Its doc is named as the tool, and a doc of the runtime may have the name.
                let taken = self.agent_docs.iter().find(|other| other.name == name);
                if let Some(registered) = taken {
                    return Err(RegistryError::DuplicateAgentDoc(registered.name));
                }
                // Names are few and live as long as the program: a tool defined again keeps
                // its name, so a name is leaked once.
                &*Box::leak(name.into_boxed_str())
            }
        };
        // Neither a new tool nor a new doc takes the name of the other, so a doc of this name
        // is the one this tool had.
        self.agent_docs.retain(|registered| registered.name != name);
        if let Some(JsonToolDoc {
            group,
            when,
            markdown,
        }) = doc
        {
            self.agent_docs.push(RegisteredDoc {
                extension: None,
                name,
                group,
                when: Cow::Owned(when),
                markdown: Cow::Owned(markdown),
            });
        }
        let decode_check = check.clone();
        let decode = move |value: serde_json::Value| {
            decode_check(&value)?;
            let check = decode_check.clone();
            Ok(Record::json(
                name,
                JsonState { value, check },
                owns_children,
            ))
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
                end: end.map(|end| -> ErasedEnd {
                    Box::new(move |project, id| end(project, project.json_state(id)?))
                }),
                owns_children,
                asset_folders,
                schema: None,
            },
        );
        Ok(())
    }

    /// The tools that describe their record to the code of the tools of a project, by name.
    pub fn tool_schemas(&self) -> Vec<ToolSchema> {
        (self.tools.values())
            .filter_map(|definition| definition.schema.clone())
            .collect()
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
    pub fn extensions(&self) -> BTreeSet<&'static str> {
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

    /// Describes the record to the code of the tools of a project and to the agent docs: its
    /// type and docs come from the schema of `S`, and `numbers` are those a cable or a signal
    /// moves. A [`crate::ranges`] transform on `S` gives the docs the range of each number.
    pub fn schema(self, numbers: &[&Parameter<S>]) -> Self
    where
        S: JsonSchema,
    {
        self.definition.schema = Some(ToolSchema::of(numbers));
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
    let state: S = parse_state(&value)?;
    state
        .validate()
        .map_err(|message| format!("state: {message}"))?;
    Ok(Record::new(state))
}

/// A record's `state` read as `S`, or what is wrong with it at its path, such as
/// `state.points[2].tick: …`.
pub fn parse_state<S: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Result<S, String> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let path = error.path().to_string();
        let field = match path.as_str() {
            "." => "state".to_string(),
            _ => format!("state.{path}"),
        };
        let inner = error.inner().to_string();
        // JavaScript numbers are all floats: say the fix where a whole number was wanted.
        let whole = inner.starts_with("invalid type: floating point")
            && [", expected u", ", expected i"]
                .iter()
                .any(|kind| inner.contains(kind));
        match whole {
            true => {
                format!("{field}: {inner}; it must be a whole number, so round it (Math.round)")
            }
            false => format!("{field}: {inner}"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::parse_state;

    #[derive(Debug, serde::Deserialize)]
    struct Point {
        #[allow(dead_code)]
        tick: u64,
    }

    #[test]
    fn a_float_where_a_whole_number_goes_says_to_round_it() {
        let error = parse_state::<Vec<Point>>(&serde_json::json!([{ "tick": 0 }, { "tick": 1.5 }]))
            .unwrap_err();
        assert!(error.starts_with("state.[1].tick: invalid type"), "{error}");
        assert!(error.ends_with("so round it (Math.round)"), "{error}");
    }
}
