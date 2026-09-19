//! The engine binding: runs tool behaviours and turns what they declare into one engine edit.
//!
//! A behaviour declares everything its instance needs, every time it runs. The binding keeps
//! what was declared last time and sends only the difference. So a behaviour is a plain
//! function of the state, and it cannot leave processors or connections behind.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet};

use super::file::{PortReference, SavedConnection, SavedDestination};
use super::instance::{InstanceId, Record, State};
use super::registry::Registry;
use crate::control::{Edit, EngineControl, Node};
use crate::engine::ErasedProcessor;
use crate::graph::{Connection, Destination, GraphError, NodeId};
use crate::processor::{InputPort, OutputPort, Ports, PrepareConfig, Processor};

/// Why a behaviour could not apply a state. It rejects the whole edit group.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum BehaviourError {
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error("{0}")]
    Other(String),
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub(crate) enum BindError {
    #[error("the behaviour of {instance} failed: {source}")]
    Behaviour {
        instance: InstanceId,
        source: BehaviourError,
    },
    #[error(transparent)]
    Graph(#[from] GraphError),
}

/// An output port of a processor, as a value a behaviour can expose and connect.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OutputEndpoint {
    pub node: NodeId,
    pub port: OutputPort,
}

impl OutputEndpoint {
    pub fn new<P>(node: Node<P>, port: impl Into<OutputPort>) -> Self {
        Self {
            node: node.id(),
            port: port.into(),
        }
    }

    pub fn to(self, input: InputEndpoint) -> Connection {
        Connection::new(self.node, self.port, input.node, input.port)
    }

    pub fn to_device(self, channel: usize) -> Connection {
        Connection::to_device(self.node, self.port, channel)
    }
}

/// An input port of a processor.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InputEndpoint {
    pub node: NodeId,
    pub port: InputPort,
}

impl InputEndpoint {
    pub fn new<P>(node: Node<P>, port: impl Into<InputPort>) -> Self {
        Self {
            node: node.id(),
            port: port.into(),
        }
    }
}

/// What the behaviour of one instance declared the last time it ran.
#[derive(Default)]
struct Binding {
    /// By the local name the behaviour chose. The type id guards the typed handle.
    nodes: BTreeMap<String, (NodeId, TypeId)>,
    connections: BTreeSet<Connection>,
    outputs: BTreeMap<String, OutputEndpoint>,
    inputs: BTreeMap<String, InputEndpoint>,
}

/// The non-generic part of [`Edit`]. A trait object hides the lifetime of the edit, so
/// [`BehaviourContext`] has one lifetime.
trait EngineEdit {
    fn prepare_config(&self) -> PrepareConfig;
    fn add(
        &mut self,
        name: &str,
        ports: Ports,
        processor: Box<dyn ErasedProcessor>,
    ) -> Result<NodeId, GraphError>;
    fn update(&mut self, node: NodeId, update: Box<dyn Any + Send>) -> Result<(), GraphError>;
    fn connect(&mut self, connection: Connection) -> Result<(), GraphError>;
}

impl EngineEdit for Edit<'_> {
    fn prepare_config(&self) -> PrepareConfig {
        Edit::prepare_config(self)
    }

    fn add(
        &mut self,
        name: &str,
        ports: Ports,
        processor: Box<dyn ErasedProcessor>,
    ) -> Result<NodeId, GraphError> {
        self.add_prepared(name, ports, processor)
    }

    fn update(&mut self, node: NodeId, update: Box<dyn Any + Send>) -> Result<(), GraphError> {
        self.update_erased(node, update)
    }

    fn connect(&mut self, connection: Connection) -> Result<(), GraphError> {
        Edit::connect(self, connection)
    }
}

/// What a behaviour works with while it applies the state of one instance.
///
/// Declare every processor, connection and port the instance needs, each time. What is not
/// declared again is removed. What was declared before is kept, so processors keep their
/// runtime state.
pub struct BehaviourContext<'a> {
    id: &'a InstanceId,
    edit: &'a mut (dyn EngineEdit + 'a),
    instances: &'a BTreeMap<InstanceId, Record>,
    bindings: &'a BTreeMap<InstanceId, Binding>,
    previous: Option<&'a Binding>,
    next: Binding,
    device_channels: usize,
}

