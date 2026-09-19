//! The typed graph the control side edits, and its compile step to a flat schedule.
//!
//! Compiling happens on the control thread and may allocate. The audio thread only walks the
//! result.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::processor::{
    AudioBuffer, ErasedEventBuffer, EventType, InputPort, MAX_BLOCK, OutputPort, Ports,
};

/// Identifies a processor in the graph. Never reused within one engine.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub(crate) u64);

/// Where a connection ends.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Destination {
    Node(NodeId, InputPort),
    /// A channel of the device output. Only audio connects here.
    DeviceOutput(usize),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Connection {
    pub source: NodeId,
    pub output: OutputPort,
    pub destination: Destination,
}

impl Connection {
    pub fn new(
        source: NodeId,
        output: impl Into<OutputPort>,
        destination: NodeId,
        input: impl Into<InputPort>,
    ) -> Self {
        Self {
            source,
            output: output.into(),
            destination: Destination::Node(destination, input.into()),
        }
    }

    pub fn to_device(source: NodeId, output: impl Into<OutputPort>, channel: usize) -> Self {
        Self {
            source,
            output: output.into(),
            destination: Destination::DeviceOutput(channel),
        }
    }
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GraphError {
    #[error("no processor with id {0:?}")]
    UnknownNode(NodeId),
    #[error("a processor named {0:?} already exists")]
    DuplicateName(String),
    #[error("processor {0:?} declares port handles out of index order")]
    PortsOutOfOrder(String),
    #[error("processor {node:?} has no output {port:?}")]
    UnknownOutput { node: String, port: OutputPort },
    #[error("processor {node:?} has no input {port:?}")]
    UnknownInput { node: String, port: InputPort },
    #[error("the device has no output channel {0}")]
    UnknownDeviceChannel(usize),
    #[error("{description}: the two ports carry different types")]
    PortTypeMismatch {
        connection: Connection,
        description: String,
    },
    #[error("{description}: this connection closes a cycle")]
    Cycle {
        connection: Connection,
        description: String,
    },
    #[error("the connection does not exist")]
    UnknownConnection(Connection),
}

#[derive(Clone)]
struct GraphNode {
    /// Stable key chosen by the caller. It orders independent branches, so the same project
    /// compiles to the same schedule whatever order its processors were created in.
    name: String,
    slot: usize,
    ports: Ports,
}

/// What a port carries, for connection checks.
#[derive(PartialEq)]
enum Carries {
    Audio,
    Events(EventType),
}

/// Processors, their ports and the connections between them. Also hands out processor slots.
#[derive(Clone, Default)]
pub(crate) struct Graph {
    nodes: BTreeMap<NodeId, GraphNode>,
    connections: BTreeSet<Connection>,
    next_node: u64,
    slot_count: usize,
    free_slots: Vec<usize>,
}

impl Graph {
    pub fn with_slots(slot_count: usize) -> Self {
        Self {
            slot_count,
            free_slots: (0..slot_count).rev().collect(),
            ..Self::default()
        }
    }

    /// Returns the new node, its slot, and the slot table size the audio side needs when the
    /// current table is too small.
    pub fn add_node(
        &mut self,
        name: &str,
        ports: Ports,
    ) -> Result<(NodeId, usize, Option<usize>), GraphError> {
        if self.nodes.values().any(|node| node.name == name) {
            return Err(GraphError::DuplicateName(name.to_string()));
        }
        if ports.out_of_order {
            return Err(GraphError::PortsOutOfOrder(name.to_string()));
        }
        let mut grown = None;
        if self.free_slots.is_empty() {
            let new_count = (self.slot_count * 2).max(1);
            self.free_slots.extend((self.slot_count..new_count).rev());
            self.slot_count = new_count;
            grown = Some(new_count);
        }
        let slot = self.free_slots.pop().unwrap_or_default();
        let id = NodeId(self.next_node);
        self.next_node += 1;
        let name = name.to_string();
        self.nodes.insert(id, GraphNode { name, slot, ports });
        Ok((id, slot, grown))
    }

    /// Removes the node and every connection that touches it. Returns its slot.
    pub fn remove_node(&mut self, id: NodeId) -> Result<usize, GraphError> {
        let node = self.nodes.remove(&id).ok_or(GraphError::UnknownNode(id))?;
        self.connections.retain(|connection| {
            connection.source != id
                && !matches!(connection.destination, Destination::Node(node, _) if node == id)
        });
        self.free_slots.push(node.slot);
        Ok(node.slot)
    }

    pub fn slot(&self, id: NodeId) -> Result<usize, GraphError> {
        Ok(self.node(id)?.slot)
    }

