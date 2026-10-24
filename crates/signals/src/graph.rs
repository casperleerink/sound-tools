//! The sound graph the SDK of a tool sends as JSON, and its lowering to [`Code`], the flat list
//! of operations a [`Machine`](crate::Machine) runs.
//!
//! The nodes come in order, each reading nodes before it by their index, so one pass in order
//! lowers them: a node is one operation, but a field or a control reads the operation made for
//! it at the start.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::Deserialize;
use sound_core::SvfShape;

use crate::code::{
    ArraySpec, BinValue, Binary, Code, MAX_ARRAY_LENGTH, MAX_BUFFER_SECONDS, MAX_DELAY_MS,
    MAX_DELAYS, MAX_LIVES, MAX_NUMBERS, MAX_OPERATIONS, MAX_OUTPUTS, MAX_PARAMETERS, MAX_SPECTRA,
    MAX_WATCHES, Operation, Output, ParameterSpec, Register, SPECTRUM_SIZES, Slots, SpectrumSpec,
    Table, Unary,
};
use crate::program;

/// A sound as the SDK sends it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub(crate) nodes: Vec<Node>,
    /// The node each `feedback` is set to, by its slot.
    pub(crate) feedbacks: Vec<usize>,
    /// The seconds of each `buffer`.
    pub(crate) buffers: Vec<f32>,
    pub(crate) watches: Vec<NamedNode>,
    pub(crate) numbers: Vec<NamedNode>,
    pub(crate) bangs: Vec<NamedNode>,
    pub(crate) notes: Vec<NotesNode>,
    pub(crate) output: OutputNode,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedNode {
    pub(crate) name: String,
    pub(crate) node: usize,
}

/// Notes a sound sends out: one sounds while `gate` is above 0, at the MIDI `pitch`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NotesNode {
    pub(crate) name: String,
    pub(crate) gate: usize,
    pub(crate) pitch: usize,
    pub(crate) velocity: usize,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum OutputNode {
    /// Runs on each channel apart.
    Mono { mono: usize },
    /// Runs once for both channels, and hears both.
    Stereo { left: usize, right: usize },
}

/// Where a table reads: a pattern or sample field by name, or a buffer by its number.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum TableNode {
    List(String),
    Buffer(usize),
}

/// A node of the graph. Each `usize` is the index of a node before it.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Node {
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
    Notch(Filter),
    Allpass(Filter),
    Peak(Gained),
    Lowshelf(Gained),
    Highshelf(Gained),
    Smooth(Smooth),
    Adsr(Adsr),
    Hold(Hold),
    Rise(One),
    Change(One),
    At(At),
    Lookup(Lookup),
    Length(Length),
    Write(Write),
    Spectral(Spectral),
    /// What a bin is, in the graph of a `spectral`.
    Magnitude,
    Phase,
    Hz,
    Index,
    /// What the bin of the second input is, in a `spectral` of two.
    SideMagnitude,
    SidePhase,
    /// A signal of the sound around a `spectral`, by its place in [`Bins::captures`].
    Capture(Capture),
}

/// The graph that runs once per bin of a `spectral`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bins {
    pub(crate) nodes: Vec<Node>,
    pub(crate) feedbacks: Vec<usize>,
    /// The node of the sound around it that each capture reads.
    pub(crate) captures: Vec<usize>,
    pub(crate) output: BinOutput,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BinOutput {
    pub(crate) magnitude: usize,
    pub(crate) phase: usize,
}

