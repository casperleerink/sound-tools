//! The language: one statement per line, compiled in one pass to a flat list of operations
//! that [`Machine`](crate::Machine) runs once per frame and channel.
//!
//! Every operation writes the register with its own index, so an expression is the register
//! of its last operation and a name is bound to a register. An operation only reads registers
//! before it, so one pass in order computes a frame. A `history` reads what it was set to in
//! the frame before, which is how a value feeds back.
//!
//! An error names the line of the code, so whoever reads the code finds it.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

/// The most params, and so the most knobs and toggles: the update carries their values in a
/// fixed array.
pub const MAX_PARAMETERS: usize = 32;
/// The most `live` controls and the most triggers: a trigger is one bit of a `u32`.
pub const MAX_LIVES: usize = 32;
/// The most watches.
pub const MAX_WATCHES: usize = 16;
/// The longest a `delay` can be.
const MAX_DELAY_MS: f32 = 4000.0;
/// Each delay holds up to [`MAX_DELAY_MS`] per channel and voice, so their number is held too.
const MAX_DELAYS: usize = 16;
/// The seconds of all the buffers of one code together, per channel and voice.
const MAX_BUFFER_SECONDS: f32 = 30.0;
/// The longest list a `param` can be.
const MAX_ARRAY_LENGTH: usize = 1024;
const MAX_OPERATIONS: usize = 4096;

/// The index of an operation, and of the register it writes.
pub(crate) type Register = u16;

/// What is wrong with code, on which line.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("code[{line}]: {message}")]
pub struct CompileError {
    /// The index of the line, from 0.
    pub line: usize,
    pub message: String,
}

/// A `param` or a `live` line: a number glided so a change does not click.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterSpec {
    pub name: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

/// A `param name[length]` line: a list the record sets, such as the steps of a sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct ArraySpec {
    pub name: String,
    pub length: usize,
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

/// Compiled code.
#[derive(Clone, Debug)]
pub struct Code {
    /// Numbers the record sets, in the order of their lines.
    pub parameters: Vec<ParameterSpec>,
    /// Lists the record sets.
    pub arrays: Vec<ArraySpec>,
    /// Numbers the interface plays and nothing saves.
    pub lives: Vec<ParameterSpec>,
    /// Names of the triggers, which the interface fires.
    pub triggers: Vec<String>,
    /// Names of the values the interface reads.
    pub watches: Vec<String>,
    /// Of the source lines, so a behaviour tells new code from new values.
    pub hash: u64,
    pub(crate) operations: Vec<Operation>,
    /// The register of `out`, `None` when the code never sets it and passes its input.
    pub(crate) output: Option<Register>,
    /// What each `history` takes for the next frame: the slot and the register.
    pub(crate) history_writes: Vec<(u16, Register)>,
    /// The register of each watch, in the order of [`Self::watches`].
    pub(crate) watch_registers: Vec<Register>,
    pub(crate) slots: Slots,
}

/// How much of each kind of memory the operations use, per channel.
#[derive(Clone, Debug, Default)]
pub(crate) struct Slots {
    pub(crate) histories: u16,
    pub(crate) phasors: u16,
    pub(crate) noises: u16,
    /// The longest each delay reaches, in milliseconds.
    pub(crate) delays: Vec<f32>,
    pub(crate) filters: u16,
    pub(crate) smooths: u16,
    pub(crate) envelopes: u16,
    /// One number each, for `hold`, `rise` and `change`.
    pub(crate) memories: u16,
    /// The seconds of each buffer.
    pub(crate) buffers: Vec<f32>,
}