    pub fn connect(&mut self, connection: Connection, channels: usize) -> Result<(), GraphError> {
        let source = self.node(connection.source)?;
        let unknown_output = || GraphError::UnknownOutput {
            node: source.name.clone(),
            port: connection.output,
        };
        let output = match connection.output {
            OutputPort::Audio(index) if index < source.ports.audio_outputs => Carries::Audio,
            OutputPort::Events(index) => Carries::Events(
                *source
                    .ports
                    .event_outputs
                    .get(index)
                    .ok_or_else(unknown_output)?,
            ),
            OutputPort::Audio(_) => return Err(unknown_output()),
        };
        let input = match connection.destination {
            Destination::DeviceOutput(channel) if channel < channels => Carries::Audio,
            Destination::DeviceOutput(channel) => {
                return Err(GraphError::UnknownDeviceChannel(channel));
            }
            Destination::Node(id, port) => {
                let destination = self.node(id)?;
                let unknown_input = || GraphError::UnknownInput {
                    node: destination.name.clone(),
                    port,
                };
                match port {
                    InputPort::Audio(index) if index < destination.ports.audio_inputs => {
                        Carries::Audio
                    }
                    InputPort::Events(index) => Carries::Events(
                        *destination
                            .ports
                            .event_inputs
                            .get(index)
                            .ok_or_else(unknown_input)?,
                    ),
                    InputPort::Audio(_) => return Err(unknown_input()),
                }
            }
        };
        if output != input {
            return Err(GraphError::PortTypeMismatch {
                connection,
                description: self.describe(&connection),
            });
        }
        self.connections.insert(connection);
        Ok(())
    }

    pub fn disconnect(&mut self, connection: &Connection) -> Result<(), GraphError> {
        if self.connections.remove(connection) {
            Ok(())
        } else {
            Err(GraphError::UnknownConnection(*connection))
        }
    }

    fn node(&self, id: NodeId) -> Result<&GraphNode, GraphError> {
        self.nodes.get(&id).ok_or(GraphError::UnknownNode(id))
    }

    fn name(&self, id: NodeId) -> &str {
        self.nodes.get(&id).map_or("?", |node| node.name.as_str())
    }

    fn describe(&self, connection: &Connection) -> String {
        let source = self.name(connection.source);
        let output = connection.output;
        match connection.destination {
            Destination::Node(id, input) => {
                format!("{source} {output:?} -> {} {input:?}", self.name(id))
            }
            Destination::DeviceOutput(channel) => {
                format!("{source} {output:?} -> device output {channel}")
            }
        }
    }

    /// Connections between two processors, as (source, destination, connection).
    fn node_connections(&self) -> impl Iterator<Item = (NodeId, NodeId, &Connection)> {
        self.connections
            .iter()
            .filter_map(|connection| match connection.destination {
                Destination::Node(destination, _) => {
                    Some((connection.source, destination, connection))
                }
                Destination::DeviceOutput(_) => None,
            })
    }

    /// Kahn's algorithm. Ready processors run in name order, so the result is reproducible.
    fn sorted(&self) -> Result<Vec<NodeId>, GraphError> {
        let mut blocked: BTreeMap<NodeId, BTreeMap<NodeId, Connection>> = BTreeMap::new();
        let mut destinations_of: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        for (source, destination, connection) in self.node_connections() {
            blocked
                .entry(destination)
                .or_default()
                .entry(source)
                .or_insert(*connection);
            destinations_of
                .entry(source)
                .or_default()
                .insert(destination);
        }
        let mut ready: BTreeSet<(&str, NodeId)> = self
            .nodes
            .iter()
            .filter(|(id, _)| !blocked.contains_key(id))
            .map(|(id, node)| (node.name.as_str(), *id))
            .collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some((_, id)) = ready.pop_first() {
            order.push(id);
            for destination in destinations_of.get(&id).into_iter().flatten() {
                if let Some(sources) = blocked.get_mut(destination) {
                    sources.remove(&id);
                    if sources.is_empty() {
                        blocked.remove(destination);
                        ready.insert((self.name(*destination), *destination));
                    }
                }
            }
        }
        match blocked.keys().next() {
            None => Ok(order),
            Some(start) => Err(self.cycle_error(&blocked, *start)),
        }
    }

    /// Every processor left in `blocked` has a source that is also left. Walking back along
    /// first sources must reach some processor twice, and that processor is on a cycle.
    fn cycle_error(
        &self,
        blocked: &BTreeMap<NodeId, BTreeMap<NodeId, Connection>>,
        start: NodeId,
    ) -> GraphError {
        let mut visited = BTreeSet::new();
        let mut destination = start;
        loop {
            let Some((source, connection)) = blocked
                .get(&destination)
                .and_then(|sources| sources.first_key_value())
            else {
                return GraphError::UnknownNode(destination);
            };
            if !visited.insert(destination) {
                return GraphError::Cycle {
                    connection: *connection,
                    description: self.describe(connection),
                };
            }
            destination = *source;
        }
    }