impl BehaviourContext<'_> {
    pub fn id(&self) -> &InstanceId {
        self.id
    }

    /// Output channels of the device. A tool that sounds with no connection in `project.json`
    /// connects itself to these.
    pub fn device_channels(&self) -> usize {
        self.device_channels
    }

    /// The processor this instance keeps under `name`. `create` runs only when it does not
    /// exist yet. Send the current values with [`Self::update`] after this, in both cases.
    pub fn processor<P: Processor>(
        &mut self,
        name: &str,
        create: impl FnOnce() -> P,
    ) -> Result<Node<P>, BehaviourError> {
        if let Some((id, _)) = self.next.nodes.get(name) {
            return Err(BehaviourError::Other(format!(
                "processor {name:?} is declared twice ({id:?})"
            )));
        }
        let kept = self
            .previous
            .and_then(|previous| previous.nodes.get(name))
            .filter(|(_, processor_type)| *processor_type == TypeId::of::<P>());
        let id = match kept {
            Some((id, _)) => *id,
            None => {
                let mut processor = create();
                processor.prepare(&self.edit.prepare_config());
                let ports = processor.ports();
                let graph_name = format!("{}#{name}", self.id);
                self.edit.add(&graph_name, ports, Box::new(processor))?
            }
        };
        self.next
            .nodes
            .insert(name.to_string(), (id, TypeId::of::<P>()));
        Ok(Node::from_id(id))
    }

    /// Sends parameters or an `Arc` snapshot. It applies in the same block as the rest of the
    /// edit group.
    pub fn update<P: Processor>(
        &mut self,
        node: Node<P>,
        update: P::Update,
    ) -> Result<(), BehaviourError> {
        Ok(self.edit.update(node.id(), Box::new(update))?)
    }

    /// A connection this instance makes by itself: between its processors, to a port of a
    /// child, or to the device. It is not saved in `project.json`.
    pub fn connect(&mut self, connection: Connection) -> Result<(), BehaviourError> {
        let kept = self
            .previous
            .is_some_and(|previous| previous.connections.contains(&connection));
        if !kept {
            self.edit.connect(connection)?;
        }
        self.next.connections.insert(connection);
        Ok(())
    }

    /// Names an output so that `project.json` connections and the parent can use it. It may be
    /// the endpoint of a child, to pass that port up.
    pub fn output(&mut self, name: &str, endpoint: OutputEndpoint) {
        self.next.outputs.insert(name.to_string(), endpoint);
    }

    pub fn input(&mut self, name: &str, endpoint: InputEndpoint) {
        self.next.inputs.insert(name.to_string(), endpoint);
    }

    /// The owned children that hold state of type `C`, as (name, state), in name order.
    pub fn children<C: State>(&self) -> impl Iterator<Item = (&str, &C)> {
        children_of(self.instances, self.id)
            .filter_map(|(id, record)| Some((id.name(), record.state::<C>()?)))
    }

    /// The state of the owned child `name`, when it exists and holds a `C`.
    pub fn child<C: State>(&self, name: &str) -> Option<&C> {
        self.instances.get(&self.id.child(name).ok()?)?.state()
    }

    /// A port that the owned child `name` exposed. Children run before their parent.
    pub fn child_output(&self, name: &str, port: &str) -> Option<OutputEndpoint> {
        let binding = self.bindings.get(&self.id.child(name).ok()?)?;
        binding.outputs.get(port).copied()
    }

    pub fn child_input(&self, name: &str, port: &str) -> Option<InputEndpoint> {
        let binding = self.bindings.get(&self.id.child(name).ok()?)?;
        binding.inputs.get(port).copied()
    }
}

/// The direct children of `parent`, in name order.
pub(crate) fn children_of<'a>(
    instances: &'a BTreeMap<InstanceId, Record>,
    parent: &InstanceId,
) -> impl Iterator<Item = (&'a InstanceId, &'a Record)> {
    let depth = parent.depth() + 1;
    parent
        .inside(instances)
        .filter(move |(id, _)| id.depth() == depth)
}

/// One state application, as the engine binding sees it.
pub(crate) struct EngineChange<'a> {
    pub registry: &'a Registry,
    /// The instances after the change.
    pub instances: &'a BTreeMap<InstanceId, Record>,
    /// Instances whose processors go away: deleted ones, and ones whose tool changed.
    pub unbound: &'a [InstanceId],
    /// Instances whose behaviour runs again.
    pub dirty: BTreeSet<InstanceId>,
    pub connections: &'a [SavedConnection],
}

#[derive(Default)]
pub(crate) struct Bindings {
    by_instance: BTreeMap<InstanceId, Binding>,
    /// The `project.json` connections that resolved, as they are in the graph.
    saved: BTreeSet<Connection>,
    /// One message per `project.json` connection that is not in the graph, and why.
    connection_problems: Vec<String>,
}

fn touches(connection: &Connection, removed: &BTreeSet<NodeId>) -> bool {
    removed.contains(&connection.source)
        || matches!(connection.destination, Destination::Node(node, _) if removed.contains(&node))
}

impl Bindings {
    pub fn connection_problems(&self) -> &[String] {
        &self.connection_problems
    }

