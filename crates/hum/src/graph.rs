//! The sound graph the SDK of a tool sends, and its lowering to [`Code`], the flat list of
//! operations a [`Machine`](crate::Machine) runs.
//!
//! The nodes come in order, each reading nodes before it by their index, so one pass in order
//! lowers them: a node is one operation, but a field or a control reads the operation made for
//! it at the start.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::Deserialize;

use crate::language::{
    ArraySpec, Binary, Code, FilterKind, MAX_ARRAY_LENGTH, MAX_BUFFER_SECONDS, MAX_DELAY_MS,
    MAX_DELAYS, MAX_LIVES, MAX_OPERATIONS, MAX_PARAMETERS, MAX_WATCHES, Operation, Output,
    ParameterSpec, Register, Slots, Table, Unary,
};

/// A sound as the SDK sends it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    nodes: Vec<Node>,
    /// The node each `feedback` is set to, by its slot.
    feedbacks: Vec<usize>,
    /// The seconds of each `buffer`.
    buffers: Vec<f32>,
    watches: Vec<WatchNode>,
    output: OutputNode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchNode {
    name: String,
    node: usize,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OutputNode {
    /// Runs on each channel apart.
    Mono { mono: usize },
    /// Runs once for both channels, and hears both.
    Stereo { left: usize, right: usize },
}

/// Where a table reads: a pattern or sample field by name, or a buffer by its number.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
enum TableNode {
    List(String),
    Buffer(usize),
}

/// A node of the graph. Each `usize` is the index of a node before it.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
enum Node {
    Constant {
        value: f32,
    },
    Input,
    InputLeft,
    InputRight,
    Channel,
    SampleRate,
    Beat,
    Bpm,
    Playing,
    Frequency,
    Pitch,
    Gate,
    Velocity,
    Onset,
    /// A field or a control, by name.
    Param {
        name: String,
    },
    Feedback {
        slot: usize,
    },
    Negate {
        x: usize,
    },
    Sin {
        x: usize,
    },
    Cos {
        x: usize,
    },
    Tan {
        x: usize,
    },
    Tanh {
        x: usize,
    },
    Abs {
        x: usize,
    },
    Sqrt {
        x: usize,
    },
    Exp {
        x: usize,
    },
    Log {
        x: usize,
    },
    Floor {
        x: usize,
    },
    Wrap {
        x: usize,
    },
    Db {
        x: usize,
    },
    Saturate {
        x: usize,
    },
    Add {
        a: usize,
        b: usize,
    },
    Subtract {
        a: usize,
        b: usize,
    },
    Multiply {
        a: usize,
        b: usize,
    },
    Divide {
        a: usize,
        b: usize,
    },
    Remainder {
        a: usize,
        b: usize,
    },
    Less {
        a: usize,
        b: usize,
    },
    Greater {
        a: usize,
        b: usize,
    },
    LessOrEqual {
        a: usize,
        b: usize,
    },
    GreaterOrEqual {
        a: usize,
        b: usize,
    },
    Equal {
        a: usize,
        b: usize,
    },
    NotEqual {
        a: usize,
        b: usize,
    },
    Min {
        a: usize,
        b: usize,
    },
    Max {
        a: usize,
        b: usize,
    },
    Pow {
        a: usize,
        b: usize,
    },
    Clamp {
        x: usize,
        low: usize,
        high: usize,
    },
    Mix {
        a: usize,
        b: usize,
        amount: usize,
    },
    Phasor {
        hz: usize,
    },
    Noise,
    /// `longest` is a constant node.
    Delay {
        x: usize,
        ms: usize,
        longest: Option<usize>,
    },
    Lowpass {
        x: usize,
        hz: usize,
        q: usize,
    },
    Highpass {
        x: usize,
        hz: usize,
        q: usize,
    },
    Bandpass {
        x: usize,
        hz: usize,
        q: usize,
    },
    Smooth {
        x: usize,
        ms: usize,
    },
    Adsr {
        gate: usize,
        attack: usize,
        decay: usize,
        sustain: usize,
        release: usize,
    },
    Hold {
        x: usize,
        when: usize,
    },
    Rise {
        x: usize,
    },
    Change {
        x: usize,
    },
    At {
        table: TableNode,
        index: usize,
    },
    Lookup {
        table: TableNode,
        phase: usize,
    },
    Length {
        table: TableNode,
    },
    Write {
        buffer: usize,
        index: usize,
        value: usize,
    },
}

/// What the record and the interface give a sound, in the order of the tool's fields and
/// controls.
#[derive(Clone, Debug, Default)]
pub struct Declarations {
    /// The knobs and toggles.
    pub parameters: Vec<ParameterSpec>,
    /// The patterns and samples.
    pub arrays: Vec<ArraySpec>,
    pub controls: Vec<ControlSpec>,
}

#[derive(Clone, Debug)]
pub enum ControlSpec {
    Live(ParameterSpec),
    Trigger(String),
}

/// Why a sound does not build.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct GraphError(String);

fn error<T>(message: impl Into<String>) -> Result<T, GraphError> {
    Err(GraphError(message.into()))
}

impl Graph {
    pub fn compile(&self, declarations: &Declarations) -> Result<Code, GraphError> {
        // Each makes at most one operation, so more of them than operations is no sound.
        if self.feedbacks.len() > MAX_OPERATIONS || self.buffers.len() > MAX_OPERATIONS {
            return error(format!(
                "a sound has at most {MAX_OPERATIONS} feedbacks and buffers"
            ));
        }
        let mut hasher = DefaultHasher::new();
        format!("{self:?}{declarations:?}").hash(&mut hasher);
        let mut lowering = Lowering {
            graph: self,
            code: Code {
                parameters: Vec::new(),
                arrays: Vec::new(),
                lives: Vec::new(),
                triggers: Vec::new(),
                watches: Vec::new(),
                hash: hasher.finish(),
                operations: Vec::new(),
                output: Output::Through,
                history_writes: Vec::new(),
                watch_registers: Vec::new(),
                slots: Slots::default(),
            },
            names: HashMap::new(),
            registers: Vec::with_capacity(self.nodes.len()),
        };
        lowering.declare(declarations)?;
        for &seconds in &self.buffers {
            let total = lowering.code.slots.buffers.iter().sum::<f32>() + seconds;
            if seconds <= 0.0 || total > MAX_BUFFER_SECONDS {
                return error(format!(
                    "buffer({seconds}): a buffer holds more than 0 s, and the buffers of a sound up to {MAX_BUFFER_SECONDS} s together"
                ));
            }
            lowering.code.slots.buffers.push(seconds);
        }
        lowering.code.slots.histories = self.feedbacks.len() as u16;
        for node in &self.nodes {
            let register = lowering.node(node)?;
            lowering.registers.push(register);
        }
        for (slot, node) in self.feedbacks.iter().enumerate() {
            let register = lowering.read(*node)?;
            lowering.code.history_writes.push((slot as u16, register));
        }
        if self.watches.len() > MAX_WATCHES {
            return error(format!("a sound has at most {MAX_WATCHES} watches"));
        }
        for watch in &self.watches {
            if lowering.code.watches.contains(&watch.name) {
                return error(format!("watch `{}` is shown twice", watch.name));
            }
            let register = lowering.read(watch.node)?;
            lowering.code.watches.push(watch.name.clone());
            lowering.code.watch_registers.push(register);
        }
        lowering.code.output = match self.output {
            OutputNode::Mono { mono } => Output::Mono(lowering.read(mono)?),
            OutputNode::Stereo { left, right } => {
                Output::Stereo(lowering.read(left)?, lowering.read(right)?)
            }
        };
        Ok(lowering.code)
    }
}

struct Lowering<'a> {
    graph: &'a Graph,
    code: Code,
    /// The register of each field and control.
    names: HashMap<&'a str, Register>,
    /// The register of each node lowered so far.
    registers: Vec<Register>,
}

