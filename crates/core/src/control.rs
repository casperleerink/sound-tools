//! The control side of the engine: owns the graph, turns edits into batches and takes back
//! everything the audio thread is done with.

use std::collections::VecDeque;
use std::marker::PhantomData;

use crate::engine::{Batch, Command, Engine, EngineStatus};
use crate::graph::{Connection, Graph, GraphError, NodeId};
use crate::processor::{MAX_BLOCK, PrepareConfig, Processor};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub sample_rate: u32,
    /// Device output channels.
    pub channels: usize,
    /// Edits that fit in each ring. More edits wait on the control side.
    pub ring_capacity: usize,
    /// Events one event port can hold per block.
    pub event_capacity: usize,
    /// Processor slots to start with. The table grows when it fills up.
    pub processor_slots: usize,
}

impl EngineConfig {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            sample_rate,
            channels,
            ring_capacity: 64,
            event_capacity: 256,
            processor_slots: 256,
        }
    }
}

impl Engine {
    /// Creates both halves of an engine. Keep the control on a normal thread. Give the engine
    /// to `OutputDevice::start`, or call `process_block` yourself to render offline.
    pub fn new(config: EngineConfig) -> (EngineControl, Engine) {
        let (command_producer, command_consumer) = rtrb::RingBuffer::new(config.ring_capacity);
        let (return_producer, return_consumer) = rtrb::RingBuffer::new(config.ring_capacity);
        let (status_writer, status_reader) = triple_buffer::triple_buffer(&EngineStatus::default());
        let graph = Graph::with_slots(config.processor_slots);
        let engine = Engine::from_parts(
            config.channels,
            config.processor_slots,
            Default::default(),
            command_consumer,
            return_producer,
            status_writer,
        );
        let control = EngineControl {
            config,
            graph,
            pending: VecDeque::new(),
            commands: command_producer,
            returns: return_consumer,
            status: status_reader,
            command_ring_full: 0,
        };
        (control, engine)
    }
}

/// Handle to a processor of type `P` in the graph. It types the updates sent to it.
pub struct Node<P> {
    id: NodeId,
    processor: PhantomData<fn(P)>,
}

impl<P> Node<P> {
    pub fn id(&self) -> NodeId {
        self.id
    }
}

impl<P> Clone for Node<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for Node<P> {}

pub struct EngineControl {
    config: EngineConfig,
    graph: Graph,
    /// Batches the command ring had no room for yet, oldest first.
    pending: VecDeque<Batch>,
    commands: rtrb::Producer<Batch>,
    returns: rtrb::Consumer<Batch>,
    status: triple_buffer::Output<EngineStatus>,
    command_ring_full: u64,
}

impl EngineControl {
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Starts an edit. Nothing reaches the audio thread until `commit` succeeds. Dropping the
    /// edit leaves the engine unchanged.
    pub fn edit(&mut self) -> Edit<'_> {
        Edit {
            graph: None,
            commands: Vec::new(),
            control: self,
        }
    }

    /// Sends one update. It applies at the start of the next block.
    pub fn update<P: Processor>(
        &mut self,
        node: Node<P>,
        update: P::Update,
    ) -> Result<(), GraphError> {
        let mut edit = self.edit();
        edit.update(node, update)?;
        edit.commit()
    }

    /// Call this regularly. It sends edits that were waiting for ring space, drops everything
    /// the audio thread returned, and gives the latest status.
    pub fn poll(&mut self) -> EngineStatus {
        while let Ok(returned) = self.returns.pop() {
            drop(returned);
        }
        self.flush();
        *self.status.read()
    }

    /// Edits still waiting on the control side for ring space.
    pub fn pending_edits(&self) -> usize {
        self.pending.len()
    }

    /// Times an edit had to wait because the command ring was full. No edit is lost.
    pub fn command_ring_full(&self) -> u64 {
        self.command_ring_full
    }

    fn flush(&mut self) {
        while let Some(batch) = self.pending.pop_front() {
            if let Err(rtrb::PushError::Full(batch)) = self.commands.push(batch) {
                self.pending.push_front(batch);
                self.command_ring_full += 1;
                return;
            }
        }
    }
}

/// A group of changes that reaches the audio thread whole, in one block, or not at all.
pub struct Edit<'a> {
    control: &'a mut EngineControl,
    /// A changed copy of the graph. `None` until the edit changes processors or connections,
    /// so plain updates cost no copy and no compile.
    graph: Option<Graph>,
    commands: Batch,
}

impl Edit<'_> {
    fn graph_mut(&mut self) -> &mut Graph {
        self.graph.get_or_insert_with(|| self.control.graph.clone())
    }

    /// Prepares the processor and adds it under `name`. The name must be unique and stable,
    /// for example the instance path. It decides the run order of independent processors.
    pub fn add_processor<P: Processor>(
        &mut self,
        name: &str,
        mut processor: P,
    ) -> Result<Node<P>, GraphError> {
        let (id, slot, grown) = self.graph_mut().add_node(name, processor.ports())?;
        processor.prepare(&PrepareConfig {
            sample_rate: self.control.config.sample_rate,
            max_block_frames: MAX_BLOCK,
        });
        if let Some(slot_count) = grown {
            let table = std::iter::repeat_with(|| None).take(slot_count).collect();
            self.commands.push(Command::GrowSlots(table));
        }
        self.commands.push(Command::SetSlot {
            slot,
            processor: Some(Box::new(processor)),
        });
        Ok(Node {
            id,
            processor: PhantomData,
        })
    }

    /// Removes the processor and its connections. The processor itself is dropped later on the
    /// control thread, in `EngineControl::poll`.
    pub fn remove_processor(&mut self, node: NodeId) -> Result<(), GraphError> {
        let slot = self.graph_mut().remove_node(node)?;
        self.commands.push(Command::SetSlot {
            slot,
            processor: None,
        });
        Ok(())
    }

    pub fn connect(&mut self, connection: Connection) -> Result<(), GraphError> {
        let channels = self.control.config.channels;
        self.graph_mut().connect(connection, channels)
    }

    pub fn disconnect(&mut self, connection: &Connection) -> Result<(), GraphError> {
        self.graph_mut().disconnect(connection)
    }

    pub fn update<P: Processor>(
        &mut self,
        node: Node<P>,
        update: P::Update,
    ) -> Result<(), GraphError> {
        let graph = self.graph.as_ref().unwrap_or(&self.control.graph);
        self.commands.push(Command::Update {
            slot: graph.slot(node.id)?,
            update: Box::new(update),
        });
        Ok(())
    }

    /// Validates and compiles the whole edit, then queues it for the audio thread. On error
    /// nothing is sent and the graph stays as it was.
    pub fn commit(mut self) -> Result<(), GraphError> {
        if let Some(graph) = self.graph {
            let config = &self.control.config;
            let schedule = graph.compile(config.channels, config.event_capacity)?;
            self.commands.push(Command::SetSchedule(Box::new(schedule)));
            self.control.graph = graph;
        }
        if !self.commands.is_empty() {
            self.control.pending.push_back(self.commands);
            self.control.flush();
        }
        Ok(())
    }
}
