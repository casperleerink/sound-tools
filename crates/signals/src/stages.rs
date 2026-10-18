//! A sound that plays through built-in devices, cut into stages around them.
//!
//! A device is a processor of the engine, which hears a whole block at once. So the code before
//! a device and the code after it are two processors, stages, with the device between: the
//! engine runs them in order, and compensates the latency of a device as it does on a track. A
//! stage takes what it reads from the stages before through sound ports, so the first stage,
//! the one that hears the record, the controls and the notes, hands them on.
//!
//! A feedback or a buffer lives in one stage, so neither may loop through a device.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::code::Code;
use crate::graph::{
    Declarations, Graph, GraphError, Import, NamedNode, Node, NotesNode, OutputNode, Played,
    TableNode,
};

/// The most devices one sound plays through.
const MAX_DEVICES: usize = 16;

/// A compiled sound: the code of each stage, the devices between them and how they connect.
#[derive(Clone, Debug)]
pub struct Program {
    /// The first hears the record, the controls and the notes.
    pub stages: Vec<Code>,
    pub devices: Vec<DeviceUse>,
    pub links: Vec<Link>,
    /// Where the sound of the tool comes from.
    pub sound: Sound,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sound {
    Stage(usize),
    Device(usize),
}

/// A built-in device the sound plays through.
#[derive(Clone, Debug)]
pub struct DeviceUse {
    pub tool: String,
    /// Its record, but the numbers signals move.
    pub state: Map<String, Value>,
    /// The numbers signals move, each by its field and where it comes from.
    pub numbers: Vec<DeviceNumber>,
}

/// A number of a device that a signal moves: the send at `index` of the stage `stage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceNumber {
    pub field: String,
    pub stage: usize,
    pub index: usize,
}