/// Where `lookup`, `len` and `name[i]` read: a list of the record, or a buffer.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Table {
    Array(u16),
    Buffer(u16),
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Operation {
    Constant(f32),
    Input,
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
    Parameter(u16),
    Live(u16),
    Trigger(u16),
    History(u16),
    Unary(Unary, Register),
    Binary(Binary, Register, Register),
    Clamp(Register, Register, Register),
    Mix(Register, Register, Register),
    Phasor {
        hz: Register,
        slot: u16,
    },
    Noise {
        slot: u16,
    },
    Delay {
        input: Register,
        ms: Register,
        slot: u16,
    },
    Filter {
        kind: FilterKind,
        input: Register,
        hz: Register,
        q: Register,
        slot: u16,
    },
    Smooth {
        input: Register,
        ms: Register,
        slot: u16,
    },
    Envelope {
        gate: Register,
        attack: Register,
        decay: Register,
        sustain: Register,
        release: Register,
        slot: u16,
    },
    Hold {
        input: Register,
        when: Register,
        slot: u16,
    },
    Rise {
        input: Register,
        slot: u16,
    },
    Change {
        input: Register,
        slot: u16,
    },
    Read {
        table: Table,
        index: Register,
    },
    Lookup {
        table: Table,
        phase: Register,
    },
    Length(Table),
    Write {
        buffer: u16,
        index: Register,
        value: Register,
    },
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Unary {
    Negate,
    Sin,
    Cos,
    Tan,
    Tanh,
    Abs,
    Sqrt,
    Exp,
    Log,
    Floor,
    Wrap,
    Decibels,
    Saturate,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Less,
    Greater,
    LessOrEqual,
    GreaterOrEqual,
    Equal,
    NotEqual,
    Min,
    Max,
    Power,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
}

/// The functions, with what they take, for the error of a wrong call.
const FUNCTIONS: &[(&str, &str)] = &[
    ("sin", "x"),
    ("cos", "x"),
    ("tan", "x"),
    ("tanh", "x"),
    ("abs", "x"),
    ("sqrt", "x"),
    ("exp", "x"),
    ("log", "x"),
    ("floor", "x"),
    ("wrap", "x"),
    ("db", "decibels"),
    ("saturate", "x"),
    ("min", "a, b"),
    ("max", "a, b"),
    ("pow", "x, power"),
    ("clamp", "x, low, high"),
    ("mix", "a, b, amount"),
    ("phasor", "hz"),
    ("noise", ""),
    ("delay", "x, ms[, longest_ms]"),
    ("lowpass", "x, hz, q"),
    ("highpass", "x, hz, q"),
    ("bandpass", "x, hz, q"),
    ("smooth", "x, ms"),
    ("adsr", "gate, attack_ms, decay_ms, sustain, release_ms"),
    ("hold", "x, when"),
    ("rise", "x"),
    ("change", "x"),
    ("lookup", "table, phase"),
    ("len", "table"),
];

/// The names code reads but never sets.
const BUILT_IN: &[&str] = &[
    "in", "channel", "sr", "pi", "tau", "beat", "bpm", "playing", "freq", "pitch", "gate",
    "velocity", "onset",
];

/// What a line can start with besides a name.
const KEYWORDS: &[&str] = &["param", "live", "trigger", "watch", "history", "buffer"];

/// A filter's `q` when a call leaves it out: no peak.
const DEFAULT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub fn compile(lines: &[String]) -> Result<Code, CompileError> {
    let mut hasher = DefaultHasher::new();
    lines.hash(&mut hasher);
    let mut compiler = Compiler {
        code: Code {
            parameters: Vec::new(),
            arrays: Vec::new(),
            lives: Vec::new(),
            triggers: Vec::new(),
            watches: Vec::new(),
            hash: hasher.finish(),
            operations: Vec::new(),
            output: None,
            history_writes: Vec::new(),
            watch_registers: Vec::new(),
            slots: Slots::default(),
        },
        names: HashMap::new(),
        line: 0,
    };
    for (index, line) in lines.iter().enumerate() {
        compiler.line = index;
        compiler.statement(line)?;
    }
    // The first in the code, not the first in the map, so the same code gives the same error.
    let unset = (compiler.names.iter())
        .filter_map(|(name, binding)| match binding {
            Binding::History {
                set: false, line, ..
            } => Some((*line, name)),
            _ => None,
        })
        .min();
    if let Some((line, name)) = unset {
        return Err(CompileError {
            line,
            message: format!("history `{name}` is never set: set it with `{name} = ...`"),
        });
    }
    compiler.code.output = match compiler.names.get("out") {
        Some(Binding::Value(register)) => Some(*register),
        _ => None,
    };
    Ok(compiler.code)
}

#[derive(Copy, Clone)]
enum Binding {
    Value(Register),
    History { slot: u16, set: bool, line: usize },
    Table(Table),
}

struct Compiler {
    code: Code,
    names: HashMap<String, Binding>,
    line: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f32),
    Name(String),
    Symbol(&'static str),
}

const SYMBOLS: &[&str] = &[
    "<=", ">=", "==", "!=", "+", "-", "*", "/", "%", "(", ")", ",", "=", "<", ">", "[", "]",
];

impl std::fmt::Display for Token {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Number(number) => write!(formatter, "`{number}`"),
            Token::Name(name) => write!(formatter, "`{name}`"),
            Token::Symbol(symbol) => write!(formatter, "`{symbol}`"),
        }
    }
}

