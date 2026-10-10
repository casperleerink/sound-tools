//! Compiled code: a flat list of operations that [`Machine`](crate::Machine) runs for each
//! channel, which [`Graph::compile`](crate::Graph::compile) makes.
//!
//! Every operation writes the register with its own index and only reads registers before it,
//! so one pass in order computes a frame. A `History` reads what a feedback was set to in the
//! frame before.

/// The most params, and so the most knobs and toggles: the update carries their values in a
/// fixed array.
pub(crate) const MAX_PARAMETERS: usize = 32;
/// The most `live` controls and the most triggers: a trigger is one bit of a `u32`.
pub(crate) const MAX_LIVES: usize = 32;
/// The most watches.
pub(crate) const MAX_WATCHES: usize = 16;
/// The longest a `delay` can be.
pub(crate) const MAX_DELAY_MS: f32 = 4000.0;
/// Each delay holds up to [`MAX_DELAY_MS`] per channel and voice, so their number is held too.
pub(crate) const MAX_DELAYS: usize = 16;
/// The seconds of all the buffers of one code together, per channel and voice.
pub(crate) const MAX_BUFFER_SECONDS: f32 = 30.0;
/// The longest a pattern can be.
pub(crate) const MAX_ARRAY_LENGTH: usize = 1024;
pub(crate) const MAX_OPERATIONS: usize = 4096;

/// The index of an operation, and of the register it writes.
pub(crate) type Register = u16;

/// A knob, a toggle or a live control: a number glided so a change does not click.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterSpec {
    pub name: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

/// A pattern or a sample: a list the record sets, such as the steps of a sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct ArraySpec {
    pub name: String,
    /// `None` for a `sample`: as long as its sound.
    pub length: Option<usize>,
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

/// Compiled code.
#[derive(Clone, Debug)]
pub struct Code {
    /// Numbers the record sets, in the order of the fields.
    pub parameters: Vec<ParameterSpec>,
    /// Lists the record sets.
    pub arrays: Vec<ArraySpec>,
    /// Numbers the interface plays and nothing saves.
    pub lives: Vec<ParameterSpec>,
    /// Names of the triggers, which the interface fires.
    pub triggers: Vec<String>,
    /// Names of the values the interface reads.
    pub watches: Vec<String>,
    /// Of the graph and what it reads, so a behaviour tells new code from new values.
    pub hash: u64,
    pub(crate) operations: Vec<Operation>,
    pub(crate) output: Output,
    /// The register each feedback takes for the next frame, by its slot.
    pub(crate) feedbacks: Vec<Register>,
    /// The register of each watch, in the order of [`Self::watches`].
    pub(crate) watch_registers: Vec<Register>,
    pub(crate) slots: Slots,
}

/// What leaves the code.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Output {
    /// The code runs on each channel apart.
    Mono(Register),
    /// The code runs once for both channels, and hears both.
    Stereo(Register, Register),
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

/// Where `lookup`, `length` and `at` read: a list of the record, or a buffer.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Table {
    Array(u16),
    Buffer(u16),
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Operation {
    Constant(f32),
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
