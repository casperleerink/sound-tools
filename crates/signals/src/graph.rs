//! The sound graph the SDK of a tool sends as JSON, and its lowering to [`Code`], the flat list
//! of operations a [`Machine`](crate::Machine) runs.
//!
//! The nodes come in order, each reading nodes before it by their index, so one pass in order
//! lowers them: a node is one operation, but a field or a control reads the operation made for
//! it at the start.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::Deserialize;

use crate::code::{
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
    Constant(Constant),
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
    Param(Param),
    Feedback(Feedback),
    Negate(One),
    Sin(One),
    Cos(One),
    Tan(One),
    Tanh(One),
    Abs(One),
    Sqrt(One),
    Exp(One),
    Log(One),
    Floor(One),
    Wrap(One),
    Db(One),
    Saturate(One),
    Add(Two),
    Subtract(Two),
    Multiply(Two),
    Divide(Two),
    Remainder(Two),
    Less(Two),
    Greater(Two),
    LessOrEqual(Two),
    GreaterOrEqual(Two),
    Equal(Two),
    NotEqual(Two),
    Min(Two),
    Max(Two),
    Pow(Two),
    Clamp(Clamp),
    Mix(Mix),
    Phasor(Phasor),
    Noise,
    Delay(Delay),
    Lowpass(Filter),
    Highpass(Filter),
    Bandpass(Filter),
    Smooth(Smooth),
    Adsr(Adsr),
    Hold(Hold),
    Rise(One),
    Change(One),
    At(At),
    Lookup(Lookup),
    Length(Length),
    Write(Write),
}

/// The fields of the nodes, as the SDK names them.
macro_rules! fields {
    ($($name:ident { $($field:ident: $type:ty),* })*) => {$(
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $name { $($field: $type),* }
    )*};
}

fields! {
    Constant { value: f32 }
    Param { name: String }
    Feedback { slot: usize }
    One { x: usize }
    Two { a: usize, b: usize }
    Clamp { x: usize, low: usize, high: usize }
    Mix { a: usize, b: usize, amount: usize }
    Phasor { hz: usize }
    // `longest` is a constant node.
    Delay { x: usize, ms: usize, longest: Option<usize> }
    Filter { x: usize, hz: usize, q: usize }
    Smooth { x: usize, ms: usize }
    Adsr { gate: usize, attack: usize, decay: usize, sustain: usize, release: usize }
    Hold { x: usize, when: usize }
    At { table: TableNode, index: usize }
    Lookup { table: TableNode, phase: usize }
    Length { table: TableNode }
    Write { buffer: usize, index: usize, value: usize }
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

/// Why a sound does not build, in the words of the SDK.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct GraphError(String);

fn error<T>(message: impl Into<String>) -> Result<T, GraphError> {
    Err(GraphError(message.into()))
}

impl Graph {
    pub fn compile(&self, declarations: &Declarations) -> Result<Code, GraphError> {
        // Each is read by an operation, so more of them than operations is no sound.
        if self.feedbacks.len() > MAX_OPERATIONS || self.buffers.len() > MAX_OPERATIONS {
            return error(format!(
                "a sound has at most {MAX_OPERATIONS} feedbacks and buffers"
            ));
        }
        let mut lowering = Lowering {
            graph: self,
            arrays: &declarations.arrays,
            operations: Vec::new(),
            slots: Slots {
                histories: self.feedbacks.len() as u16,
                ..Slots::default()
            },
            lives: Vec::new(),
            triggers: Vec::new(),
            names: HashMap::new(),
            registers: Vec::with_capacity(self.nodes.len()),
        };
        lowering.declare(declarations)?;
        for &seconds in &self.buffers {
            let total = lowering.slots.buffers.iter().sum::<f32>() + seconds;
            if seconds <= 0.0 || total > MAX_BUFFER_SECONDS {
                return error(format!(
                    "buffer({seconds}): a buffer holds more than 0 s, and the buffers of a sound up to {MAX_BUFFER_SECONDS} s together"
                ));
            }
            lowering.slots.buffers.push(seconds);
        }
        for node in &self.nodes {
            let register = lowering.node(node)?;
            lowering.registers.push(register);
        }
        let history_writes = (self.feedbacks.iter().enumerate())
            .map(|(slot, node)| Ok((slot as u16, lowering.read(*node)?)))
            .collect::<Result<_, GraphError>>()?;
        if self.watches.len() > MAX_WATCHES {
            return error(format!("a sound has at most {MAX_WATCHES} watches"));
        }
        let mut watches: Vec<String> = Vec::new();
        for watch in &self.watches {
            if watches.contains(&watch.name) {
                return error(format!("watch `{}` is shown twice", watch.name));
            }
            watches.push(watch.name.clone());
        }
        let watch_registers = (self.watches.iter())
            .map(|watch| lowering.read(watch.node))
            .collect::<Result<_, _>>()?;
        let output = match self.output {
            OutputNode::Mono { mono } => Output::Mono(lowering.read(mono)?),
            OutputNode::Stereo { left, right } => {
                Output::Stereo(lowering.read(left)?, lowering.read(right)?)
            }
        };
        let mut hasher = DefaultHasher::new();
        format!("{self:?}{declarations:?}").hash(&mut hasher);
        Ok(Code {
            parameters: declarations.parameters.clone(),
            arrays: declarations.arrays.clone(),
            lives: lowering.lives,
            triggers: lowering.triggers,
            watches,
            hash: hasher.finish(),
            operations: lowering.operations,
            output,
            history_writes,
            watch_registers,
            slots: lowering.slots,
        })
    }
}

struct Lowering<'a> {
    graph: &'a Graph,
    arrays: &'a [ArraySpec],
    operations: Vec<Operation>,
    slots: Slots,
    lives: Vec<ParameterSpec>,
    triggers: Vec<String>,
    /// The register of each field and control.
    names: HashMap<&'a str, Register>,
    /// The register of each node lowered so far.
    registers: Vec<Register>,
}