/// A sound connection between two stages, a stage and a device, or two devices.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub from: LinkFrom,
    pub to: LinkTo,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinkFrom {
    Export { stage: usize, port: usize },
    Device(usize),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkTo {
    Import { stage: usize, port: usize },
    Device(usize),
}

fn error<T>(message: impl Into<String>) -> Result<T, GraphError> {
    Err(GraphError(message.into()))
}

impl Graph {
    /// The program of this sound: one stage, as [`Graph::compile`] makes it, when it plays
    /// through no device.
    pub fn program(&self, declarations: &Declarations) -> Result<Program, GraphError> {
        if !self
            .nodes
            .iter()
            .any(|node| matches!(node, Node::Device(_)))
        {
            return Ok(Program {
                stages: vec![self.compile(declarations)?],
                devices: Vec::new(),
                links: Vec::new(),
                sound: Sound::Stage(0),
            });
        }
        Split::new(self)?.run(declarations)
    }
}

/// Reads no memory and nothing a stage hands on: any stage makes its own.
fn free(node: &Node) -> bool {
    matches!(
        node,
        Node::Constant(_)
            | Node::Channel
            | Node::SampleRate
            | Node::Beat
            | Node::Bpm
            | Node::Playing
    )
}

/// The nodes `node` reads, but the feedback it reads, as places to change.
fn operands(node: &mut Node) -> Vec<&mut usize> {
    match node {
        Node::Negate(one)
        | Node::Sin(one)
        | Node::Cos(one)
        | Node::Tan(one)
        | Node::Tanh(one)
        | Node::Abs(one)
        | Node::Sqrt(one)
        | Node::Exp(one)
        | Node::Log(one)
        | Node::Floor(one)
        | Node::Wrap(one)
        | Node::Db(one)
        | Node::Saturate(one)
        | Node::Rise(one)
        | Node::Change(one) => vec![&mut one.x],
        Node::Add(two)
        | Node::Subtract(two)
        | Node::Multiply(two)
        | Node::Divide(two)
        | Node::Remainder(two)
        | Node::Less(two)
        | Node::Greater(two)
        | Node::LessOrEqual(two)
        | Node::GreaterOrEqual(two)
        | Node::Equal(two)
        | Node::NotEqual(two)
        | Node::Min(two)
        | Node::Max(two)
        | Node::Pow(two) => vec![&mut two.a, &mut two.b],
        Node::Clamp(clamp) => vec![&mut clamp.x, &mut clamp.low, &mut clamp.high],
        Node::Mix(mix) => vec![&mut mix.a, &mut mix.b, &mut mix.amount],
        Node::Phasor(phasor) => vec![&mut phasor.hz],
        Node::Delay(delay) => [&mut delay.x, &mut delay.ms]
            .into_iter()
            .chain(delay.longest.as_mut())
            .collect(),
        Node::Lowpass(filter) | Node::Highpass(filter) | Node::Bandpass(filter) => {
            vec![&mut filter.x, &mut filter.hz, &mut filter.q]
        }
        Node::Smooth(smooth) => vec![&mut smooth.x, &mut smooth.ms],
        Node::Adsr(adsr) => vec![
            &mut adsr.gate,
            &mut adsr.attack,
            &mut adsr.decay,
            &mut adsr.sustain,
            &mut adsr.release,
        ],
        Node::Hold(hold) => vec![&mut hold.x, &mut hold.when],
        Node::At(at) => vec![&mut at.index],
        Node::Lookup(lookup) => vec![&mut lookup.phase],
        Node::Write(write) => vec![&mut write.index, &mut write.value],
        Node::Device(device) => (device.input.iter_mut())
            .chain(device.numbers.values_mut())
            .collect(),
        Node::Played(played) => vec![&mut played.device],
        Node::Constant(_)
        | Node::Input
        | Node::InputLeft
        | Node::InputRight
        | Node::Channel
        | Node::SampleRate
        | Node::Beat
        | Node::Bpm
        | Node::Playing
        | Node::Frequency
        | Node::Pitch
        | Node::Gate
        | Node::Velocity
        | Node::Onset
        | Node::Param(_)
        | Node::Feedback(_)
        | Node::Noise
        | Node::Length(_)
        | Node::Import(_) => Vec::new(),
    }
}

fn reads(node: &Node) -> Vec<usize> {
    operands(&mut node.clone())
        .into_iter()
        .map(|read| *read)
        .collect()
}

/// The table `node` reads or writes.
fn table(node: &Node) -> Option<&TableNode> {
    match node {
        Node::At(at) => Some(&at.table),
        Node::Lookup(lookup) => Some(&lookup.table),
        Node::Length(length) => Some(&length.table),
        _ => None,
    }
}

/// The buffer `node` reads or writes.
fn buffer(node: &Node) -> Option<usize> {
    match (node, table(node)) {
        (Node::Write(write), _) => Some(write.buffer),
        (_, Some(TableNode::Buffer(buffer))) => Some(*buffer),
        _ => None,
    }
}

/// The stage of each node: the latest of what it reads, and one after a device for what the
/// device plays. A device is in the stage of the latest of what it hears. The read of a
/// feedback is in the stage of what it is set to, and all of a buffer in one.
fn stages(graph: &Graph) -> Result<Vec<usize>, GraphError> {
    let devices = (graph.nodes.iter())
        .filter(|node| matches!(node, Node::Device(_)))
        .count();
    let mut feedbacks = vec![0; graph.feedbacks.len()];
    let mut buffers = vec![0; graph.buffers.len()];
    loop {
        let mut stage: Vec<usize> = Vec::with_capacity(graph.nodes.len());
        for node in &graph.nodes {
            let read = reads(node).into_iter().filter_map(|read| stage.get(read));
            let mut at = read.max().copied().unwrap_or(0);
            match node {
                Node::Feedback(feedback) => {
                    at = feedbacks.get(feedback.slot).copied().unwrap_or(0);
                }
                Node::Played(_) => at += 1,
                _ => {}
            }
            if let Some(buffer) = buffer(node) {
                at = at.max(buffers.get(buffer).copied().unwrap_or(0));
            }
            stage.push(at);
        }
        let mut moved = false;
        for (slot, source) in graph.feedbacks.iter().enumerate() {
            let (Some(at), Some(floor)) = (stage.get(*source), feedbacks.get_mut(slot)) else {
                continue;
            };
            if *at > *floor {
                *floor = *at;
                moved = true;
            }
        }
        for (node, at) in graph.nodes.iter().zip(&stage) {
            let floor = buffer(node).and_then(|buffer| buffers.get_mut(buffer));
            if let Some(floor) = floor.filter(|floor| *at > **floor) {
                *floor = *at;
                moved = true;
            }
        }
        // Each device moves a stage on at most once, so a stage past their count goes round.
        if feedbacks
            .iter()
            .chain(&buffers)
            .any(|floor| *floor > devices)
        {
            return error(
                "a feedback or a buffer goes through a device: a device hears a block at once, so nothing it plays comes back within the block",
            );
        }
        if !moved {
            return Ok(stage);
        }
    }
}

/// The making of the stages of a graph.
struct Split<'a> {
    graph: &'a Graph,
    stage: Vec<usize>,
    /// The index of each device among the devices, by its node.
    device_of: HashMap<usize, usize>,
    graphs: Vec<Graph>,
    /// The node of each node of the graph in each stage.
    made: Vec<HashMap<usize, usize>>,
    /// What each port of each stage takes.
    imports: Vec<Vec<LinkFrom>>,
    /// The slot of each feedback, and the place of each buffer, in its stage.
    feedback_slots: Vec<usize>,
    buffer_places: Vec<usize>,
    links: Vec<Link>,
}

