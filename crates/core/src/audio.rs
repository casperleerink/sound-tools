use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventData {
    Parameter { id: u32, value: f32 },
    Custom { kind: u32, data: [f32; 4] },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    pub frame: usize,
    pub data: EventData,
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessContext {
    pub sample_rate: f64,
    pub engine_frame: u64,
    pub project_frame: u64,
    pub playing: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Prepare {
    pub sample_rate: f64,
    pub max_frames: usize,
}

impl Prepare {
    pub fn validate(self) -> Result<Self> {
        if !self.sample_rate.is_finite() || self.sample_rate < 1.0 || self.max_frames == 0 {
            return Err(Error("Invalid audio preparation settings".into()));
        }
        Ok(self)
    }
}

pub struct AudioBuffer {
    data: Vec<f32>,
    channels: usize,
    capacity: usize,
    frames: usize,
}

impl AudioBuffer {
    pub fn new(channels: usize, capacity: usize) -> Self {
        Self {
            data: vec![0.0; channels * capacity],
            channels,
            capacity,
            frames: capacity,
        }
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    pub fn frames(&self) -> usize {
        self.frames
    }
    pub fn channel(&self, channel: usize) -> &[f32] {
        &self.data[channel * self.capacity..channel * self.capacity + self.frames]
    }
    pub fn channel_mut(&mut self, channel: usize) -> &mut [f32] {
        &mut self.data[channel * self.capacity..channel * self.capacity + self.frames]
    }
    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }
    pub fn set_frames(&mut self, frames: usize) -> Result<()> {
        if frames > self.capacity {
            return Err(Error("Block exceeds prepared capacity".into()));
        }
        self.frames = frames;
        Ok(())
    }
}

pub trait Processor: Send + 'static {
    fn input_channels(&self) -> usize;
    fn output_channels(&self) -> usize;
    fn prepare(&mut self, settings: Prepare) -> Result<()>;
    fn process(
        &mut self,
        context: ProcessContext,
        input: &AudioBuffer,
        output: &mut AudioBuffer,
        events: &[Event],
    );
    fn reset(&mut self);
    fn latency_frames(&self) -> usize {
        0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId(pub usize);

struct Node {
    processor: Box<dyn Processor>,
    inputs: AudioBuffer,
    outputs: AudioBuffer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connection {
    pub source: NodeId,
    pub source_channel: usize,
    pub target: NodeId,
    pub target_channel: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct NodeEvent {
    pub node: NodeId,
    pub event: Event,
}

pub struct GraphBuilder {
    settings: Prepare,
    processors: Vec<Box<dyn Processor>>,
    connections: Vec<Connection>,
}

impl GraphBuilder {
    pub fn new(settings: Prepare) -> Result<Self> {
        Ok(Self {
            settings: settings.validate()?,
            processors: Vec::new(),
            connections: Vec::new(),
        })
    }
    pub fn add(&mut self, processor: impl Processor) -> NodeId {
        self.add_boxed(Box::new(processor))
    }
    pub fn add_boxed(&mut self, processor: Box<dyn Processor>) -> NodeId {
        let id = NodeId(self.processors.len());
        self.processors.push(processor);
        id
    }
    pub fn connect(&mut self, connection: Connection) -> Result<()> {
        let source = self
            .processors
            .get(connection.source.0)
            .ok_or_else(|| Error("Missing source processor".into()))?;
        let target = self
            .processors
            .get(connection.target.0)
            .ok_or_else(|| Error("Missing target processor".into()))?;
        if connection.source_channel >= source.output_channels()
            || connection.target_channel >= target.input_channels()
        {
            return Err(Error("Connection channel is out of range".into()));
        }
        if self.connections.contains(&connection) {
            return Err(Error("Duplicate connection".into()));
        }
        self.connections.push(connection);
        Ok(())
    }
    pub fn connect_stereo(&mut self, source: NodeId, target: NodeId) -> Result<()> {
        for channel in 0..2 {
            self.connect(Connection {
                source,
                source_channel: channel,
                target,
                target_channel: channel,
            })?;
        }
        Ok(())
    }
    pub fn build(self, output: NodeId) -> Result<Graph> {
        if output.0 >= self.processors.len() {
            return Err(Error("Missing graph output".into()));
        }
        let count = self.processors.len();
        let mut indegree = vec![0usize; count];
        for edge in &self.connections {
            indegree[edge.target.0] += 1;
        }
        let mut order = Vec::with_capacity(count);
        let mut ready: Vec<_> = indegree
            .iter()
            .enumerate()
            .filter_map(|(id, &degree)| (degree == 0).then_some(id))
            .collect();
        while let Some(id) = ready.pop() {
            order.push(id);
            for edge in self.connections.iter().filter(|edge| edge.source.0 == id) {
                indegree[edge.target.0] -= 1;
                if indegree[edge.target.0] == 0 {
                    ready.push(edge.target.0);
                }
            }
        }
        if order.len() != count {
            return Err(Error("Feedback requires an explicit delayed execution path; cyclic graphs are not supported yet".into()));
        }
        let mut nodes = Vec::with_capacity(count);
        for mut processor in self.processors {
            processor.prepare(self.settings)?;
            nodes.push(Node {
                inputs: AudioBuffer::new(processor.input_channels(), self.settings.max_frames),
                outputs: AudioBuffer::new(processor.output_channels(), self.settings.max_frames),
                processor,
            });
        }
        Ok(Graph {
            settings: self.settings,
            nodes,
            connections: self.connections,
            order,
            output,
            event_scratch: Vec::with_capacity(4096),
        })
    }
}

pub struct Graph {
    settings: Prepare,
    nodes: Vec<Node>,
    connections: Vec<Connection>,
    order: Vec<usize>,
    output: NodeId,
    event_scratch: Vec<Event>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessError {
    BlockTooLarge,
    TooManyEvents,
    InvalidEvent,
}

impl Graph {
    pub fn process(
        &mut self,
        context: ProcessContext,
        frames: usize,
        events: &[NodeEvent],
    ) -> std::result::Result<&AudioBuffer, ProcessError> {
        if frames > self.settings.max_frames {
            return Err(ProcessError::BlockTooLarge);
        }
        if events.len() > self.event_scratch.capacity() {
            return Err(ProcessError::TooManyEvents);
        }
        if events
            .iter()
            .any(|event| event.node.0 >= self.nodes.len() || event.event.frame >= frames)
        {
            return Err(ProcessError::InvalidEvent);
        }
        for node in &mut self.nodes {
            node.inputs.frames = frames;
            node.outputs.frames = frames;
            node.inputs.clear();
            node.outputs.clear();
        }
        for &id in &self.order {
            self.event_scratch.clear();
            self.event_scratch.extend(
                events
                    .iter()
                    .filter(|event| event.node.0 == id)
                    .map(|event| event.event),
            );
            self.event_scratch.sort_unstable_by_key(|event| event.frame);
            let node = &mut self.nodes[id];
            node.processor.process(
                context,
                &node.inputs,
                &mut node.outputs,
                &self.event_scratch,
            );
            for edge in self.connections.iter().filter(|edge| edge.source.0 == id) {
                let (source, target) = if id < edge.target.0 {
                    let (left, right) = self.nodes.split_at_mut(edge.target.0);
                    (&left[id], &mut right[0])
                } else {
                    let (left, right) = self.nodes.split_at_mut(id);
                    (&right[0], &mut left[edge.target.0])
                };
                for (destination, sample) in target
                    .inputs
                    .channel_mut(edge.target_channel)
                    .iter_mut()
                    .zip(source.outputs.channel(edge.source_channel))
                {
                    *destination += sample;
                }
            }
        }
        Ok(&self.nodes[self.output.0].outputs)
    }
    pub fn reset(&mut self) {
        for node in &mut self.nodes {
            node.processor.reset();
        }
    }
    pub fn settings(&self) -> Prepare {
        self.settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Gain(f32);
    impl Processor for Gain {
        fn input_channels(&self) -> usize {
            2
        }
        fn output_channels(&self) -> usize {
            2
        }
        fn prepare(&mut self, _: Prepare) -> Result<()> {
            Ok(())
        }
        fn process(
            &mut self,
            _: ProcessContext,
            input: &AudioBuffer,
            output: &mut AudioBuffer,
            _: &[Event],
        ) {
            for channel in 0..2 {
                for (out, &sample) in output
                    .channel_mut(channel)
                    .iter_mut()
                    .zip(input.channel(channel))
                {
                    *out = sample + self.0;
                }
            }
        }
        fn reset(&mut self) {}
    }
    fn settings() -> Prepare {
        Prepare {
            sample_rate: 48000.0,
            max_frames: 128,
        }
    }
    #[test]
    fn sums_parallel_paths_and_handles_short_blocks() {
        let mut builder = GraphBuilder::new(settings()).unwrap();
        let master = builder.add(Gain(0.0));
        let first = builder.add(Gain(0.25));
        let second = builder.add(Gain(0.5));
        builder.connect_stereo(first, master).unwrap();
        builder.connect_stereo(second, master).unwrap();
        let mut graph = builder.build(master).unwrap();
        let context = ProcessContext {
            sample_rate: 48000.0,
            engine_frame: 0,
            project_frame: 0,
            playing: true,
        };
        for frames in [128, 17, 1, 128] {
            let output = graph.process(context, frames, &[]).unwrap();
            assert_eq!(output.channel(0), vec![0.75; frames]);
            assert_eq!(output.channel(1), vec![0.75; frames]);
        }
        assert!(matches!(
            graph.process(context, 129, &[]),
            Err(ProcessError::BlockTooLarge)
        ));
    }
    #[test]
    fn rejects_cycles_and_invalid_channels() {
        let mut builder = GraphBuilder::new(settings()).unwrap();
        let a = builder.add(Gain(0.0));
        let b = builder.add(Gain(0.0));
        assert!(
            builder
                .connect(Connection {
                    source: a,
                    source_channel: 2,
                    target: b,
                    target_channel: 0
                })
                .is_err()
        );
        builder.connect_stereo(a, b).unwrap();
        builder.connect_stereo(b, a).unwrap();
        assert!(builder.build(b).is_err());
    }
}