impl Compiler {
    fn error<T>(&self, message: impl Into<String>) -> Result<T, CompileError> {
        Err(CompileError {
            line: self.line,
            message: message.into(),
        })
    }

    fn statement(&mut self, line: &str) -> Result<(), CompileError> {
        let source = line.split("//").next().unwrap_or_default();
        let tokens = self.tokens(source)?;
        let mut tokens = Tokens {
            tokens: &tokens,
            next: 0,
        };
        let Some(first) = tokens.peek().cloned() else {
            return Ok(());
        };
        let keyword = match &first {
            Token::Name(word) if KEYWORDS.contains(&word.as_str()) => Some(word.clone()),
            Token::Name(_) => None,
            token => {
                return self.error(format!(
                    "a line starts with a name or one of {}, not {token}",
                    KEYWORDS.join(", ")
                ));
            }
        };
        match keyword.as_deref() {
            Some("param") => {
                tokens.next();
                self.parameter(&mut tokens)?;
            }
            Some("live") => {
                tokens.next();
                self.live(&mut tokens)?;
            }
            Some("trigger") => {
                tokens.next();
                let name = self.new_name(&mut tokens)?;
                if self.code.triggers.len() == MAX_LIVES {
                    return self.error(format!("code has at most {MAX_LIVES} triggers"));
                }
                let index = self.code.triggers.len() as u16;
                let register = self.emit(Operation::Trigger(index))?;
                self.code.triggers.push(name.clone());
                self.names.insert(name, Binding::Value(register));
            }
            Some("watch") => {
                tokens.next();
                let name = self.new_name(&mut tokens)?;
                if !tokens.eat("=") {
                    return self.error(format!("a watch is `watch {name} = <value>`"));
                }
                if self.code.watches.len() == MAX_WATCHES {
                    return self.error(format!("code has at most {MAX_WATCHES} watches"));
                }
                let register = self.expression(&mut tokens)?;
                self.code.watches.push(name.clone());
                self.code.watch_registers.push(register);
                self.names.insert(name, Binding::Value(register));
            }
            Some("history") => {
                tokens.next();
                let name = self.new_name(&mut tokens)?;
                let slot = next(&mut self.code.slots.histories);
                self.names.insert(
                    name,
                    Binding::History {
                        slot,
                        set: false,
                        line: self.line,
                    },
                );
            }
            Some("buffer") => {
                tokens.next();
                self.buffer(&mut tokens)?;
            }
            _ => self.assignment(&mut tokens)?,
        }
        match tokens.next() {
            None => Ok(()),
            Some(token) => self.error(format!("unexpected {token} at the end of the line")),
        }
    }

    fn tokens(&self, source: &str) -> Result<Vec<Token>, CompileError> {
        let mut tokens = Vec::new();
        let mut rest = source.trim_start();
        while let Some(first) = rest.chars().next() {
            if first.is_ascii_digit() || first == '.' {
                let end = rest
                    .find(|character: char| {
                        !(character.is_ascii_alphanumeric() || character == '.')
                    })
                    .unwrap_or(rest.len());
                let text = &rest[..end];
                let Ok(number) = text.parse::<f32>() else {
                    return self.error(format!("`{text}` is not a number"));
                };
                tokens.push(Token::Number(number));
                rest = &rest[end..];
            } else if first.is_ascii_alphabetic() || first == '_' {
                let end = rest
                    .find(|character: char| {
                        !(character.is_ascii_alphanumeric() || character == '_')
                    })
                    .unwrap_or(rest.len());
                tokens.push(Token::Name(rest[..end].to_string()));
                rest = &rest[end..];
            } else if let Some(symbol) = SYMBOLS.iter().find(|symbol| rest.starts_with(**symbol)) {
                tokens.push(Token::Symbol(symbol));
                rest = &rest[symbol.len()..];
            } else {
                return self.error(format!("`{first}` is not part of the language"));
            }
            rest = rest.trim_start();
        }
        Ok(tokens)
    }

