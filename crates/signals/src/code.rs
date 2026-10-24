//! Compiled code: a flat list of operations that [`Machine`](crate::Machine) runs for each
//! channel, which [`Graph::compile`](crate::Graph::compile) makes.
//!
//! Every operation writes the register with its own index and only reads registers before it,
//! so one pass in order computes a frame. A `History` reads what a feedback was set to in the
//! frame before.

use sound_core::SvfShape;

/// The most params, and so the most knobs and toggles: the update carries their values in a
/// fixed array.
pub(crate) const MAX_PARAMETERS: usize = 32;
/// The most `live` controls and the most triggers: a trigger is one bit of a `u32`.
pub(crate) const MAX_LIVES: usize = 32;
/// The most watches.
pub(crate) const MAX_WATCHES: usize = 16;
/// The most bangs and notes a sound sends out, of each: each is an event port of every
/// processor.
pub(crate) const MAX_OUTPUTS: usize = 8;
/// The most numbers a sound sends out: each is an event port of every processor, and a watch a
/// view reads.
pub(crate) const MAX_NUMBERS: usize = MAX_WATCHES;
/// The longest a `delay` can be.
pub(crate) const MAX_DELAY_MS: f32 = 4000.0;
/// Each delay holds up to [`MAX_DELAY_MS`] per channel and voice, so their number is held too.
pub(crate) const MAX_DELAYS: usize = 16;
/// The seconds of all the buffers of one code together, per channel and voice.
pub(crate) const MAX_BUFFER_SECONDS: f32 = 30.0;
/// The longest a pattern can be.
pub const MAX_ARRAY_LENGTH: usize = 1024;
/// The sizes a `spectral` can be, powers of two: each bin keeps a memory, and a spectrum comes
/// that many samples late.
pub(crate) const SPECTRUM_SIZES: std::ops::RangeInclusive<usize> = 256..=8192;
/// The most spectra one sound has, as each costs a transform both ways and its bins.
pub(crate) const MAX_SPECTRA: usize = 8;
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
    /// Names of the numbers, bangs and notes it sends out, which a parent connects to another
    /// tool.
    pub numbers: Vec<String>,
    pub bangs: Vec<String>,
    pub notes: Vec<String>,
    /// Of the graph and what it reads, so a behaviour tells new code from new values.
    pub hash: u64,
    /// How many samples late its sound comes out: of the spectra on the way.
    pub latency: u32,
    pub(crate) operations: Vec<Operation>,
    pub(crate) output: Output,
    /// The register each feedback takes for the next frame, by its slot.
    pub(crate) feedbacks: Vec<Register>,
    /// The register of each watch, in the order of [`Self::watches`].
    pub(crate) watch_registers: Vec<Register>,
    /// The register of each number, in the order of [`Self::numbers`].
    pub(crate) number_registers: Vec<Register>,
    /// The registers that send events, frame by frame: each bang's, then the gate, the pitch
    /// and the velocity of each notes.
    pub(crate) event_registers: Vec<Register>,
    pub(crate) slots: Slots,
}

impl Code {
    /// Whether it calls `spectral`, which costs a transform per voice.
    pub fn has_spectra(&self) -> bool {
        !self.slots.spectra.is_empty()
    }

    /// Whether what leaves it is what comes in, on each channel, and nothing else leaves: the
    /// code of a tool with no sound.
    pub(crate) fn passes_input(&self) -> bool {
        let Output::Mono(register) = self.output else {
            return false;
        };
        let input = self.operations.get(usize::from(register));
        matches!(input, Some(Operation::Input))
            && self.watch_registers.is_empty()
            && self.number_registers.is_empty()
            && self.event_registers.is_empty()
    }
}

/// What leaves the code.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Output {
    /// The code runs on each channel apart.
    Mono(Register),
    /// The code runs once for both channels, and hears both.
    Stereo(Register, Register),
    /// The magnitude and the phase of one bin, of the code of a `spectral`.
    Bin(Register, Register),
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
    pub(crate) spectra: Vec<SpectrumSpec>,
}

/// A `spectral`: the code that runs once per bin of each spectrum, and what it reads of the
/// sound around it. In that code a `Parameter` is one of those captures, as it was in the
/// last sample of the hop.
#[derive(Clone, Debug)]
pub(crate) struct SpectrumSpec {
    pub(crate) size: usize,
    pub(crate) code: Code,
    pub(crate) captures: Vec<Register>,
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
        kind: SvfShape,
        input: Register,
        hz: Register,
        q: Register,
        /// In dB. A shape without a gain ignores it.
        gain: Register,
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
    Spectral {
        input: Register,
        slot: u16,
    },
    /// What a bin of a spectrum is, in the code of a `spectral`.
    Bin(BinValue),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum BinValue {
    Magnitude,
    Phase,
    Hz,
    Index,
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