impl<'a> Lowering<'a> {
    /// The operations of the fields and controls, which come first, and their specs.
    fn declare(&mut self, declarations: &'a Declarations) -> Result<(), GraphError> {
        if declarations.parameters.len() > MAX_PARAMETERS {
            return error(format!(
                "a tool has at most {MAX_PARAMETERS} knobs and toggles"
            ));
        }
        for (index, parameter) in declarations.parameters.iter().enumerate() {
            check_range(
                &parameter.name,
                parameter.default,
                parameter.min,
                parameter.max,
            )?;
            let register = self.emit(Operation::Parameter(index as u16))?;
            self.names.insert(&parameter.name, register);
        }
        self.code.parameters = declarations.parameters.clone();
        for array in &declarations.arrays {
            if array
                .length
                .is_some_and(|length| !(1..=MAX_ARRAY_LENGTH).contains(&length))
            {
                return error(format!(
                    "`{}`: a pattern holds 1 to {MAX_ARRAY_LENGTH} values",
                    array.name
                ));
            }
            check_range(&array.name, array.default, array.min, array.max)?;
        }
        self.code.arrays = declarations.arrays.clone();
        for control in &declarations.controls {
            let (name, operation) = match control {
                ControlSpec::Live(live) => {
                    if self.code.lives.len() == MAX_LIVES {
                        return error(format!("a tool has at most {MAX_LIVES} live controls"));
                    }
                    check_range(&live.name, live.default, live.min, live.max)?;
                    self.code.lives.push(live.clone());
                    (
                        &live.name,
                        Operation::Live(self.code.lives.len() as u16 - 1),
                    )
                }
                ControlSpec::Trigger(name) => {
                    if self.code.triggers.len() == MAX_LIVES {
                        return error(format!("a tool has at most {MAX_LIVES} triggers"));
                    }
                    self.code.triggers.push(name.clone());
                    (
                        name,
                        Operation::Trigger(self.code.triggers.len() as u16 - 1),
                    )
                }
            };
            let register = self.emit(operation)?;
            self.names.insert(name, register);
        }
        Ok(())
    }