    /// A number written in the code, with an optional `-`.
    fn literal(&self, tokens: &mut Tokens<'_>, form: &str) -> Result<f32, CompileError> {
        let negative = tokens.eat("-");
        match tokens.next() {
            Some(Token::Number(number)) if negative => Ok(-number),
            Some(Token::Number(number)) => Ok(*number),
            _ => self.error(form.to_string()),
        }
    }

    /// `= <default> [<min>, <max>]`, checked.
    fn range(
        &self,
        name: &str,
        tokens: &mut Tokens<'_>,
        form: &str,
    ) -> Result<(f32, f32, f32), CompileError> {
        if !tokens.eat("=") {
            return self.error(form.to_string());
        }
        let default = self.literal(tokens, form)?;
        if !tokens.eat("[") {
            return self.error(form.to_string());
        }
        let min = self.literal(tokens, form)?;
        if !tokens.eat(",") {
            return self.error(form.to_string());
        }
        let max = self.literal(tokens, form)?;
        if !tokens.eat("]") {
            return self.error(form.to_string());
        }
        if min >= max {
            return self.error(format!("`{name}`: the range [{min}, {max}] is empty"));
        }
        if !(min..=max).contains(&default) {
            return self.error(format!(
                "`{name}`: the default {default} is outside [{min}, {max}]"
            ));
        }
        Ok((default, min, max))
    }

    /// `param <name> = <default> [<min>, <max>]`, or `param <name>[<length>] = ...` for a list.
    fn parameter(&mut self, tokens: &mut Tokens<'_>) -> Result<(), CompileError> {
        const FORM: &str = "a param is `param <name> = <default> [<min>, <max>]`, or `param <name>[<length>] = <default> [<min>, <max>]` for a list";
        let name = self.new_name(tokens)?;
        if tokens.eat("[") {
            let length = self.literal(tokens, FORM)?;
            if !tokens.eat("]") {
                return self.error(FORM);
            }
            if length < 1.0 || length.fract() != 0.0 || length as usize > MAX_ARRAY_LENGTH {
                return self.error(format!(
                    "param `{name}`: a list holds 1 to {MAX_ARRAY_LENGTH} values"
                ));
            }
            let (default, min, max) = self.range(&name, tokens, FORM)?;
            let index = self.code.arrays.len() as u16;
            self.code.arrays.push(ArraySpec {
                name: name.clone(),
                length: length as usize,
                default,
                min,
                max,
            });
            self.names.insert(name, Binding::Table(Table::Array(index)));
            return Ok(());
        }
        let (default, min, max) = self.range(&name, tokens, FORM)?;
        if self.code.parameters.len() == MAX_PARAMETERS {
            return self.error(format!("code has at most {MAX_PARAMETERS} params"));
        }
        let index = self.code.parameters.len() as u16;
        let register = self.emit(Operation::Parameter(index))?;
        let spec = ParameterSpec {
            name: name.clone(),
            default,
            min,
            max,
        };
        self.code.parameters.push(spec);
        self.names.insert(name, Binding::Value(register));
        Ok(())
    }

    /// `live <name> = <default> [<min>, <max>]`
    fn live(&mut self, tokens: &mut Tokens<'_>) -> Result<(), CompileError> {
        const FORM: &str = "a live control is `live <name> = <default> [<min>, <max>]`";
        let name = self.new_name(tokens)?;
        let (default, min, max) = self.range(&name, tokens, FORM)?;
        if self.code.lives.len() == MAX_LIVES {
            return self.error(format!("code has at most {MAX_LIVES} live controls"));
        }
        let index = self.code.lives.len() as u16;
        let register = self.emit(Operation::Live(index))?;
        let spec = ParameterSpec {
            name: name.clone(),
            default,
            min,
            max,
        };
        self.code.lives.push(spec);
        self.names.insert(name, Binding::Value(register));
        Ok(())
    }