    /// Validates the whole graph and builds the schedule with all its buffers.
    pub fn compile(&self, channels: usize, event_capacity: usize) -> Result<Schedule, GraphError> {
        let order = self.sorted()?;
        let mut schedule = Schedule {
            device_sources: vec![Vec::new(); channels],
            ..Schedule::default()
        };
        let mut step_of = BTreeMap::new();
        for id in order {
            let node = self.node(id)?;
            // Every output port gets its own buffer. All connections from that port read the
            // same buffer, which is the fan-out sharing.
            let audio_start = schedule.audio_outputs.len();
            let audio_end = audio_start + node.ports.audio_outputs;
            schedule.audio_outputs.resize(audio_end, [0.0; MAX_BLOCK]);
            let event_outputs_start = schedule.event_outputs.len();
            for event_type in &node.ports.event_outputs {
                schedule
                    .event_outputs
                    .push((event_type.new_buffer)(event_capacity));
            }
            let event_inputs_start = schedule.event_inputs.len();
            for event_type in &node.ports.event_inputs {
                schedule
                    .event_inputs
                    .push((event_type.new_buffer)(event_capacity));
            }
            step_of.insert(id, schedule.steps.len());
            schedule.steps.push(Step {
                slot: node.slot,
                audio_sources: vec![Vec::new(); node.ports.audio_inputs],
                audio_outputs: audio_start..audio_end,
                event_sources: vec![Vec::new(); node.ports.event_inputs.len()],
                event_inputs: event_inputs_start..schedule.event_inputs.len(),
                event_outputs: event_outputs_start..schedule.event_outputs.len(),
            });
        }

        for connection in &self.connections {
            let step = |id| step_of.get(id).and_then(|index| schedule.steps.get(*index));
            let Some(source) = step(&connection.source) else {
                return Err(GraphError::UnknownNode(connection.source));
            };
            let buffer = match connection.output {
                OutputPort::Audio(index) => source.audio_outputs.start + index,
                OutputPort::Events(index) => source.event_outputs.start + index,
            };
            let sources = match connection.destination {
                Destination::DeviceOutput(channel) => schedule.device_sources.get_mut(channel),
                Destination::Node(id, input) => step_of
                    .get(&id)
                    .and_then(|index| schedule.steps.get_mut(*index))
                    .and_then(|step| match input {
                        InputPort::Audio(index) => step.audio_sources.get_mut(index),
                        InputPort::Events(index) => step.event_sources.get_mut(index),
                    }),
            };
            // `connect` checked both ends, and removing a processor removes its connections.
            if let Some(sources) = sources {
                sources.push(buffer);
            }
        }

        // Buffer numbers follow the schedule order, so sorting fixes the order of sums and
        // merges whatever order the connections were made in.
        let widest = schedule
            .steps
            .iter()
            .map(|step| step.audio_sources.len())
            .max();
        schedule
            .audio_scratch
            .resize(widest.unwrap_or_default(), [0.0; MAX_BLOCK]);
        for step in &mut schedule.steps {
            step.audio_sources
                .iter_mut()
                .for_each(|sources| sources.sort_unstable());
            step.event_sources
                .iter_mut()
                .for_each(|sources| sources.sort_unstable());
        }
        schedule
            .device_sources
            .iter_mut()
            .for_each(|sources| sources.sort_unstable());
        Ok(schedule)
    }
}

/// One processor call. Buffer numbers index into the owning [`Schedule`]. The source lists
/// also tell which earlier steps this step depends on, for a later parallel executor.
pub(crate) struct Step {
    pub slot: usize,
    /// Per audio input port: the output buffers summed into it.
    pub audio_sources: Vec<Vec<usize>>,
    pub audio_outputs: Range<usize>,
    /// Per event input port: the output buffers merged into it.
    pub event_sources: Vec<Vec<usize>>,
    pub event_inputs: Range<usize>,
    pub event_outputs: Range<usize>,
}

/// The flat list the audio thread walks, with every buffer it needs.
#[derive(Default)]
pub(crate) struct Schedule {
    pub steps: Vec<Step>,
    pub audio_outputs: Vec<AudioBuffer>,
    /// Summed audio inputs of the step being run. Sized for the widest processor.
    pub audio_scratch: Vec<AudioBuffer>,
    pub event_outputs: Vec<Box<dyn ErasedEventBuffer>>,
    pub event_inputs: Vec<Box<dyn ErasedEventBuffer>>,
    /// Per device channel: the output buffers summed into it.
    pub device_sources: Vec<Vec<usize>>,
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::processor::{AudioInput, AudioOutput};