impl<'a> Split<'a> {
    fn new(graph: &'a Graph) -> Result<Self, GraphError> {
        let stage = stages(graph)?;
        let device_nodes: Vec<usize> = (graph.nodes.iter().enumerate())
            .filter(|(_, node)| matches!(node, Node::Device(_)))
            .map(|(index, _)| index)
            .collect();
        if device_nodes.len() > MAX_DEVICES {
            return error(format!(
                "a sound plays through at most {MAX_DEVICES} devices"
            ));
        }
        let count = stage.iter().max().map_or(1, |last| last + 1);
        let empty = || Graph {
            nodes: Vec::new(),
            feedbacks: Vec::new(),
            buffers: Vec::new(),
            watches: Vec::new(),
            numbers: Vec::new(),
            bangs: Vec::new(),
            notes: Vec::new(),
            output: OutputNode::Mono { mono: 0 },
            exports: Vec::new(),
            device_numbers: Vec::new(),
        };
        let mut graphs: Vec<Graph> = (0..count).map(|_| empty()).collect();
        let mut feedback_slots = Vec::new();
        for source in &graph.feedbacks {
            let at = stage.get(*source).copied().unwrap_or(0);
            let Some(graph) = graphs.get_mut(at) else {
                return error(format!(
                    "feedback {} is set to no node",
                    feedback_slots.len()
                ));
            };
            feedback_slots.push(graph.feedbacks.len());
            graph.feedbacks.push(0);
        }
        let mut buffer_places = Vec::new();
        for (index, seconds) in graph.buffers.iter().enumerate() {
            let at = (graph.nodes.iter().zip(&stage))
                .find(|(node, _)| buffer(node) == Some(index))
                .map_or(0, |(_, at)| *at);
            let Some(graph) = graphs.get_mut(at) else {
                return error(format!("buffer {index} is in no stage"));
            };
            buffer_places.push(graph.buffers.len());
            graph.buffers.push(*seconds);
        }
        Ok(Self {
            graph,
            device_of: (device_nodes.iter().enumerate())
                .map(|(index, node)| (*node, index))
                .collect(),
            made: vec![HashMap::new(); count],
            imports: vec![Vec::new(); count],
            stage,
            graphs,
            feedback_slots,
            buffer_places,
            links: Vec::new(),
        })
    }