    /// `buffer <name> = <seconds>`
    fn buffer(&mut self, tokens: &mut Tokens<'_>) -> Result<(), CompileError> {
        const FORM: &str = "a buffer is `buffer <name> = <seconds>`";
        let name = self.new_name(tokens)?;
        if !tokens.eat("=") {
            return self.error(FORM);
        }
        let seconds = self.literal(tokens, FORM)?;
        let total: f32 = self.code.slots.buffers.iter().sum::<f32>() + seconds;
        if seconds <= 0.0 || total > MAX_BUFFER_SECONDS {
            return self.error(format!(
                "buffer `{name}`: the buffers of code hold up to {MAX_BUFFER_SECONDS} s together"
            ));
        }
        let index = self.code.slots.buffers.len() as u16;
        self.code.slots.buffers.push(seconds);
        self.names
            .insert(name, Binding::Table(Table::Buffer(index)));
        Ok(())
    }

    /// `<name> = <expression>`, or `<buffer>[<index>] = <expression>`. A name is set once; a
    /// `history` is set once after its line.
    fn assignment(&mut self, tokens: &mut Tokens<'_>) -> Result<(), CompileError> {
        let Some(Token::Name(name)) = tokens.next().cloned() else {
            return self.error("a line starts with a name");
        };
        if tokens.eat("[") {
            let buffer = match self.names.get(&name) {
                Some(Binding::Table(Table::Buffer(buffer))) => *buffer,
                Some(Binding::Table(Table::Array(_))) => {
                    return self.error(format!(
                        "`{name}` is a list the record sets: code reads it and cannot write it"
                    ));
                }
                _ => {
                    return self.error(format!(
                        "`{name}` is no buffer: declare `buffer {name} = <seconds>` first"
                    ));
                }
            };
            let index = self.expression(tokens)?;
            if !tokens.eat("]") || !tokens.eat("=") {
                return self.error(format!("a write is `{name}[<index>] = <value>`"));
            }
            let value = self.expression(tokens)?;
            self.emit(Operation::Write {
                buffer,
                index,
                value,
            })?;
            return Ok(());
        }
        if !tokens.eat("=") {
            return self.error(format!("`{name}` is not set: write `{name} = ...`"));
        }
        if BUILT_IN.contains(&name.as_str()) {
            return self.error(format!("`{name}` is built in and cannot be set"));
        }
        let register = self.expression(tokens)?;
        match self.names.get_mut(&name) {
            None => {
                self.names.insert(name, Binding::Value(register));
            }
            Some(Binding::History { slot, set, .. }) if !*set => {
                *set = true;
                self.code.history_writes.push((*slot, register));
            }
            Some(_) => {
                return self.error(format!(
                    "`{name}` is set twice: a name is set once. For a value that feeds back, read it from the last frame with `history {name}`"
                ));
            }
        }
        Ok(())
    }

    fn new_name(&self, tokens: &mut Tokens<'_>) -> Result<String, CompileError> {
        let Some(Token::Name(name)) = tokens.next().cloned() else {
            return self.error("a name must follow");
        };
        if BUILT_IN.contains(&name.as_str()) || name == "out" || KEYWORDS.contains(&name.as_str()) {
            return self.error(format!("`{name}` is built in"));
        }
        if self.names.contains_key(&name) {
            return self.error(format!("`{name}` is already used"));
        }
        Ok(name)
    }

    fn emit(&mut self, operation: Operation) -> Result<Register, CompileError> {
        if self.code.operations.len() == MAX_OPERATIONS {
            return self.error(format!("code has at most {MAX_OPERATIONS} operations"));
        }
        self.code.operations.push(operation);
        Ok((self.code.operations.len() - 1) as Register)
    }

    /// A comparison, which is 1 when true and 0 when false.
    fn expression(&mut self, tokens: &mut Tokens<'_>) -> Result<Register, CompileError> {
        let left = self.sum(tokens)?;
        let comparison = [
            ("<", Binary::Less),
            (">", Binary::Greater),
            ("<=", Binary::LessOrEqual),
            (">=", Binary::GreaterOrEqual),
            ("==", Binary::Equal),
            ("!=", Binary::NotEqual),
        ]
        .into_iter()
        .find(|(symbol, _)| tokens.peek() == Some(&Token::Symbol(symbol)));
        let Some((_, binary)) = comparison else {
            return Ok(left);
        };
        tokens.next();
        let right = self.sum(tokens)?;
        self.emit(Operation::Binary(binary, left, right))
    }