    const PORTS_PER_NODE: usize = 2;

    fn ports() -> Ports {
        Ports::new()
            .audio_input(AudioInput::new(0))
            .audio_input(AudioInput::new(1))
            .audio_output(AudioOutput::new(0))
            .audio_output(AudioOutput::new(1))
    }

    /// Builds a graph of nodes named after their number, created in the given order.
    fn graph(creation_order: &[usize], edges: &[(usize, usize, usize, usize)]) -> Graph {
        let mut graph = Graph::with_slots(1);
        let mut ids = BTreeMap::new();
        for number in creation_order {
            let (id, _, _) = graph
                .add_node(&format!("node-{number:02}"), ports())
                .unwrap();
            ids.insert(*number, id);
        }
        for (source, output, destination, input) in edges {
            let connection = Connection::new(
                ids[source],
                OutputPort::Audio(*output),
                ids[destination],
                InputPort::Audio(*input),
            );
            graph.connect(connection, 0).unwrap();
        }
        graph
    }

    fn reaches(graph: &Graph, from: NodeId, to: NodeId) -> bool {
        let mut visited = BTreeSet::new();
        let mut pending = vec![from];
        while let Some(node) = pending.pop() {
            if node == to {
                return true;
            }
            if visited.insert(node) {
                pending.extend(
                    graph
                        .node_connections()
                        .filter(|(source, _, _)| *source == node)
                        .map(|(_, destination, _)| destination),
                );
            }
        }
        false
    }

    fn names_in_run_order(graph: &Graph, schedule: &Schedule) -> Vec<String> {
        schedule
            .steps
            .iter()
            .map(|step| {
                let node = graph.nodes.values().find(|node| node.slot == step.slot);
                node.unwrap().name.clone()
            })
            .collect()
    }

    proptest! {
        #[test]
        fn random_graphs_compile_to_valid_schedules_or_name_a_cycle(
            node_count in 1_usize..9,
            raw_edges in prop::collection::vec((0_usize..9, 0..PORTS_PER_NODE, 0_usize..9, 0..PORTS_PER_NODE), 0..20),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .map(|(source, output, destination, input)| {
                    (source % node_count, output, destination % node_count, input)
                })
                .collect();
            let forward: Vec<usize> = (0..node_count).collect();
            let backward: Vec<usize> = (0..node_count).rev().collect();
            let built = graph(&forward, &edges);
            let cyclic = built
                .node_connections()
                .any(|(source, destination, _)| reaches(&built, destination, source));

            match built.compile(0, 4) {
                Err(GraphError::Cycle { connection, .. }) => {
                    let Destination::Node(destination, _) = connection.destination else {
                        panic!("a device connection cannot close a cycle");
                    };
                    prop_assert!(built.connections.contains(&connection));
                    prop_assert!(reaches(&built, destination, connection.source));
                }
                Err(other) => panic!("unexpected error {other}"),
                Ok(schedule) => {
                    prop_assert!(!cyclic);
                    prop_assert_eq!(schedule.steps.len(), node_count);
                    prop_assert_eq!(schedule.audio_outputs.len(), node_count * PORTS_PER_NODE);
                    let step_of_slot: BTreeMap<usize, usize> = schedule
                        .steps
                        .iter()
                        .enumerate()
                        .map(|(index, step)| (step.slot, index))
                        .collect();
                    prop_assert_eq!(step_of_slot.len(), node_count);
                    for (source, destination, connection) in built.node_connections() {
                        let source_step = &schedule.steps[step_of_slot[&built.slot(source).unwrap()]];
                        let destination_index = step_of_slot[&built.slot(destination).unwrap()];
                        let destination_step = &schedule.steps[destination_index];
                        let (OutputPort::Audio(output), Destination::Node(_, InputPort::Audio(input))) =
                            (connection.output, connection.destination)
                        else {
                            panic!("only audio ports are generated");
                        };
                        let buffer = source_step.audio_outputs.start + output;
                        prop_assert!(source_step.audio_outputs.contains(&buffer));
                        prop_assert!(destination_step.audio_sources[input].contains(&buffer));
                        // A source runs before everything that reads it.
                        prop_assert!(source_step.audio_outputs.end <= destination_step.audio_outputs.start);
                    }

                    // The same project built in another order runs in the same order.
                    let rebuilt = graph(&backward, &edges);
                    let rebuilt_schedule = rebuilt.compile(0, 4).unwrap();
                    prop_assert_eq!(
                        names_in_run_order(&built, &schedule),
                        names_in_run_order(&rebuilt, &rebuilt_schedule)
                    );
                }
            }
            prop_assert_eq!(built.compile(0, 4).is_err(), cyclic);
        }
    }
}