    fn node(&self, index: usize) -> Result<&'a Node, GraphError> {
        match self.graph.nodes.get(index) {
            Some(node) => Ok(node),
            None => error(format!("node {index} is read before it is made")),
        }
    }

    fn stage_of(&self, index: usize) -> usize {
        self.stage.get(index).copied().unwrap_or(0)
    }

    fn push(&mut self, stage: usize, node: Node) -> Result<usize, GraphError> {
        let Some(graph) = self.graphs.get_mut(stage) else {
            return error(format!("stage {stage} is not made"));
        };
        graph.nodes.push(node);
        Ok(graph.nodes.len() - 1)
    }

    /// The node in `stage` of the node `index` of the graph: its own, a copy of a free one, or
    /// what an earlier stage or a device hands on.
    fn take(&mut self, stage: usize, index: usize) -> Result<usize, GraphError> {
        if let Some(made) = self.made.get(stage).and_then(|made| made.get(&index)) {
            return Ok(*made);
        }
        let node = self.node(index)?;
        let made = match node {
            node if free(node) => self.push(stage, node.clone())?,
            Node::Played(Played { device, channel }) => {
                let Some(device) = self.device_of.get(device).copied() else {
                    return error(format!(
                        "node {index} plays node {device}, which is no device"
                    ));
                };
                if channel.is_none() && matches!(self.graph.output, OutputNode::Stereo { .. }) {
                    return error(
                        "a stereo sound reads what a device plays by its .left and .right",
                    );
                }
                let port = self.import(stage, LinkFrom::Device(device))?;
                self.push(
                    stage,
                    Node::Import(Import {
                        port,
                        channel: *channel,
                    }),
                )?
            }
            Node::Device(_) => {
                return error(format!(
                    "node {index} is a device, not a signal: read what it plays"
                ));
            }
            _ if self.stage_of(index) < stage => {
                let from = self.export(self.stage_of(index), [index, index])?;
                let port = self.import(stage, from)?;
                self.push(
                    stage,
                    Node::Import(Import {
                        port,
                        channel: None,
                    }),
                )?
            }
            _ => return error(format!("node {index} is read before it is made")),
        };
        if let Some(stage) = self.made.get_mut(stage) {
            stage.insert(index, made);
        }
        Ok(made)
    }

    /// The port of `stage` that takes `from`.
    fn import(&mut self, stage: usize, from: LinkFrom) -> Result<u16, GraphError> {
        let Some(imports) = self.imports.get_mut(stage) else {
            return error(format!("stage {stage} is not made"));
        };
        let port = match imports.iter().position(|import| *import == from) {
            Some(port) => port,
            None => {
                imports.push(from);
                let port = imports.len() - 1;
                self.links.push(Link {
                    from,
                    to: LinkTo::Import { stage, port },
                });
                port
            }
        };
        u16::try_from(port).or_else(|_| error("too many signals cross a device"))
    }

    /// Where `stage` hands on the left and the right of `pair`.
    fn export(&mut self, stage: usize, [left, right]: [usize; 2]) -> Result<LinkFrom, GraphError> {
        let pair = [self.take(stage, left)?, self.take(stage, right)?];
        let Some(graph) = self.graphs.get_mut(stage) else {
            return error(format!("stage {stage} is not made"));
        };
        let port = match graph.exports.iter().position(|export| *export == pair) {
            Some(port) => port,
            None => {
                graph.exports.push(pair);
                graph.exports.len() - 1
            }
        };
        Ok(LinkFrom::Export { stage, port })
    }

    /// The device a sound comes straight from: when `pair` is just what it plays.
    fn played_by(&self, [left, right]: [usize; 2]) -> Option<usize> {
        let played = |index: usize| match self.graph.nodes.get(index) {
            Some(Node::Played(played)) => Some((played.device, played.channel)),
            _ => None,
        };
        match (played(left)?, played(right)?) {
            ((device, None), (other, None)) | ((device, Some(0)), (other, Some(1)))
                if device == other =>
            {
                self.device_of.get(&device).copied()
            }
            _ => None,
        }
    }

    /// `node` of the graph as a node of `stage`, reading the nodes of that stage.
    fn remake(&mut self, stage: usize, node: &Node) -> Result<Node, GraphError> {
        let mut node = node.clone();
        for read in operands(&mut node) {
            *read = self.take(stage, *read)?;
        }
        match &mut node {
            Node::Feedback(feedback) => {
                feedback.slot = self.feedback_slots.get(feedback.slot).copied().unwrap_or(0);
            }
            Node::Write(write) => {
                write.buffer = self.buffer_places.get(write.buffer).copied().unwrap_or(0);
            }
            _ => {}
        }
        if let Some(TableNode::Buffer(buffer)) = table_mut(&mut node) {
            *buffer = self.buffer_places.get(*buffer).copied().unwrap_or(0);
        }
        Ok(node)
    }

    fn named(&mut self, nodes: &[NamedNode], kind: Kind) -> Result<(), GraphError> {
        for named in nodes {
            let stage = self.stage_of(named.node);
            let node = self.take(stage, named.node)?;
            let named = NamedNode {
                name: named.name.clone(),
                node,
            };
            let Some(graph) = self.graphs.get_mut(stage) else {
                continue;
            };
            match kind {
                Kind::Watch => graph.watches.push(named),
                Kind::Number => graph.numbers.push(named),
                Kind::Bang => graph.bangs.push(named),
            }
        }
        Ok(())
    }

    fn run(mut self, declarations: &Declarations) -> Result<Program, GraphError> {
        let graph = self.graph;
        for (index, node) in graph.nodes.iter().enumerate() {
            if free(node) || matches!(node, Node::Device(_) | Node::Played(_)) {
                continue;
            }
            let stage = self.stage_of(index);
            if table(node).is_some_and(|table| matches!(table, TableNode::List(_))) && stage > 0 {
                return error(
                    "a pattern or a sample is read before every device, not with what a device plays",
                );
            }
            let made = self.remake(stage, node)?;
            let made = self.push(stage, made)?;
            if let Some(stage) = self.made.get_mut(stage) {
                stage.insert(index, made);
            }
        }
        for (slot, source) in graph.feedbacks.iter().enumerate() {
            let stage = self.stage_of(*source);
            let node = self.take(stage, *source)?;
            let place = self.feedback_slots.get(slot).copied().unwrap_or(0);
            if let Some(set) = (self.graphs.get_mut(stage)).and_then(|g| g.feedbacks.get_mut(place))
            {
                *set = node;
            }
        }
        self.named(&graph.watches, Kind::Watch)?;
        self.named(&graph.numbers, Kind::Number)?;
        self.named(&graph.bangs, Kind::Bang)?;
        for notes in &graph.notes {
            let stage = [notes.gate, notes.pitch, notes.velocity]
                .map(|node| self.stage_of(node))
                .into_iter()
                .max()
                .unwrap_or(0);
            let made = NotesNode {
                name: notes.name.clone(),
                gate: self.take(stage, notes.gate)?,
                pitch: self.take(stage, notes.pitch)?,
                velocity: self.take(stage, notes.velocity)?,
            };
            if let Some(graph) = self.graphs.get_mut(stage) {
                graph.notes.push(made);
            }
        }
        let mut devices = Vec::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            let Node::Device(device) = node else {
                continue;
            };
            let at = self.device_of.get(&index).copied().unwrap_or(0);
            let from = match self.played_by(device.input) {
                Some(device) => LinkFrom::Device(device),
                None => {
                    let stage = device.input.map(|node| self.stage_of(node));
                    self.export(stage[0].max(stage[1]), device.input)?
                }
            };
            self.links.push(Link {
                from,
                to: LinkTo::Device(at),
            });
            let mut state = device.state.clone();
            let mut numbers = Vec::new();
            for (field, node) in &device.numbers {
                if let Node::Constant(constant) = self.node(*node)? {
                    state.insert(field.clone(), Value::from(f64::from(constant.value)));
                    continue;
                }
                let stage = self.stage_of(*node);
                let made = self.take(stage, *node)?;
                let Some(graph) = self.graphs.get_mut(stage) else {
                    continue;
                };
                graph.device_numbers.push(made);
                numbers.push(DeviceNumber {
                    field: field.clone(),
                    stage,
                    index: graph.device_numbers.len() - 1,
                });
            }
            devices.push(DeviceUse {
                tool: device.tool.clone(),
                state,
                numbers,
            });
        }
        let (pair, stereo) = match graph.output {
            OutputNode::Mono { mono } => ([mono, mono], false),
            OutputNode::Stereo { left, right } => ([left, right], true),
        };
        let sound = match self.played_by(pair) {
            Some(device) => Sound::Device(device),
            None => {
                let stage = self.stage_of(pair[0]).max(self.stage_of(pair[1]));
                let [left, right] = [self.take(stage, pair[0])?, self.take(stage, pair[1])?];
                if let Some(graph) = self.graphs.get_mut(stage) {
                    graph.output = match stereo {
                        true => OutputNode::Stereo { left, right },
                        false => OutputNode::Mono { mono: left },
                    };
                }
                Sound::Stage(stage)
            }
        };
        // The other stages play silence, and run as the sound does: on each channel apart, or
        // once for both.
        for (index, graph) in self.graphs.iter_mut().enumerate() {
            if sound != Sound::Stage(index) {
                graph
                    .nodes
                    .push(Node::Constant(crate::graph::Constant { value: 0.0 }));
                let silence = graph.nodes.len() - 1;
                graph.output = match stereo {
                    true => OutputNode::Stereo {
                        left: silence,
                        right: silence,
                    },
                    false => OutputNode::Mono { mono: silence },
                };
            }
        }
        // A stage that is only what a device plays, straight into another device, is none.
        let kept: Vec<bool> = (self.graphs.iter().enumerate())
            .map(|(index, graph)| {
                index == 0 || sound == Sound::Stage(index) || graph.nodes.len() > 1
            })
            .collect();
        let place = |stage: usize| kept.iter().take(stage).filter(|kept| **kept).count();
        let graphs: Vec<Graph> = (self.graphs.into_iter().zip(&kept))
            .filter_map(|(graph, kept)| kept.then_some(graph))
            .collect();
        let links = (self.links.into_iter())
            .map(|link| Link {
                from: match link.from {
                    LinkFrom::Export { stage, port } => LinkFrom::Export {
                        stage: place(stage),
                        port,
                    },
                    device => device,
                },
                to: match link.to {
                    LinkTo::Import { stage, port } => LinkTo::Import {
                        stage: place(stage),
                        port,
                    },
                    device => device,
                },
            })
            .collect();
        for device in &mut devices {
            for send in &mut device.numbers {
                send.stage = place(send.stage);
            }
        }
        let sound = match sound {
            Sound::Stage(stage) => Sound::Stage(place(stage)),
            device => device,
        };
        let stages = (graphs.iter().enumerate())
            .map(|(stage, graph)| match stage {
                0 => graph.compile(declarations),
                _ => graph.compile(&Declarations::default()),
            })
            .collect::<Result<_, _>>()?;
        Ok(Program {
            stages,
            devices,
            links,
            sound,
        })
    }
}