/// The fields of the nodes, as the SDK names them.
macro_rules! fields {
    ($($name:ident { $($field:ident: $type:ty),* })*) => {$(
        #[derive(Clone, Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $name { $(pub(crate) $field: $type),* }
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
    Gained { x: usize, hz: usize, db: usize, q: usize }
    Smooth { x: usize, ms: usize }
    Adsr { gate: usize, attack: usize, decay: usize, sustain: usize, release: usize }
    Hold { x: usize, when: usize }
    At { table: TableNode, index: usize }
    Lookup { table: TableNode, phase: usize }
    Length { table: TableNode }
    Write { buffer: usize, index: usize, value: usize }
    Spectral { x: usize, side: Option<usize>, size: usize, bins: Bins }
    Capture { index: usize }
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

impl Declarations {
    /// Whether a sound can take these: how many there are, and each range and length. A sound
    /// checks them as it compiles; a tool checks them before it is defined.
    pub fn check(&self) -> Result<(), GraphError> {
        if self.parameters.len() > MAX_PARAMETERS {
            return error(format!(
                "a tool has at most {MAX_PARAMETERS} knobs and toggles"
            ));
        }
        for parameter in &self.parameters {
            check_range(
                &parameter.name,
                parameter.default,
                parameter.min,
                parameter.max,
            )?;
        }
        for array in &self.arrays {
            if !(1..=MAX_ARRAY_LENGTH).contains(&array.length.unwrap_or(1)) {
                return error(format!(
                    "`{}`: a pattern holds 1 to {MAX_ARRAY_LENGTH} values",
                    array.name
                ));
            }
            check_range(&array.name, array.default, array.min, array.max)?;
        }
        let (mut lives, mut triggers) = (0, 0);
        for control in &self.controls {
            match control {
                ControlSpec::Live(live) => {
                    lives += 1;
                    check_range(&live.name, live.default, live.min, live.max)?;
                }
                ControlSpec::Trigger(_) => triggers += 1,
            }
        }
        if lives > MAX_LIVES {
            return error(format!("a tool has at most {MAX_LIVES} live controls"));
        }
        if triggers > MAX_LIVES {
            return error(format!("a tool has at most {MAX_LIVES} triggers"));
        }
        Ok(())
    }
}

/// Why a sound does not build, in the words of the SDK.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct GraphError(pub(crate) String);

fn error<T>(message: impl Into<String>) -> Result<T, GraphError> {
    Err(GraphError(message.into()))
}

impl Graph {
    /// The names of the fields and inputs it reads.
    pub fn params(&self) -> std::collections::BTreeSet<&str> {
        (self.nodes.iter())
            .filter_map(|node| match node {
                Node::Param(Param { name }) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn compile(&self, declarations: &Declarations) -> Result<Code, GraphError> {
        // Each is read by an operation, so more of them than operations is no sound.
        if self.feedbacks.len() > MAX_OPERATIONS || self.buffers.len() > MAX_OPERATIONS {
            return error(format!(
                "a sound has at most {MAX_OPERATIONS} feedbacks and buffers"
            ));
        }
        let mut lowering = Lowering::new(&self.nodes, self.feedbacks.len(), &declarations.arrays);
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
        let feedbacks = (self.feedbacks.iter())
            .map(|node| lowering.read(*node))
            .collect::<Result<_, _>>()?;
        let (watches, watch_registers) =
            lowering.named(&self.watches, MAX_WATCHES, ("watch", "watches"))?;
        let (numbers, number_registers) =
            lowering.named(&self.numbers, MAX_NUMBERS, ("number", "numbers"))?;
        let (bangs, mut event_registers) =
            lowering.named(&self.bangs, MAX_OUTPUTS, ("bang", "bangs"))?;
        if self.notes.len() > MAX_OUTPUTS {
            return error(format!("a sound has at most {MAX_OUTPUTS} notes"));
        }
        let mut notes = Vec::new();
        for NotesNode {
            name,
            gate,
            pitch,
            velocity,
        } in &self.notes
        {
            notes.push(name.clone());
            for node in [gate, pitch, velocity] {
                event_registers.push(lowering.read(*node)?);
            }
        }
        // They are the outputs of one tool, next to its sound.
        let mut outputs: Vec<&str> = (numbers.iter().chain(&bangs).chain(&notes))
            .map(String::as_str)
            .collect();
        if outputs.contains(&"audio") {
            return error("output `audio` is the sound itself: give it another name");
        }
        outputs.sort();
        if let Some(twice) = outputs.windows(2).find(|pair| pair[0] == pair[1]) {
            return error(format!("output `{}` is given twice", twice[0]));
        }
        let output = match self.output {
            OutputNode::Mono { mono } => Output::Mono(lowering.read(mono)?),
            OutputNode::Stereo { left, right } => {
                Output::Stereo(lowering.read(left)?, lowering.read(right)?)
            }
        };
        let mut hasher = DefaultHasher::new();
        format!("{self:?}{declarations:?}").hash(&mut hasher);
        let mut code = Code {
            parameters: declarations.parameters.clone(),
            arrays: declarations.arrays.clone(),
            lives: lowering.lives,
            triggers: lowering.triggers,
            watches,
            numbers,
            bangs,
            notes,
            hash: hasher.finish(),
            latency: 0,
            operations: lowering.operations,
            output,
            feedbacks,
            watch_registers,
            number_registers,
            event_registers,
            slots: lowering.slots,
        };
        code.latency = program::latency(&code);
        Ok(code)
    }
}

impl Bins {
    /// The code of one bin. It reads the lists of the tool, but no buffer: each bin would hold
    /// its own.
    fn compile(&self, arrays: &[ArraySpec]) -> Result<Code, GraphError> {
        if self.captures.len() > MAX_PARAMETERS {
            return error(format!(
                "spectral: the bins read at most {MAX_PARAMETERS} signals of the sound around them"
            ));
        }
        let mut lowering = Lowering::new(&self.nodes, self.feedbacks.len(), arrays);
        lowering.captures = Some(self.captures.len());
        for node in &self.nodes {
            let register = lowering.node(node)?;
            lowering.registers.push(register);
        }
        let feedbacks = (self.feedbacks.iter())
            .map(|node| lowering.read(*node))
            .collect::<Result<_, _>>()?;
        let BinOutput { magnitude, phase } = self.output;
        Ok(Code {
            parameters: Vec::new(),
            arrays: Vec::new(),
            lives: Vec::new(),
            triggers: Vec::new(),
            watches: Vec::new(),
            numbers: Vec::new(),
            bangs: Vec::new(),
            notes: Vec::new(),
            hash: 0,
            latency: 0,
            output: Output::Bin(lowering.read(magnitude)?, lowering.read(phase)?),
            operations: lowering.operations,
            feedbacks,
            watch_registers: Vec::new(),
            number_registers: Vec::new(),
            event_registers: Vec::new(),
            slots: lowering.slots,
        })
    }
}

struct Lowering<'a> {
    nodes: &'a [Node],
    /// How many feedbacks the graph sets.
    feedbacks: usize,
    arrays: &'a [ArraySpec],
    /// In the graph of the bins of a `spectral`, how many signals of the sound around it they
    /// read.
    captures: Option<usize>,
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
    fn new(nodes: &'a [Node], feedbacks: usize, arrays: &'a [ArraySpec]) -> Self {
        Self {
            nodes,
            feedbacks,
            arrays,
            captures: None,
            operations: Vec::new(),
            slots: Slots {
                histories: feedbacks as u16,
                ..Slots::default()
            },
            lives: Vec::new(),
            triggers: Vec::new(),
            names: HashMap::new(),
            registers: Vec::with_capacity(nodes.len()),
        }
    }

    /// The operations of the fields and controls, which come first.
    fn declare(&mut self, declarations: &'a Declarations) -> Result<(), GraphError> {
        declarations.check()?;
        for (index, parameter) in declarations.parameters.iter().enumerate() {
            let register = self.emit(Operation::Parameter(index as u16))?;
            self.names.insert(&parameter.name, register);
        }
        for control in &declarations.controls {
            let (name, operation) = match control {
                ControlSpec::Live(live) => {
                    let index = self.lives.len() as u16;
                    self.lives.push(live.clone());
                    (&live.name, Operation::Live(index))
                }
                ControlSpec::Trigger(name) => {
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

    /// The names of `nodes` and the register each reads, at most `most` of them, each name once.
    fn named(
        &self,
        nodes: &[NamedNode],
        most: usize,
        (one, many): (&str, &str),
    ) -> Result<(Vec<String>, Vec<Register>), GraphError> {
        if nodes.len() > most {
            return error(format!("a sound has at most {most} {many}"));
        }
        let mut names: Vec<String> = Vec::new();
        for node in nodes {
            if names.contains(&node.name) {
                return error(format!("{one} `{}` is given twice", node.name));
            }
            names.push(node.name.clone());
        }
        let registers = nodes.iter().map(|node| self.read(node.node));
        Ok((names, registers.collect::<Result<_, _>>()?))
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

    fn filter(&mut self, kind: SvfShape, filter: &Filter) -> Result<Operation, GraphError> {
        let q = self.read(filter.q)?;
        Ok(Operation::Filter {
            kind,
            input: self.read(filter.x)?,
            hz: self.read(filter.hz)?,
            q,
            gain: q,
            slot: next(&mut self.slots.filters),
        })
    }

    fn gained(&mut self, kind: SvfShape, gained: &Gained) -> Result<Operation, GraphError> {
        Ok(Operation::Filter {
            kind,
            input: self.read(gained.x)?,
            hz: self.read(gained.hz)?,
            q: self.read(gained.q)?,
            gain: self.read(gained.db)?,
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
        match self.nodes.get(longest) {
            Some(Node::Constant(Constant { value })) if *value > 0.0 && *value <= MAX_DELAY_MS => {
                Ok(*value)
            }
            _ => error(format!(
                "delay(x, ms, longest): longest is a number of ms above 0 and up to {MAX_DELAY_MS}"
            )),
        }
    }

    /// Whether `node` may be where it is: what reads the sound around it only around the bins,
    /// what reads a bin only in them.
    fn check_place(&self, node: &Node) -> Result<(), GraphError> {
        let around = match node {
            Node::Input => Some("input"),
            Node::InputLeft | Node::InputRight => Some("inputLeft and inputRight"),
            Node::Channel => Some("channel"),
            Node::SampleRate => Some("sampleRate"),
            Node::Beat | Node::Bpm | Node::Playing => Some("the transport"),
            Node::Frequency | Node::Pitch | Node::Gate | Node::Velocity | Node::Onset => {
                Some("the note")
            }
            Node::Param(_) => Some("a field"),
            Node::Delay(_) => Some("delay"),
            Node::Spectral(_) => Some("spectral"),
            _ => None,
        };
        let in_bins = matches!(
            node,
            Node::Magnitude
                | Node::Phase
                | Node::Hz
                | Node::Index
                | Node::SideMagnitude
                | Node::SidePhase
                | Node::Capture(_)
        );
        match (self.captures.is_some(), around, in_bins) {
            (true, Some(what), _) => error(format!(
                "spectral: a bin reads {what} only as a signal of the sound around it"
            )),
            (false, _, true) => error("a bin is read only inside its spectral"),
            _ => Ok(()),
        }
    }

    fn spectral(&mut self, spectral: &Spectral) -> Result<Operation, GraphError> {
        let Spectral {
            x,
            side,
            size,
            bins,
        } = spectral;
        let input = self.read(*x)?;
        let side = side.map(|side| self.read(side)).transpose()?;
        if !SPECTRUM_SIZES.contains(size) || !size.is_power_of_two() {
            return error(format!(
                "spectral(x, {size}): the size is a power of two from {} to {}",
                SPECTRUM_SIZES.start(),
                SPECTRUM_SIZES.end()
            ));
        }
        if self.slots.spectra.len() == MAX_SPECTRA {
            return error(format!(
                "a sound calls spectral at most {MAX_SPECTRA} times"
            ));
        }
        let captures = (bins.captures.iter())
            .map(|node| self.read(*node))
            .collect::<Result<_, _>>()?;
        let slot = self.slots.spectra.len() as u16;
        self.slots.spectra.push(SpectrumSpec {
            size: *size,
            side: side.is_some(),
            code: bins.compile(self.arrays)?,
            captures,
        });
        Ok(Operation::Spectral { input, side, slot })
    }

    /// The register of `node`: of the operation it makes, or of its field or control.
    fn node(&mut self, node: &Node) -> Result<Register, GraphError> {
        self.check_place(node)?;
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
            Node::Feedback(Feedback { slot }) if *slot < self.feedbacks => {
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
            Node::Lowpass(filter) => self.filter(SvfShape::LowPass, filter)?,
            Node::Highpass(filter) => self.filter(SvfShape::HighPass, filter)?,
            Node::Bandpass(filter) => self.filter(SvfShape::BandPass, filter)?,
            Node::Notch(filter) => self.filter(SvfShape::Notch, filter)?,
            Node::Allpass(filter) => self.filter(SvfShape::AllPass, filter)?,
            Node::Peak(gained) => self.gained(SvfShape::Peak, gained)?,
            Node::Lowshelf(gained) => self.gained(SvfShape::LowShelf, gained)?,
            Node::Highshelf(gained) => self.gained(SvfShape::HighShelf, gained)?,
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
            Node::Spectral(spectral) => self.spectral(spectral)?,
            Node::Magnitude => Operation::Bin(BinValue::Magnitude),
            Node::Phase => Operation::Bin(BinValue::Phase),
            Node::Hz => Operation::Bin(BinValue::Hz),
            Node::Index => Operation::Bin(BinValue::Index),
            Node::SideMagnitude => Operation::Bin(BinValue::SideMagnitude),
            Node::SidePhase => Operation::Bin(BinValue::SidePhase),
            Node::Capture(Capture { index }) => match index < &self.captures.unwrap_or(0) {
                true => Operation::Parameter(*index as u16),
                false => return error(format!("capture {index} is not made")),
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