impl<'a> Lowering<'a> {
    /// The operations of the fields and controls, which come first.
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
        for array in &declarations.arrays {
            if !(1..=MAX_ARRAY_LENGTH).contains(&array.length.unwrap_or(1)) {
                return error(format!(
                    "`{}`: a pattern holds 1 to {MAX_ARRAY_LENGTH} values",
                    array.name
                ));
            }
            check_range(&array.name, array.default, array.min, array.max)?;
        }
        for control in &declarations.controls {
            let (name, operation) = match control {
                ControlSpec::Live(live) => {
                    if self.lives.len() == MAX_LIVES {
                        return error(format!("a tool has at most {MAX_LIVES} live controls"));
                    }
                    check_range(&live.name, live.default, live.min, live.max)?;
                    let index = self.lives.len() as u16;
                    self.lives.push(live.clone());
                    (&live.name, Operation::Live(index))
                }
                ControlSpec::Trigger(name) => {
                    if self.triggers.len() == MAX_LIVES {
                        return error(format!("a tool has at most {MAX_LIVES} triggers"));
                    }
                    let index = self.triggers.len() as u16;
                    self.triggers.push(name.clone());
                    (name, Operation::Trigger(index))
                }
            };
            let register = self.emit(operation)?;
            self.names.insert(name, register);
        }
        Ok(())
    }

    fn emit(&mut self, operation: Operation) -> Result<Register, GraphError> {
        if self.operations.len() == MAX_OPERATIONS {
            return error(format!(
                "a sound has at most {MAX_OPERATIONS} operations: build it from fewer signals"
            ));
        }
        self.operations.push(operation);
        Ok((self.operations.len() - 1) as Register)
    }

    /// The register of a node lowered before.
    fn read(&self, node: usize) -> Result<Register, GraphError> {
        match self.registers.get(node) {
            Some(register) => Ok(*register),
            None => error(format!("node {node} is read before it is made")),
        }
    }

    fn unary(&self, unary: Unary, One { x }: &One) -> Result<Operation, GraphError> {
        Ok(Operation::Unary(unary, self.read(*x)?))
    }

    fn binary(&self, binary: Binary, Two { a, b }: &Two) -> Result<Operation, GraphError> {
        Ok(Operation::Binary(binary, self.read(*a)?, self.read(*b)?))
    }

    fn filter(&mut self, kind: FilterKind, filter: &Filter) -> Result<Operation, GraphError> {
        Ok(Operation::Filter {
            kind,
            input: self.read(filter.x)?,
            hz: self.read(filter.hz)?,
            q: self.read(filter.q)?,
            slot: next(&mut self.slots.filters),
        })
    }

    fn table(&self, table: &TableNode) -> Result<Table, GraphError> {
        match table {
            TableNode::List(name) => match self.arrays.iter().position(|a| a.name == *name) {
                Some(index) => Ok(Table::Array(index as u16)),
                None => error(format!(
                    "`{name}` is no pattern or sample field of the tool"
                )),
            },
            TableNode::Buffer(index) => Ok(Table::Buffer(self.buffer(*index)?)),
        }
    }

    fn buffer(&self, index: usize) -> Result<u16, GraphError> {
        match index < self.slots.buffers.len() {
            true => Ok(index as u16),
            false => error(format!("buffer {index} is not made")),
        }
    }

    /// The longest a delay holds: what it says, or the most any delay holds.
    fn longest(&self, longest: Option<usize>) -> Result<f32, GraphError> {
        let Some(longest) = longest else {
            return Ok(MAX_DELAY_MS);
        };
        self.read(longest)?;
        match self.graph.nodes.get(longest) {
            Some(Node::Constant(Constant { value })) if *value > 0.0 && *value <= MAX_DELAY_MS => {
                Ok(*value)
            }
            _ => error(format!(
                "delay(x, ms, longest): longest is a number of ms above 0 and up to {MAX_DELAY_MS}"
            )),
        }
    }

    /// The register of `node`: of the operation it makes, or of its field or control.
    fn node(&mut self, node: &Node) -> Result<Register, GraphError> {
        let operation = match node {
            // JSON gives an f64; one beyond f32 arrives as infinity.
            Node::Constant(Constant { value }) if !value.is_finite() => {
                return error(format!(
                    "a number of a sound is at most {:e} either way",
                    f32::MAX
                ));
            }
            Node::Constant(Constant { value }) => Operation::Constant(*value),
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
            Node::Param(Param { name }) => {
                return match self.names.get(name.as_str()) {
                    Some(register) => Ok(*register),
                    None => error(format!("`{name}` is no field or control of the tool")),
                };
            }
            Node::Feedback(Feedback { slot }) if *slot < self.graph.feedbacks.len() => {
                Operation::History(*slot as u16)
            }
            Node::Feedback(Feedback { slot }) => {
                return error(format!("feedback {slot} is never set"));
            }
            Node::Negate(x) => self.unary(Unary::Negate, x)?,
            Node::Sin(x) => self.unary(Unary::Sin, x)?,
            Node::Cos(x) => self.unary(Unary::Cos, x)?,
            Node::Tan(x) => self.unary(Unary::Tan, x)?,
            Node::Tanh(x) => self.unary(Unary::Tanh, x)?,
            Node::Abs(x) => self.unary(Unary::Abs, x)?,
            Node::Sqrt(x) => self.unary(Unary::Sqrt, x)?,
            Node::Exp(x) => self.unary(Unary::Exp, x)?,
            Node::Log(x) => self.unary(Unary::Log, x)?,
            Node::Floor(x) => self.unary(Unary::Floor, x)?,
            Node::Wrap(x) => self.unary(Unary::Wrap, x)?,
            Node::Db(x) => self.unary(Unary::Decibels, x)?,
            Node::Saturate(x) => self.unary(Unary::Saturate, x)?,
            Node::Add(two) => self.binary(Binary::Add, two)?,
            Node::Subtract(two) => self.binary(Binary::Subtract, two)?,
            Node::Multiply(two) => self.binary(Binary::Multiply, two)?,
            Node::Divide(two) => self.binary(Binary::Divide, two)?,
            Node::Remainder(two) => self.binary(Binary::Remainder, two)?,
            Node::Less(two) => self.binary(Binary::Less, two)?,
            Node::Greater(two) => self.binary(Binary::Greater, two)?,
            Node::LessOrEqual(two) => self.binary(Binary::LessOrEqual, two)?,
            Node::GreaterOrEqual(two) => self.binary(Binary::GreaterOrEqual, two)?,
            Node::Equal(two) => self.binary(Binary::Equal, two)?,
            Node::NotEqual(two) => self.binary(Binary::NotEqual, two)?,
            Node::Min(two) => self.binary(Binary::Min, two)?,
            Node::Max(two) => self.binary(Binary::Max, two)?,
            Node::Pow(two) => self.binary(Binary::Power, two)?,
            Node::Clamp(Clamp { x, low, high }) => {
                Operation::Clamp(self.read(*x)?, self.read(*low)?, self.read(*high)?)
            }
            Node::Mix(Mix { a, b, amount }) => {
                Operation::Mix(self.read(*a)?, self.read(*b)?, self.read(*amount)?)
            }
            Node::Phasor(Phasor { hz }) => Operation::Phasor {
                hz: self.read(*hz)?,
                slot: next(&mut self.slots.phasors),
            },
            Node::Noise => Operation::Noise {
                slot: next(&mut self.slots.noises),
            },
            Node::Delay(Delay { x, ms, longest }) => {
                let (input, ms) = (self.read(*x)?, self.read(*ms)?);
                if self.slots.delays.len() == MAX_DELAYS {
                    return error(format!("a sound has at most {MAX_DELAYS} delays"));
                }
                let slot = self.slots.delays.len() as u16;
                self.slots.delays.push(self.longest(*longest)?);
                Operation::Delay { input, ms, slot }
            }
            Node::Lowpass(filter) => self.filter(FilterKind::LowPass, filter)?,
            Node::Highpass(filter) => self.filter(FilterKind::HighPass, filter)?,
            Node::Bandpass(filter) => self.filter(FilterKind::BandPass, filter)?,
            Node::Smooth(Smooth { x, ms }) => Operation::Smooth {
                input: self.read(*x)?,
                ms: self.read(*ms)?,
                slot: next(&mut self.slots.smooths),
            },
            Node::Adsr(Adsr {
                gate,
                attack,
                decay,
                sustain,
                release,
            }) => Operation::Envelope {
                gate: self.read(*gate)?,
                attack: self.read(*attack)?,
                decay: self.read(*decay)?,
                sustain: self.read(*sustain)?,
                release: self.read(*release)?,
                slot: next(&mut self.slots.envelopes),
            },
            Node::Hold(Hold { x, when }) => Operation::Hold {
                input: self.read(*x)?,
                when: self.read(*when)?,
                slot: next(&mut self.slots.memories),
            },
            Node::Rise(One { x }) => Operation::Rise {
                input: self.read(*x)?,
                slot: next(&mut self.slots.memories),
            },
            Node::Change(One { x }) => Operation::Change {
                input: self.read(*x)?,
                slot: next(&mut self.slots.memories),
            },
            Node::At(At { table, index }) => Operation::Read {
                table: self.table(table)?,
                index: self.read(*index)?,
            },
            Node::Lookup(Lookup { table, phase }) => Operation::Lookup {
                table: self.table(table)?,
                phase: self.read(*phase)?,
            },
            Node::Length(Length { table }) => Operation::Length(self.table(table)?),
            Node::Write(Write {
                buffer,
                index,
                value,
            }) => Operation::Write {
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