    fn emit(&mut self, operation: Operation) -> Result<Register, GraphError> {
        if self.code.operations.len() == MAX_OPERATIONS {
            return error(format!(
                "a sound has at most {MAX_OPERATIONS} operations: build it from fewer signals"
            ));
        }
        self.code.operations.push(operation);
        Ok((self.code.operations.len() - 1) as Register)
    }

    /// The register of a node lowered before.
    fn read(&self, node: usize) -> Result<Register, GraphError> {
        match self.registers.get(node) {
            Some(register) => Ok(*register),
            None => error(format!("node {node} is read before it is made")),
        }
    }

    fn unary(&self, unary: Unary, x: usize) -> Result<Operation, GraphError> {
        Ok(Operation::Unary(unary, self.read(x)?))
    }

    fn binary(&self, binary: Binary, a: usize, b: usize) -> Result<Operation, GraphError> {
        Ok(Operation::Binary(binary, self.read(a)?, self.read(b)?))
    }

    fn filter(
        &mut self,
        kind: FilterKind,
        x: usize,
        hz: usize,
        q: usize,
    ) -> Result<Operation, GraphError> {
        Ok(Operation::Filter {
            kind,
            input: self.read(x)?,
            hz: self.read(hz)?,
            q: self.read(q)?,
            slot: next(&mut self.code.slots.filters),
        })
    }

    fn table(&self, table: &TableNode) -> Result<Table, GraphError> {
        match table {
            TableNode::List(name) => {
                let index = (self.code.arrays.iter()).position(|array| array.name == *name);
                match index {
                    Some(index) => Ok(Table::Array(index as u16)),
                    None => error(format!(
                        "`{name}` is no pattern or sample field of the tool"
                    )),
                }
            }
            TableNode::Buffer(index) => Ok(Table::Buffer(self.buffer(*index)?)),
        }
    }