    fn sum(&mut self, tokens: &mut Tokens<'_>) -> Result<Register, CompileError> {
        let mut left = self.product(tokens)?;
        loop {
            let binary = if tokens.eat("+") {
                Binary::Add
            } else if tokens.eat("-") {
                Binary::Subtract
            } else {
                return Ok(left);
            };
            let right = self.product(tokens)?;
            left = self.emit(Operation::Binary(binary, left, right))?;
        }
    }

    fn product(&mut self, tokens: &mut Tokens<'_>) -> Result<Register, CompileError> {
        let mut left = self.unary(tokens)?;
        loop {
            let binary = if tokens.eat("*") {
                Binary::Multiply
            } else if tokens.eat("/") {
                Binary::Divide
            } else if tokens.eat("%") {
                Binary::Remainder
            } else {
                return Ok(left);
            };
            let right = self.unary(tokens)?;
            left = self.emit(Operation::Binary(binary, left, right))?;
        }
    }

    fn unary(&mut self, tokens: &mut Tokens<'_>) -> Result<Register, CompileError> {
        if tokens.eat("-") {
            let value = self.unary(tokens)?;
            return self.emit(Operation::Unary(Unary::Negate, value));
        }
        self.primary(tokens)
    }

    fn primary(&mut self, tokens: &mut Tokens<'_>) -> Result<Register, CompileError> {
        match tokens.next().cloned() {
            Some(Token::Number(number)) => self.emit(Operation::Constant(number)),
            Some(Token::Symbol("(")) => {
                let value = self.expression(tokens)?;
                if !tokens.eat(")") {
                    return self.error("a `(` is not closed");
                }
                Ok(value)
            }
            Some(Token::Name(name)) if tokens.eat("(") => self.call(&name, tokens),
            Some(Token::Name(name)) if tokens.eat("[") => {
                let table = self.table(&name)?;
                let index = self.expression(tokens)?;
                if !tokens.eat("]") {
                    return self.error(format!("a `[` after `{name}` is not closed"));
                }
                self.emit(Operation::Read { table, index })
            }
            Some(Token::Name(name)) => self.read(&name),
            Some(token) => self.error(format!("expected a value, found {token}")),
            None => self.error("expected a value at the end of the line"),
        }
    }

    fn table(&self, name: &str) -> Result<Table, CompileError> {
        match self.names.get(name) {
            Some(Binding::Table(table)) => Ok(*table),
            _ => self.error(format!(
                "`{name}` is no list or buffer: declare `param {name}[<length>] = ...` or `buffer {name} = <seconds>` first"
            )),
        }
    }

    fn read(&mut self, name: &str) -> Result<Register, CompileError> {
        let operation = match name {
            "in" => Operation::Input,
            "channel" => Operation::Channel,
            "sr" => Operation::SampleRate,
            "pi" => Operation::Constant(std::f32::consts::PI),
            "tau" => Operation::Constant(std::f32::consts::TAU),
            "beat" => Operation::Beat,
            "bpm" => Operation::Bpm,
            "playing" => Operation::Playing,
            "freq" => Operation::Frequency,
            "pitch" => Operation::Pitch,
            "gate" => Operation::Gate,
            "velocity" => Operation::Velocity,
            "onset" => Operation::Onset,
            _ => match self.names.get(name) {
                Some(Binding::Value(register)) => return Ok(*register),
                Some(Binding::History { slot, .. }) => Operation::History(*slot),
                Some(Binding::Table(_)) => {
                    return self.error(format!(
                        "`{name}` is a list of values: read one with `{name}[<index>]` or `lookup({name}, <phase>)`"
                    ));
                }
                None => {
                    return self.error(format!(
                        "unknown name `{name}`. A name is set on a line above where it is read; to read a value from the last frame, declare `history {name}` first"
                    ));
                }
            },
        };
        self.emit(operation)
    }