#[derive(Copy, Clone)]
enum Kind {
    Watch,
    Number,
    Bang,
}

fn table_mut(node: &mut Node) -> Option<&mut TableNode> {
    match node {
        Node::At(at) => Some(&mut at.table),
        Node::Lookup(lookup) => Some(&mut lookup.table),
        Node::Length(length) => Some(&mut length.table),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn program(graph: serde_json::Value) -> Result<Program, GraphError> {
        let graph: Graph =
            serde_json::from_value(graph).map_err(|error| GraphError(error.to_string()))?;
        graph.program(&Declarations::default())
    }

    fn graph(
        nodes: serde_json::Value,
        feedbacks: serde_json::Value,
        output: usize,
    ) -> serde_json::Value {
        json!({
            "nodes": nodes, "feedbacks": feedbacks, "buffers": [], "watches": [], "numbers": [],
            "bangs": [], "notes": [], "output": { "mono": output },
        })
    }

    fn device(tool: &str, input: usize) -> serde_json::Value {
        json!({ "op": "device", "tool": tool, "input": [input, input], "numbers": {}, "state": {} })
    }

    #[test]
    fn a_feedback_through_a_device_is_refused() {
        let nodes = json!([
            { "op": "input" },
            { "op": "feedback", "slot": 0 },
            { "op": "add", "a": 0, "b": 1 },
            device("filter", 2),
            { "op": "played", "device": 3 },
        ]);
        let refused = program(graph(nodes, json!([4]), 2)).map(drop);
        assert!(
            refused.is_err_and(|error| error.0.contains("goes through a device")),
            "a feedback that loops through a device would hear it a block late"
        );
    }

    #[test]
    fn a_device_straight_into_another_has_no_stage_between_them() {
        let nodes = json!([
            { "op": "input" },
            device("filter", 0),
            { "op": "played", "device": 1 },
            device("reverb", 2),
            { "op": "played", "device": 3 },
        ]);
        let program = program(graph(nodes, json!([]), 4)).unwrap();
        assert_eq!(program.stages.len(), 1);
        assert_eq!(
            program.links,
            [
                Link {
                    from: LinkFrom::Export { stage: 0, port: 0 },
                    to: LinkTo::Device(0)
                },
                Link {
                    from: LinkFrom::Device(0),
                    to: LinkTo::Device(1)
                },
            ]
        );
        assert_eq!(program.sound, Sound::Device(1));
    }
}