    /// Runs the whole change as one engine edit: one batch and at most one compile. On an
    /// error the engine and the bindings stay as they were.
    pub fn apply(
        &mut self,
        control: &mut EngineControl,
        change: EngineChange<'_>,
    ) -> Result<(), BindError> {
        let mut backups = Vec::new();
        let device_channels = control.config().channels;
        let mut edit = control.edit();
        let result = self
            .run(&mut edit, device_channels, &change, &mut backups)
            .and_then(|resolved| {
                edit.commit()?;
                Ok(resolved)
            });
        match result {
            Ok((saved, problems)) => {
                self.saved = saved;
                self.connection_problems = problems;
                Ok(())
            }
            Err(error) => {
                for (id, binding) in backups.into_iter().rev() {
                    match binding {
                        Some(binding) => self.by_instance.insert(id, binding),
                        None => self.by_instance.remove(&id),
                    };
                }
                Err(error)
            }
        }
    }

    fn run(
        &mut self,
        edit: &mut Edit<'_>,
        device_channels: usize,
        change: &EngineChange<'_>,
        backups: &mut Vec<(InstanceId, Option<Binding>)>,
    ) -> Result<(BTreeSet<Connection>, Vec<String>), BindError> {
        // Removing a processor also removes its connections from the graph.
        let mut removed = BTreeSet::new();
        for id in change.unbound {
            if let Some(binding) = self.by_instance.remove(id) {
                for (node, _) in binding.nodes.values() {
                    edit.remove_processor(*node)?;
                    removed.insert(*node);
                }
                backups.push((id.clone(), Some(binding)));
            }
        }

        // Children before parents, so a parent finds the ports of its children.
        let mut dirty: Vec<&InstanceId> = change.dirty.iter().collect();
        dirty.sort_by_key(|id| std::cmp::Reverse(id.depth()));
        for id in dirty {
            let Some(record) = change.instances.get(id) else {
                continue;
            };
            let definition = change.registry.definition(record.tool);
            let Some(behaviour) = definition.and_then(|definition| definition.behaviour.as_ref())
            else {
                continue;
            };
            backups.push((id.clone(), self.by_instance.remove(id)));
            let previous = backups.last().and_then(|(_, binding)| binding.as_ref());
            let mut context = BehaviourContext {
                id,
                edit: &mut *edit,
                instances: change.instances,
                bindings: &self.by_instance,
                previous,
                next: Binding::default(),
                device_channels,
            };
            behaviour(record.state.as_any(), &mut context).map_err(|source| {
                BindError::Behaviour {
                    instance: id.clone(),
                    source,
                }
            })?;
            let next = context.next;
            if let Some(previous) = previous {
                for (name, (node, _)) in &previous.nodes {
                    let kept = next.nodes.get(name).is_some_and(|(kept, _)| kept == node);
                    if !kept {
                        edit.remove_processor(*node)?;
                        removed.insert(*node);
                    }
                }
                for connection in previous.connections.difference(&next.connections) {
                    if !touches(connection, &removed) && !self.saved.contains(connection) {
                        edit.disconnect(connection)?;
                    }
                }
            }
            self.by_instance.insert(id.clone(), next);
        }

        let mut saved = BTreeSet::new();
        let mut problems = Vec::new();
        for (index, connection) in change.connections.iter().enumerate() {
            let resolved = self.resolve(connection).and_then(|resolved| {
                if !self.saved.contains(&resolved) {
                    edit.connect(resolved).map_err(|error| error.to_string())?;
                }
                Ok(resolved)
            });
            match resolved {
                Ok(resolved) => {
                    saved.insert(resolved);
                }
                Err(message) => problems.push(format!("connections[{index}]: {message}")),
            }
        }
        for connection in self.saved.difference(&saved) {
            let declared = |binding: &Binding| binding.connections.contains(connection);
            if !touches(connection, &removed) && !self.by_instance.values().any(declared) {
                edit.disconnect(connection)?;
            }
        }
        Ok((saved, problems))
    }

    fn resolve(&self, connection: &SavedConnection) -> Result<Connection, String> {
        let binding = |port: &PortReference| {
            self.by_instance.get(&port.instance).ok_or_else(|| {
                format!(
                    "instance {:?} does not exist or has no ports",
                    port.instance.as_str()
                )
            })
        };
        let from = &connection.from;
        let output = binding(from)?.outputs.get(&from.port).ok_or_else(|| {
            format!(
                "instance {:?} has no output {:?}",
                from.instance.as_str(),
                from.port
            )
        })?;
        match &connection.to {
            SavedDestination::DeviceOutput(channel) => Ok(output.to_device(*channel)),
            SavedDestination::Input(to) => {
                let input = binding(to)?.inputs.get(&to.port).ok_or_else(|| {
                    format!(
                        "instance {:?} has no input {:?}",
                        to.instance.as_str(),
                        to.port
                    )
                })?;
                Ok(output.to(*input))
            }
        }
    }
}