    fn call(&mut self, name: &str, tokens: &mut Tokens<'_>) -> Result<Register, CompileError> {
        // `lookup` and `len` take a table by name, which is no value.
        let table = match name {
            "lookup" | "len" => {
                let Some(Token::Name(table)) = tokens.next().cloned() else {
                    return self.error(format!("`{name}` takes the name of a list or a buffer"));
                };
                let table = self.table(&table)?;
                if name == "len" {
                    if !tokens.eat(")") {
                        return self.error("`len(table)` takes one name");
                    }
                    return self.emit(Operation::Length(table));
                }
                if !tokens.eat(",") {
                    return self.error("`lookup(table, phase)` takes a name and a phase");
                }
                Some(table)
            }
            _ => None,
        };
        let mut arguments = Vec::new();
        if !tokens.eat(")") {
            loop {
                arguments.push(self.expression(tokens)?);
                if tokens.eat(")") {
                    break;
                }
                if !tokens.eat(",") {
                    return self.error(format!("the call of `{name}` is not closed with `)`"));
                }
            }
        }
        let Some((_, takes)) = FUNCTIONS.iter().find(|(function, _)| *function == name) else {
            let names: Vec<&str> = FUNCTIONS.iter().map(|(function, _)| *function).collect();
            return self.error(format!(
                "unknown function `{name}`. The functions are {}",
                names.join(", ")
            ));
        };
        let wrong = |compiler: &Self| {
            compiler.error(format!(
                "`{name}({takes})` does not take {} values",
                arguments.len()
            ))
        };
        if let (Some(unary), [x]) = (unary(name), arguments.as_slice()) {
            return self.emit(Operation::Unary(unary, *x));
        }
        let operation = match (name, arguments.as_slice()) {
            ("min", [a, b]) => Operation::Binary(Binary::Min, *a, *b),
            ("max", [a, b]) => Operation::Binary(Binary::Max, *a, *b),
            ("pow", [x, power]) => Operation::Binary(Binary::Power, *x, *power),
            ("clamp", [x, low, high]) => Operation::Clamp(*x, *low, *high),
            ("mix", [a, b, amount]) => Operation::Mix(*a, *b, *amount),
            ("phasor", [hz]) => Operation::Phasor {
                hz: *hz,
                slot: next(&mut self.code.slots.phasors),
            },
            ("noise", []) => Operation::Noise {
                slot: next(&mut self.code.slots.noises),
            },
            ("delay", [input, ms, rest @ ..]) if rest.len() <= 1 => {
                if self.code.slots.delays.len() == MAX_DELAYS {
                    return self.error(format!("code has at most {MAX_DELAYS} delays"));
                }
                let longest = match rest {
                    [longest] => match self.code.operations.get(usize::from(*longest)) {
                        Some(Operation::Constant(ms)) if *ms > 0.0 && *ms <= MAX_DELAY_MS => *ms,
                        _ => {
                            return self.error(format!(
                                "the longest time of a delay is a number of ms up to {MAX_DELAY_MS}"
                            ));
                        }
                    },
                    _ => MAX_DELAY_MS,
                };
                let slot = self.code.slots.delays.len() as u16;
                self.code.slots.delays.push(longest);
                Operation::Delay {
                    input: *input,
                    ms: *ms,
                    slot,
                }
            }
            ("lowpass" | "highpass" | "bandpass", [input, hz, rest @ ..]) if rest.len() <= 1 => {
                let q = match rest {
                    [q] => *q,
                    _ => self.emit(Operation::Constant(DEFAULT_Q))?,
                };
                let kind = match name {
                    "lowpass" => FilterKind::LowPass,
                    "highpass" => FilterKind::HighPass,
                    _ => FilterKind::BandPass,
                };
                Operation::Filter {
                    kind,
                    input: *input,
                    hz: *hz,
                    q,
                    slot: next(&mut self.code.slots.filters),
                }
            }
            ("smooth", [input, ms]) => Operation::Smooth {
                input: *input,
                ms: *ms,
                slot: next(&mut self.code.slots.smooths),
            },
            ("adsr", [gate, attack, decay, sustain, release]) => Operation::Envelope {
                gate: *gate,
                attack: *attack,
                decay: *decay,
                sustain: *sustain,
                release: *release,
                slot: next(&mut self.code.slots.envelopes),
            },
            ("hold", [input, when]) => Operation::Hold {
                input: *input,
                when: *when,
                slot: next(&mut self.code.slots.memories),
            },
            ("rise", [input]) => Operation::Rise {
                input: *input,
                slot: next(&mut self.code.slots.memories),
            },
            ("change", [input]) => Operation::Change {
                input: *input,
                slot: next(&mut self.code.slots.memories),
            },
            ("lookup", [phase]) => match table {
                Some(table) => Operation::Lookup {
                    table,
                    phase: *phase,
                },
                None => return wrong(self),
            },
            _ => return wrong(self),
        };
        self.emit(operation)
    }
}