    fn buffer(&self, index: usize) -> Result<u16, GraphError> {
        match index < self.code.slots.buffers.len() {
            true => Ok(index as u16),
            false => error(format!("buffer {index} is not made")),
        }
    }

    /// The register of `node`: of the operation it makes, or of its field or control.
    fn node(&mut self, node: &Node) -> Result<Register, GraphError> {
        let operation = match node {
            Node::Constant { value } => Operation::Constant(*value),
            Node::Input => Operation::Input,
            Node::InputLeft => Operation::InputLeft,
            Node::InputRight => Operation::InputRight,
            Node::Channel => Operation::Channel,
            Node::SampleRate => Operation::SampleRate,
            Node::Beat => Operation::Beat,
            Node::Bpm => Operation::Bpm,
            Node::Playing => Operation::Playing,
            Node::Frequency => Operation::Frequency,
            Node::Pitch => Operation::Pitch,
            Node::Gate => Operation::Gate,
            Node::Velocity => Operation::Velocity,
            Node::Onset => Operation::Onset,
            Node::Param { name } => {
                return match self.names.get(name.as_str()) {
                    Some(register) => Ok(*register),
                    None => error(format!("`{name}` is no field or control of the tool")),
                };
            }
            Node::Feedback { slot } if *slot < self.graph.feedbacks.len() => {
                Operation::History(*slot as u16)
            }
            Node::Feedback { slot } => return error(format!("feedback {slot} is never set")),
            Node::Negate { x } => self.unary(Unary::Negate, *x)?,
            Node::Sin { x } => self.unary(Unary::Sin, *x)?,
            Node::Cos { x } => self.unary(Unary::Cos, *x)?,
            Node::Tan { x } => self.unary(Unary::Tan, *x)?,
            Node::Tanh { x } => self.unary(Unary::Tanh, *x)?,
            Node::Abs { x } => self.unary(Unary::Abs, *x)?,
            Node::Sqrt { x } => self.unary(Unary::Sqrt, *x)?,
            Node::Exp { x } => self.unary(Unary::Exp, *x)?,
            Node::Log { x } => self.unary(Unary::Log, *x)?,
            Node::Floor { x } => self.unary(Unary::Floor, *x)?,
            Node::Wrap { x } => self.unary(Unary::Wrap, *x)?,
            Node::Db { x } => self.unary(Unary::Decibels, *x)?,
            Node::Saturate { x } => self.unary(Unary::Saturate, *x)?,
            Node::Add { a, b } => self.binary(Binary::Add, *a, *b)?,
            Node::Subtract { a, b } => self.binary(Binary::Subtract, *a, *b)?,
            Node::Multiply { a, b } => self.binary(Binary::Multiply, *a, *b)?,
            Node::Divide { a, b } => self.binary(Binary::Divide, *a, *b)?,
            Node::Remainder { a, b } => self.binary(Binary::Remainder, *a, *b)?,
            Node::Less { a, b } => self.binary(Binary::Less, *a, *b)?,
            Node::Greater { a, b } => self.binary(Binary::Greater, *a, *b)?,
            Node::LessOrEqual { a, b } => self.binary(Binary::LessOrEqual, *a, *b)?,
            Node::GreaterOrEqual { a, b } => self.binary(Binary::GreaterOrEqual, *a, *b)?,
            Node::Equal { a, b } => self.binary(Binary::Equal, *a, *b)?,
            Node::NotEqual { a, b } => self.binary(Binary::NotEqual, *a, *b)?,
            Node::Min { a, b } => self.binary(Binary::Min, *a, *b)?,
            Node::Max { a, b } => self.binary(Binary::Max, *a, *b)?,
            Node::Pow { a, b } => self.binary(Binary::Power, *a, *b)?,
            Node::Clamp { x, low, high } => {
                Operation::Clamp(self.read(*x)?, self.read(*low)?, self.read(*high)?)
            }
            Node::Mix { a, b, amount } => {
                Operation::Mix(self.read(*a)?, self.read(*b)?, self.read(*amount)?)
            }
            Node::Phasor { hz } => Operation::Phasor {
                hz: self.read(*hz)?,
                slot: next(&mut self.code.slots.phasors),
            },
            Node::Noise => Operation::Noise {
                slot: next(&mut self.code.slots.noises),
            },
            Node::Delay { x, ms, longest } => {
                let (input, ms) = (self.read(*x)?, self.read(*ms)?);
                if self.code.slots.delays.len() == MAX_DELAYS {
                    return error(format!("a sound has at most {MAX_DELAYS} delays"));
                }
                let longest = match longest {
                    None => MAX_DELAY_MS,
                    Some(longest) => {
                        self.read(*longest)?;
                        match self.graph.nodes.get(*longest) {
                            Some(Node::Constant { value })
                                if *value > 0.0 && *value <= MAX_DELAY_MS =>
                            {
                                *value
                            }
                            _ => {
                                return error(format!(
                                    "delay(x, ms, longest): longest is a number of ms above 0 and up to {MAX_DELAY_MS}"
                                ));
                            }
                        }
                    }
                };
                self.code.slots.delays.push(longest);
                Operation::Delay {
                    input,
                    ms,
                    slot: self.code.slots.delays.len() as u16 - 1,
                }
            }
            Node::Lowpass { x, hz, q } => self.filter(FilterKind::LowPass, *x, *hz, *q)?,
            Node::Highpass { x, hz, q } => self.filter(FilterKind::HighPass, *x, *hz, *q)?,
            Node::Bandpass { x, hz, q } => self.filter(FilterKind::BandPass, *x, *hz, *q)?,
            Node::Smooth { x, ms } => Operation::Smooth {
                input: self.read(*x)?,
                ms: self.read(*ms)?,
                slot: next(&mut self.code.slots.smooths),
            },
            Node::Adsr {
                gate,
                attack,
                decay,
                sustain,
                release,
            } => Operation::Envelope {
                gate: self.read(*gate)?,
                attack: self.read(*attack)?,
                decay: self.read(*decay)?,
                sustain: self.read(*sustain)?,
                release: self.read(*release)?,
                slot: next(&mut self.code.slots.envelopes),
            },
            Node::Hold { x, when } => Operation::Hold {
                input: self.read(*x)?,
                when: self.read(*when)?,
                slot: next(&mut self.code.slots.memories),
            },
            Node::Rise { x } => Operation::Rise {
                input: self.read(*x)?,
                slot: next(&mut self.code.slots.memories),
            },
            Node::Change { x } => Operation::Change {
                input: self.read(*x)?,
                slot: next(&mut self.code.slots.memories),
            },
            Node::At { table, index } => Operation::Read {
                table: self.table(table)?,
                index: self.read(*index)?,
            },
            Node::Lookup { table, phase } => Operation::Lookup {
                table: self.table(table)?,
                phase: self.read(*phase)?,
            },
            Node::Length { table } => Operation::Length(self.table(table)?),
            Node::Write {
                buffer,
                index,
                value,
            } => Operation::Write {
                buffer: self.buffer(*buffer)?,
                index: self.read(*index)?,
                value: self.read(*value)?,
            },
        };
        self.emit(operation)
    }
}

/// `default` in `[min, max]`, and the range not empty.
fn check_range(name: &str, default: f32, min: f32, max: f32) -> Result<(), GraphError> {
    if min >= max {
        return error(format!("`{name}`: the range [{min}, {max}] is empty"));
    }
    if !(min..=max).contains(&default) {
        return error(format!(
            "`{name}`: the default {default} is outside [{min}, {max}]"
        ));
    }
    Ok(())
}

fn next(count: &mut u16) -> u16 {
    *count += 1;
    *count - 1
}