fn unary(name: &str) -> Option<Unary> {
    Some(match name {
        "sin" => Unary::Sin,
        "cos" => Unary::Cos,
        "tan" => Unary::Tan,
        "tanh" => Unary::Tanh,
        "abs" => Unary::Abs,
        "sqrt" => Unary::Sqrt,
        "exp" => Unary::Exp,
        "log" => Unary::Log,
        "floor" => Unary::Floor,
        "wrap" => Unary::Wrap,
        "db" => Unary::Decibels,
        "saturate" => Unary::Saturate,
        _ => return None,
    })
}

fn next(count: &mut u16) -> u16 {
    *count += 1;
    *count - 1
}

struct Tokens<'a> {
    tokens: &'a [Token],
    next: usize,
}

impl Tokens<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.next)
    }

    fn next(&mut self) -> Option<&Token> {
        let token = self.tokens.get(self.next);
        self.next += 1;
        token
    }

    /// Takes the next token when it is `symbol`.
    fn eat(&mut self, symbol: &str) -> bool {
        let found = matches!(self.peek(), Some(Token::Symbol(next)) if *next == symbol);
        if found {
            self.next += 1;
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str) -> Vec<String> {
        source.lines().map(str::to_string).collect()
    }

    fn error(source: &str) -> CompileError {
        compile(&lines(source)).expect_err(source)
    }

    #[test]
    fn an_error_names_the_line_and_says_what_to_write_instead() {
        let found = error("param rate = 4 [0.1, 20]\nx = sin(rat)");
        assert_eq!(found.line, 1);
        assert!(found.message.contains("unknown name `rat`"), "{found}");

        let found = error("x = lowpas(in, 100)");
        assert!(found.message.contains("lowpass"), "{found}");

        let found = error("x = delay(in)");
        assert!(
            found.message.contains("`delay(x, ms[, longest_ms])`"),
            "{found}"
        );

        let found = error("x = in\nx = 2");
        assert!(found.message.contains("history x"), "{found}");

        let found = error("// feedback\nhistory feedback\nout = in");
        assert_eq!(found.line, 1);
        assert!(found.message.contains("never set"), "{found}");

        let found = error("param depth = 2 [0, 1]");
        assert!(found.message.contains("outside"), "{found}");

        let found = error("param steps[4] = 0 [0, 1]\nout = steps");
        assert!(found.message.contains("steps[<index>]"), "{found}");

        let found = error("param steps[4] = 0 [0, 1]\nsteps[0] = 1");
        assert!(found.message.contains("cannot write"), "{found}");

        let found = error("x = delay(in, 10, in)");
        assert!(found.message.contains("longest time"), "{found}");
    }

    #[test]
    fn a_name_is_read_only_below_the_line_that_sets_it() {
        assert!(compile(&lines("out = x\nx = in")).is_err());
        assert!(compile(&lines("history x\nout = x\nx = in")).is_ok());
    }

    #[test]
    fn the_code_says_what_the_record_and_the_interface_give_it() {
        let code = compile(&lines(
            "param rate = 4 [0.1, 20]\nparam steps[16] = 0 [0, 1]\nlive x = 0.5 [0, 1]\ntrigger hit\nbuffer loop = 2\nwatch level = abs(in)",
        ))
        .unwrap();
        assert_eq!(code.parameters.len(), 1);
        assert_eq!(code.arrays[0].length, 16);
        assert_eq!(code.lives[0].name, "x");
        assert_eq!(code.triggers, ["hit"]);
        assert_eq!(code.watches, ["level"]);
        assert_eq!(code.slots.buffers, [2.0]);
    }
}
