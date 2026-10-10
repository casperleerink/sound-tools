//! Runs compiled [`Code`] over a span of frames of a block, for both channels, with the memory
//! of every feedback, `phasor`, `delay`, filter, envelope and buffer of the code: the memory of
//! one voice. Made on the control thread, where it allocates all of it; running it allocates
//! nothing.
//!
//! What every voice shares, the values of the record and of the interface and the transport,
//! the processor works out once per block as a [`Block`]. The note of a voice is the same over
//! the span the voice plays: the processor splits a block where a note comes or goes.

use std::iter::repeat;
use std::ops::Range;

use sound_core::{
    CHANNELS, DelayLine, MAX_BLOCK, SVF_MAX_Q, SvfFactors, SvfSection, amplitude, soft_clip,
};

use crate::code::{
    Binary, Code, FilterKind, MAX_LIVES, MAX_PARAMETERS, Operation, Output, Register, Table, Unary,
};
use crate::program::{self, Run, Step};

/// Where every knob and toggle stands, and every list of the record, in the order of the
/// fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values {
    pub parameters: [f32; MAX_PARAMETERS],
    /// `None` leaves the lists the processor has: a sample is long, so it is sent only when
    /// it or another list changed, not at every turn of a knob.
    pub arrays: Option<Vec<Vec<f32>>>,
    /// For each number an automation lane can move, in the order of its index, the param it
    /// moves.
    pub automated: Vec<u16>,
}

/// What leaves the code, and the processor, is held to this, about 12 dB over full scale, so
/// a feedback that runs away is loud but not deafening.
pub(crate) const LIMIT: f32 = 4.0;

/// The note a voice plays. A source follows the newest held note; an effect has none.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Note {
    pub(crate) pitch: f32,
    pub(crate) frequency: f32,
    pub(crate) velocity: f32,
    pub(crate) gate: bool,
    /// The first frame of the note: the first frame of the span it starts.
    pub(crate) onset: bool,
}

/// Both channels of up to a block of frames.
pub(crate) type Frames = [[f32; MAX_BLOCK]; CHANNELS];

/// What every voice reads in one block, frame by frame, from the processor.
pub(crate) struct Block {
    pub(crate) input: Frames,
    pub(crate) parameters: [[f32; MAX_BLOCK]; MAX_PARAMETERS],
    pub(crate) lives: [[f32; MAX_BLOCK]; MAX_LIVES],
    /// One bit per param, and per live control, whose row is one value in every frame of the
    /// block: it does not glide.
    pub(crate) steady_parameters: u32,
    pub(crate) steady_lives: u32,
    /// One bit per trigger, set in the frame it fires.
    pub(crate) triggers: [u32; MAX_BLOCK],
    pub(crate) beat: [f32; MAX_BLOCK],
    pub(crate) bpm: f32,
    pub(crate) playing: bool,
}

impl Block {
    pub(crate) fn new() -> Self {
        Self {
            input: [[0.0; MAX_BLOCK]; CHANNELS],
            parameters: [[0.0; MAX_BLOCK]; MAX_PARAMETERS],
            lives: [[0.0; MAX_BLOCK]; MAX_LIVES],
            steady_parameters: 0,
            steady_lives: 0,
            triggers: [0; MAX_BLOCK],
            beat: [0.0; MAX_BLOCK],
            bpm: 0.0,
            playing: false,
        }
    }
}

/// What one voice reads over a span of frames.
pub(crate) struct Inputs<'a> {
    pub(crate) block: &'a Block,
    pub(crate) arrays: &'a [Vec<f32>],
    pub(crate) note: Note,
}

#[derive(Clone)]
pub struct Machine {
    code: Code,
    steps: Box<[Step]>,
    /// The steps the right channel of mono code runs again; it shares the results of the others
    /// with the left.
    per_channel: Box<[Step]>,
    sample_rate: f32,
    /// What each operation gave in each frame of the span: of the left channel, and then of the
    /// right where it differs.
    registers: Vec<[f32; MAX_BLOCK]>,
    /// The value a register holds in every frame of its row, where it holds one. A step that
    /// gives one value over a span works it out once, and fills the row only when it differs.
    filled: Vec<Option<f32>>,
    channels: [Memory; CHANNELS],
    /// Where the delays write the first frame of the next span.
    position: usize,
    /// The last value of each watch, of the left channel.
    watched: Vec<f32>,
}

/// The memory of one channel.
#[derive(Clone)]
struct Memory {
    histories: Vec<f32>,
    phases: Vec<f32>,
    noises: Vec<u32>,
    delays: Vec<DelayLine>,
    filters: Vec<Filter>,
    smooths: Vec<Smooth>,
    envelopes: Vec<Envelope>,
    memories: Vec<f32>,
    buffers: Vec<Vec<f32>>,
}

#[derive(Clone, Default)]
struct Filter {
    section: SvfSection,
    factors: SvfFactors,
    /// The damping, `1 / Q`, which a band and a high pass need too.
    k: f32,
    /// The cutoff and Q the factors are for, so a `tan` runs only when they move.
    hz: f32,
    q: f32,
}

#[derive(Clone, Default)]
struct Smooth {
    value: f32,
    ms: f32,
    factor: f32,
}

/// A straight-line ADSR. A new gate starts the attack from where the level is, so a note that
/// takes over a sounding voice does not click. The release takes its whole time from wherever
/// the level is when the gate falls, as a musician hears "a release of 20 ms".
#[derive(Clone, Default)]
struct Envelope {
    level: f32,
    stage: Stage,
    gate: bool,
    /// How much the release takes off per frame.
    release_step: f32,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// `$body` for the formula `$op` of a unary operation, with `$kind` that formula: matched
/// once, and `$body` compiled for each formula with its own constant.
macro_rules! with_unary {
    ($op:expr, $kind:ident => $body:expr) => {
        with_kind!($op, $kind => $body, Unary: Negate, Sin, Cos, Tan, Tanh, Abs, Sqrt, Exp, Log,
            Floor, Wrap, Decibels, Saturate)
    };
}

/// As [`with_unary`], of a binary operation.
macro_rules! with_binary {
    ($op:expr, $kind:ident => $body:expr) => {
        with_kind!($op, $kind => $body, Binary: Add, Subtract, Multiply, Divide, Remainder, Less,
            Greater, LessOrEqual, GreaterOrEqual, Equal, NotEqual, Min, Max, Power)
    };
}

macro_rules! with_kind {
    ($op:expr, $kind:ident => $body:expr, $type:ident: $($variant:ident),*) => {
        match $op {
            $($type::$variant => {
                let $kind = $type::$variant;
                $body
            })*
        }
    };
}

impl Machine {
    pub fn new(code: Code, sample_rate: f32) -> Self {
        let slots = &code.slots;
        let channels: [Memory; CHANNELS] = std::array::from_fn(|channel| Memory {
            histories: vec![0.0; usize::from(slots.histories)],
            phases: vec![0.0; usize::from(slots.phasors)],
            // A fixed seed per slot and channel, so a render is the same every time and the
            // two channels hear different noise.
            noises: (0..u32::from(slots.noises))
                .map(|slot| 0x9E37_79B9 ^ (slot * 2 + channel as u32 + 1).wrapping_mul(0x85EB_CA6B))
                .collect(),
            delays: (slots.delays.iter())
                .map(|ms| DelayLine::new((ms * 0.001 * sample_rate) as usize + 4))
                .collect(),
            filters: (0..slots.filters)
                .map(|_| Filter {
                    hz: f32::NAN,
                    ..Filter::default()
                })
                .collect(),
            smooths: (0..slots.smooths)
                .map(|_| Smooth {
                    ms: f32::NAN,
                    ..Smooth::default()
                })
                .collect(),
            envelopes: vec![Envelope::default(); usize::from(slots.envelopes)],
            memories: vec![0.0; usize::from(slots.memories)],
            buffers: (slots.buffers.iter())
                .map(|seconds| vec![0.0; ((seconds * sample_rate) as usize).max(1)])
                .collect(),
        });
        let mut registers = vec![[0.0; MAX_BLOCK]; code.operations.len()];
        let mut filled = vec![None; code.operations.len()];
        // The buffers of every channel are as long.
        let buffers = channels.first().map_or(&[][..], |memory| &memory.buffers);
        let edges = program::edges(&code);
        let mut steps = Vec::new();
        for step in program::schedule(&code, &edges) {
            if let Step::Block(register) = step {
                let register = usize::from(register);
                // What is the same in every frame for as long as the machine lives is set once,
                // and is no step.
                let fixed = match code.operations.get(register) {
                    Some(Operation::Constant(value)) => Some(*value),
                    Some(Operation::SampleRate) => Some(sample_rate),
                    Some(Operation::Length(table @ Table::Buffer(_))) => {
                        Some(table_of(*table, &[], buffers).len() as f32)
                    }
                    _ => None,
                };
                if let Some(value) = fixed {
                    fill(&mut registers, &mut filled, register, value);
                    continue;
                }
            }
            steps.push(step);
        }
        Self {
            per_channel: program::per_channel(&code, &edges, &steps),
            steps: steps.into_boxed_slice(),
            sample_rate,
            registers,
            filled,
            channels,
            position: 0,
            watched: vec![0.0; code.watches.len()],
            code,
        }
    }

    pub fn code(&self) -> &Code {
        &self.code
    }

    /// The last value of each watch, in the order of [`Code::watches`].
    pub(crate) fn watched(&self) -> &[f32] {
        &self.watched
    }

    /// The `frames` of a block, both channels, into the same frames of `output`.
    pub(crate) fn render(
        &mut self,
        inputs: &Inputs<'_>,
        frames: Range<usize>,
        output: &mut Frames,
    ) {
        let Some(last) = frames.clone().last() else {
            return;
        };
        // Stereo code runs once, with the memory of the left channel, for both.
        let runs = match self.code.output {
            Output::Mono(_) => CHANNELS,
            Output::Stereo(..) => 1,
        };
        for channel in 0..runs {
            self.run(channel, inputs, frames.clone());
            let registers = &self.registers;
            if channel == 0 {
                // The watches show the left channel.
                for (watched, register) in self.watched.iter_mut().zip(&self.code.watch_registers) {
                    *watched = value_at(registers, *register, last);
                }
            }
            match self.code.output {
                Output::Mono(register) => {
                    if let Some(output) = output.get_mut(channel) {
                        held_into(output, registers, register, &frames);
                    }
                }
                Output::Stereo(left, right) => {
                    for (output, register) in output.iter_mut().zip([left, right]) {
                        held_into(output, registers, register, &frames);
                    }
                }
            }
        }
        self.position = self.position.wrapping_add(frames.len());
    }

    /// Runs the steps of `channel` over `frames` with its memory: every step for the left, and
    /// for the right only those whose results differ from the left.
    fn run(&mut self, channel: usize, inputs: &Inputs<'_>, frames: Range<usize>) {
        let Self {
            code,
            steps,
            per_channel,
            sample_rate,
            registers,
            filled,
            channels,
            position,
            ..
        } = self;
        let Some(memory) = channels.get_mut(channel) else {
            return;
        };
        let steps = if channel == 0 { steps } else { per_channel };
        let span = Span {
            block: inputs.block,
            arrays: inputs.arrays,
            note: inputs.note,
            channel,
            first: frames.start,
            position: *position,
            sample_rate: *sample_rate,
            feedbacks: &code.feedbacks,
        };
        let operations = &code.operations;
        for step in steps.iter() {
            match step {
                Step::Block(register) => {
                    let register = usize::from(*register);
                    let Some(operation) = operations.get(register) else {
                        continue;
                    };
                    match span.steady(*operation, registers, filled, memory, &frames) {
                        Some(value) => fill(registers, filled, register, value),
                        None => {
                            let frames = frames.clone();
                            span.run(*operation, register, frames, registers, filled, memory);
                            if let Some(filled) = filled.get_mut(register) {
                                *filled = None;
                            }
                        }
                    }
                }
                Step::Loop(looped) => span.run_loop(looped, frames.clone(), registers, memory),
            }
        }
        // What each feedback reads in the first frame of the next span.
        let last = frames.end.saturating_sub(1);
        for (history, source) in memory.histories.iter_mut().zip(&code.feedbacks) {
            *history = finite(value_at(registers, *source, last));
        }
    }
}

/// What the operations of one channel read over a span of frames.
struct Span<'a> {
    block: &'a Block,
    arrays: &'a [Vec<f32>],
    note: Note,
    channel: usize,
    /// The first frame of the span: of the onset of a note, and the one where a feedback reads
    /// what was set in the span before.
    first: usize,
    /// Where the delays write the first frame.
    position: usize,
    sample_rate: f32,
    feedbacks: &'a [Register],
}

impl Span<'_> {
    /// Runs `operation`, whose register is `register`, over `frames` of the span. Each kind of
    /// operation has a loop of its own over the frames, so it is not matched in every frame.
    ///
    /// A memory is kept in a local over the frames and put back after: left in its slot, it is
    /// stored and loaded again in every frame, which lengthens the chain from one frame to the
    /// next.
    ///
    /// A memory whose settings, such as the cutoff of a filter, are [`Machine::filled`] runs over
    /// their one value repeated: the formula works out from them in every frame, and the
    /// compiler moves that out of the loop, so it runs once per span.
    fn run(
        &self,
        operation: Operation,
        register: usize,
        frames: Range<usize>,
        registers: &mut [[f32; MAX_BLOCK]],
        filled: &[Option<f32>],
        memory: &mut Memory,
    ) {
        // An operation reads only registers before its own, but for the source of a feedback.
        let Some((before, rest)) = registers.split_at_mut_checked(register) else {
            return;
        };
        let Some((own, after)) = rest.split_first_mut() else {
            return;
        };
        let Some(out) = own.get_mut(frames.clone()) else {
            return;
        };
        let read = |register: Register| row(before, usize::from(register), &frames);
        let one = |register: Register| filled.get(usize::from(register)).copied().flatten();
        // `$body` with each of the `$settings` registers as its values: one value repeated
        // where all are filled, else their rows.
        macro_rules! settled {
            ([$($settings:ident),*], $body:expr) => {
                if let ($(Some($settings),)*) = ($(one($settings),)*) {
                    $(let $settings = repeat($settings);)*
                    $body
                } else {
                    $(let $settings = read($settings).iter().copied();)*
                    $body
                }
            };
        }
        let block = self.block;
        let note = self.note;
        let sample_rate = self.sample_rate;
        match operation {
            Operation::Constant(value) => out.fill(value),
            Operation::Input => copy(out, row(&block.input, self.channel, &frames)),
            Operation::InputLeft => copy(out, row(&block.input, 0, &frames)),
            Operation::InputRight => copy(out, row(&block.input, 1, &frames)),
            Operation::Channel => out.fill(self.channel as f32),
            Operation::SampleRate => out.fill(sample_rate),
            Operation::Beat => copy(out, part(&block.beat, &frames)),
            Operation::Bpm => out.fill(block.bpm),
            Operation::Playing => out.fill(truth(block.playing)),
            Operation::Frequency => out.fill(note.frequency),
            Operation::Pitch => out.fill(note.pitch),
            Operation::Gate => out.fill(truth(note.gate)),
            Operation::Velocity => out.fill(note.velocity),
            Operation::Onset => {
                for (out, frame) in out.iter_mut().zip(frames.clone()) {
                    *out = truth(self.onset(frame));
                }
            }
            Operation::Parameter(index) => {
                copy(out, row(&block.parameters, usize::from(index), &frames));
            }
            Operation::Live(index) => copy(out, row(&block.lives, usize::from(index), &frames)),
            Operation::Trigger(index) => {
                let triggers = block.triggers.get(frames.clone()).unwrap_or_default();
                for (out, triggers) in out.iter_mut().zip(triggers) {
                    *out = truth(triggers & (1 << index) != 0);
                }
            }
            Operation::History(slot) => {
                // A feedback set to its own read is in neither `before` nor `after`: it reads 0.
                let source = self.feedbacks.get(usize::from(slot)).map(|source| {
                    let source = usize::from(*source);
                    match source.checked_sub(register + 1) {
                        Some(index) => after.get(index),
                        None => before.get(source),
                    }
                });
                let source = source.flatten();
                for (out, frame) in out.iter_mut().zip(frames) {
                    *out = self.heard(frame, slot, &memory.histories, source);
                }
            }
            Operation::Unary(unary, x) => apply_unary(unary, out, read(x)),
            Operation::Binary(binary, a, b) => apply_binary(binary, out, read(a), read(b)),
            Operation::Clamp(x, low, high) => map3(out, read(x), read(low), read(high), clamp),
            Operation::Mix(a, b, amount) => map3(out, read(a), read(b), read(amount), mix),
            Operation::Phasor { hz, slot } => match memory.phases.get_mut(usize::from(slot)) {
                Some(kept) => settled!([hz], {
                    let mut phase = *kept;
                    for (out, hz) in out.iter_mut().zip(hz) {
                        *out = phasor(&mut phase, hz, sample_rate);
                    }
                    *kept = phase;
                }),
                None => out.fill(0.0),
            },
            Operation::Noise { slot } => match memory.noises.get_mut(usize::from(slot)) {
                Some(kept) => {
                    let mut state = *kept;
                    for out in out.iter_mut() {
                        // xorshift32
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        *out = state as f32 / u32::MAX as f32 * 2.0 - 1.0;
                    }
                    *kept = state;
                }
                None => out.fill(0.0),
            },
            Operation::Delay { input, ms, slot } => {
                match memory.delays.get_mut(usize::from(slot)) {
                    Some(line) => {
                        let mut position =
                            (self.position).wrapping_add(frames.start.wrapping_sub(self.first));
                        for ((out, input), ms) in out.iter_mut().zip(read(input)).zip(read(ms)) {
                            *out = delayed(line, position, *input, *ms, sample_rate);
                            position = position.wrapping_add(1);
                        }
                    }
                    None => out.fill(0.0),
                }
            }
            Operation::Filter {
                kind,
                input,
                hz,
                q,
                slot,
            } => match memory.filters.get_mut(usize::from(slot)) {
                Some(kept) => settled!([hz, q], {
                    let mut filter = kept.clone();
                    let values = read(input).iter().zip(hz).zip(q);
                    for (out, ((input, hz), q)) in out.iter_mut().zip(values) {
                        *out = filter.next(kind, *input, hz, q, sample_rate);
                    }
                    *kept = filter;
                }),
                None => out.fill(0.0),
            },
            Operation::Smooth { input, ms, slot } => {
                match memory.smooths.get_mut(usize::from(slot)) {
                    Some(kept) => {
                        let mut smooth = kept.clone();
                        for ((out, input), ms) in out.iter_mut().zip(read(input)).zip(read(ms)) {
                            *out = smooth.next(*input, *ms, sample_rate);
                        }
                        *kept = smooth;
                    }
                    None => out.fill(0.0),
                }
            }
            Operation::Envelope {
                gate,
                attack,
                decay,
                sustain,
                release,
                slot,
            } => match memory.envelopes.get_mut(usize::from(slot)) {
                Some(kept) => settled!([attack, decay, sustain, release], {
                    let mut envelope = kept.clone();
                    let times = attack.zip(decay).zip(release);
                    let values = read(gate).iter().zip(sustain).zip(times);
                    let outs = out.iter_mut().zip(frames.clone());
                    for ((out, frame), ((gate, sustain), ((attack, decay), release))) in
                        outs.zip(values)
                    {
                        let times = [attack, decay, release];
                        let onset = self.onset(frame);
                        *out = envelope.next(*gate, onset, times, sustain, sample_rate);
                    }
                    *kept = envelope;
                }),
                None => out.fill(0.0),
            },
            Operation::Hold { input, when, slot } => {
                match memory.memories.get_mut(usize::from(slot)) {
                    Some(kept) => {
                        let mut held = *kept;
                        for ((out, input), when) in out.iter_mut().zip(read(input)).zip(read(when))
                        {
                            *out = hold(&mut held, *input, *when);
                        }
                        *kept = held;
                    }
                    None => out.fill(0.0),
                }
            }
            Operation::Rise { input, slot } => match memory.memories.get_mut(usize::from(slot)) {
                Some(kept) => {
                    let mut before = *kept;
                    for (out, now) in out.iter_mut().zip(read(input)) {
                        *out = rise(&mut before, *now);
                    }
                    *kept = before;
                }
                None => out.fill(0.0),
            },
            Operation::Change { input, slot } => match memory.memories.get_mut(usize::from(slot)) {
                Some(kept) => {
                    let mut before = *kept;
                    for (out, now) in out.iter_mut().zip(read(input)) {
                        *out = change(&mut before, *now);
                    }
                    *kept = before;
                }
                None => out.fill(0.0),
            },
            Operation::Read { table, index } => {
                let values = table_of(table, self.arrays, &memory.buffers);
                map(out, read(index), |index| read_at(values, index));
            }
            Operation::Lookup { table, phase } => {
                let values = table_of(table, self.arrays, &memory.buffers);
                map(out, read(phase), |phase| look_up(values, phase));
            }
            Operation::Length(table) => {
                out.fill(table_of(table, self.arrays, &memory.buffers).len() as f32);
            }
            Operation::Write {
                buffer,
                index,
                value,
            } => {
                for ((out, index), value) in out.iter_mut().zip(read(index)).zip(read(value)) {
                    *out = write(memory.buffers.get_mut(usize::from(buffer)), *index, *value);
                }
            }
        }
    }

    /// Runs the operations of a loop, in their order, frame by frame. Not inlined: in one
    /// function with the kernels of [`Self::run`], a loop ran slower.
    #[inline(never)]
    fn run_loop(
        &self,
        looped: &[Run],
        frames: Range<usize>,
        registers: &mut [[f32; MAX_BLOCK]],
        memory: &mut Memory,
    ) {
        // No frame is past a block, which the compiler then knows: it checks no frame of a row.
        for frame in frames.start..frames.end.min(MAX_BLOCK) {
            for run in looped {
                self.run_formula(run, frame, registers, memory);
            }
        }
    }

    /// The value `operation` gives in every frame of the span, where it gives one: when what it
    /// reads is one value over the span, as a note is, a param that does not glide, or a
    /// register that is [`Machine::filled`]. By the formula it runs with over a span, on the
    /// same values, so it gives what running it over the span would. A `hold` whose trigger does
    /// not fire in the span gives what it holds.
    fn steady(
        &self,
        operation: Operation,
        registers: &[[f32; MAX_BLOCK]],
        filled: &[Option<f32>],
        memory: &Memory,
        frames: &Range<usize>,
    ) -> Option<f32> {
        let one = |register: Register| filled.get(usize::from(register)).copied().flatten();
        let block = self.block;
        let row_of = |rows: &[[f32; MAX_BLOCK]], steady: u32, index: u16| {
            let steady = steady.checked_shr(u32::from(index)).unwrap_or(0) & 1 == 1;
            steady.then(|| value_at(rows, index, self.first))
        };
        match operation {
            Operation::Constant(value) => Some(value),
            Operation::Channel => Some(self.channel as f32),
            Operation::SampleRate => Some(self.sample_rate),
            Operation::Bpm => Some(block.bpm),
            Operation::Playing => Some(truth(block.playing)),
            Operation::Frequency => Some(self.note.frequency),
            Operation::Pitch => Some(self.note.pitch),
            Operation::Gate => Some(truth(self.note.gate)),
            Operation::Velocity => Some(self.note.velocity),
            Operation::Parameter(index) => {
                row_of(&block.parameters, block.steady_parameters, index)
            }
            Operation::Live(index) => row_of(&block.lives, block.steady_lives, index),
            Operation::Length(table) => {
                Some(table_of(table, self.arrays, &memory.buffers).len() as f32)
            }
            Operation::Unary(op, x) => Some(unary(op, one(x)?)),
            Operation::Binary(op, a, b) => Some(binary(op, one(a)?, one(b)?)),
            Operation::Clamp(x, low, high) => Some(clamp(one(x)?, one(low)?, one(high)?)),
            Operation::Mix(a, b, amount) => Some(mix(one(a)?, one(b)?, one(amount)?)),
            // A trigger that does not fire in the span keeps what is held, whatever the input.
            Operation::Hold { when, slot, .. } => {
                let whens = row(registers, usize::from(when), frames);
                let fires = (whens.iter()).fold(false, |fired, when| fired | fires(*when));
                (memory.memories.get(usize::from(slot)).copied()).filter(|_| !fires)
            }
            // What moves within a span, or keeps a memory.
            Operation::Input
            | Operation::InputLeft
            | Operation::InputRight
            | Operation::Beat
            | Operation::Onset
            | Operation::Trigger(_)
            | Operation::History(_)
            | Operation::Phasor { .. }
            | Operation::Noise { .. }
            | Operation::Delay { .. }
            | Operation::Filter { .. }
            | Operation::Smooth { .. }
            | Operation::Envelope { .. }
            | Operation::Rise { .. }
            | Operation::Change { .. }
            | Operation::Read { .. }
            | Operation::Lookup { .. }
            | Operation::Write { .. } => None,
        }
    }

    /// Runs `run`, operations of one formula, in one `frame`, in their order. The formula is
    /// matched once for the run, and [`Self::step`] is compiled for each formula on its own.
    #[inline(always)]
    fn run_formula(
        &self,
        run: &[(Register, Operation)],
        frame: usize,
        registers: &mut [[f32; MAX_BLOCK]],
        memory: &mut Memory,
    ) {
        // Every operation of a run has the formula of the first, so it matches `$pattern`.
        macro_rules! each {
            ($pattern:pat => $operation:expr) => {
                for (register, operation) in run {
                    match *operation {
                        $pattern => {
                            let value = self.step($operation, *register, frame, registers, memory);
                            let row = registers.get_mut(usize::from(*register));
                            if let Some(out) = row.and_then(|row| row.get_mut(frame)) {
                                *out = value;
                            }
                        }
                        #[allow(unreachable_patterns)]
                        _ => debug_assert!(false, "a run mixes formulas"),
                    }
                }
            };
        }
        let Some((_, first)) = run.first() else {
            return;
        };
        match *first {
            Operation::Unary(op, _) => with_unary!(op, kind => {
                each!(Operation::Unary(_, x) => Operation::Unary(kind, x))
            }),
            Operation::Binary(op, ..) => with_binary!(op, kind => {
                each!(Operation::Binary(_, a, b) => Operation::Binary(kind, a, b))
            }),
            Operation::History(_) => each!(operation @ Operation::History(_) => operation),
            Operation::Clamp(..) => each!(operation @ Operation::Clamp(..) => operation),
            Operation::Mix(..) => each!(operation @ Operation::Mix(..) => operation),
            Operation::Phasor { .. } => each!(operation @ Operation::Phasor { .. } => operation),
            Operation::Delay { .. } => each!(operation @ Operation::Delay { .. } => operation),
            Operation::Filter { .. } => each!(operation @ Operation::Filter { .. } => operation),
            Operation::Smooth { .. } => each!(operation @ Operation::Smooth { .. } => operation),
            Operation::Envelope { .. } => {
                each!(operation @ Operation::Envelope { .. } => operation);
            }
            Operation::Hold { .. } => each!(operation @ Operation::Hold { .. } => operation),
            Operation::Rise { .. } => each!(operation @ Operation::Rise { .. } => operation),
            Operation::Change { .. } => each!(operation @ Operation::Change { .. } => operation),
            Operation::Read { .. } => each!(operation @ Operation::Read { .. } => operation),
            Operation::Lookup { .. } => each!(operation @ Operation::Lookup { .. } => operation),
            Operation::Write { .. } => each!(operation @ Operation::Write { .. } => operation),
            // In no loop, see `step`.
            _ => each!(operation => operation),
        }
    }

    /// What `operation`, whose register is `register`, gives in one `frame`, from what the
    /// registers hold in it: how a loop runs it, every operation in turn in each frame. With the
    /// formulas of [`Self::run`], so a loop gives what the span would. Inlined, so a loop is one
    /// function.
    #[inline(always)]
    fn step(
        &self,
        operation: Operation,
        register: Register,
        frame: usize,
        registers: &[[f32; MAX_BLOCK]],
        memory: &mut Memory,
    ) -> f32 {
        let at = |register: Register| value_at(registers, register, frame);
        let sample_rate = self.sample_rate;
        match operation {
            // The source of a feedback in a loop is in it too, and puts each frame in its row,
            // where this reads the frame before. As in `run`, one set to its own read reads 0.
            Operation::History(slot) => {
                let source = (self.feedbacks.get(usize::from(slot)))
                    .filter(|source| **source != register)
                    .and_then(|source| registers.get(usize::from(*source)));
                self.heard(frame, slot, &memory.histories, source)
            }
            Operation::Unary(op, x) => unary(op, at(x)),
            Operation::Binary(op, a, b) => binary(op, at(a), at(b)),
            Operation::Clamp(x, low, high) => clamp(at(x), at(low), at(high)),
            Operation::Mix(a, b, amount) => mix(at(a), at(b), at(amount)),
            Operation::Phasor { hz, slot } => (memory.phases.get_mut(usize::from(slot)))
                .map_or(0.0, |phase| phasor(phase, at(hz), sample_rate)),
            Operation::Delay { input, ms, slot } => {
                let position = (self.position).wrapping_add(frame.wrapping_sub(self.first));
                (memory.delays.get_mut(usize::from(slot))).map_or(0.0, |line| {
                    delayed(line, position, at(input), at(ms), sample_rate)
                })
            }
            Operation::Filter {
                kind,
                input,
                hz,
                q,
                slot,
            } => (memory.filters.get_mut(usize::from(slot))).map_or(0.0, |filter| {
                filter.next(kind, at(input), at(hz), at(q), sample_rate)
            }),
            Operation::Smooth { input, ms, slot } => (memory.smooths.get_mut(usize::from(slot)))
                .map_or(0.0, |smooth| smooth.next(at(input), at(ms), sample_rate)),
            Operation::Envelope {
                gate,
                attack,
                decay,
                sustain,
                release,
                slot,
            } => (memory.envelopes.get_mut(usize::from(slot))).map_or(0.0, |envelope| {
                let times = [at(attack), at(decay), at(release)];
                let onset = self.onset(frame);
                envelope.next(at(gate), onset, times, at(sustain), sample_rate)
            }),
            Operation::Hold { input, when, slot } => (memory.memories.get_mut(usize::from(slot)))
                .map_or(0.0, |held| hold(held, at(input), at(when))),
            Operation::Rise { input, slot } => (memory.memories.get_mut(usize::from(slot)))
                .map_or(0.0, |before| rise(before, at(input))),
            Operation::Change { input, slot } => (memory.memories.get_mut(usize::from(slot)))
                .map_or(0.0, |before| change(before, at(input))),
            Operation::Read { table, index } => {
                read_at(table_of(table, self.arrays, &memory.buffers), at(index))
            }
            Operation::Lookup { table, phase } => {
                look_up(table_of(table, self.arrays, &memory.buffers), at(phase))
            }
            Operation::Write {
                buffer,
                index,
                value,
            } => write(
                memory.buffers.get_mut(usize::from(buffer)),
                at(index),
                at(value),
            ),
            // These read no register, feedback or buffer, so they are in no loop. Not run over
            // one frame with `run` either: a second call of `run` keeps it from being inlined
            // where a span runs it, which made code without loops slower.
            Operation::Constant(_)
            | Operation::Input
            | Operation::InputLeft
            | Operation::InputRight
            | Operation::Channel
            | Operation::SampleRate
            | Operation::Beat
            | Operation::Bpm
            | Operation::Playing
            | Operation::Frequency
            | Operation::Pitch
            | Operation::Gate
            | Operation::Velocity
            | Operation::Onset
            | Operation::Parameter(_)
            | Operation::Live(_)
            | Operation::Trigger(_)
            | Operation::Noise { .. }
            | Operation::Length(_) => {
                debug_assert!(false, "an operation that reads nothing is in a loop");
                0.0
            }
        }
    }

    /// What the feedback of `slot` reads in `frame`: what its `source` was in the frame before,
    /// and in the first frame of the span what it was set to in the span before.
    #[inline(always)]
    fn heard(
        &self,
        frame: usize,
        slot: u16,
        histories: &[f32],
        source: Option<&[f32; MAX_BLOCK]>,
    ) -> f32 {
        if frame == self.first {
            return histories.get(usize::from(slot)).copied().unwrap_or(0.0);
        }
        let before = source.and_then(|source| source.get(frame - 1));
        before.map_or(0.0, |value| finite(*value))
    }

    /// Whether `frame` is the first of a note.
    fn onset(&self, frame: usize) -> bool {
        self.note.onset && frame == self.first
    }
}

/// Room for a row that is not there: 0 in every frame.
static ZEROS: [f32; MAX_BLOCK] = [0.0; MAX_BLOCK];

/// The `frames` of `samples`.
fn part<'a>(samples: &'a [f32; MAX_BLOCK], frames: &Range<usize>) -> &'a [f32] {
    samples
        .get(frames.clone())
        .unwrap_or_else(|| ZEROS.get(..frames.len()).unwrap_or_default())
}

/// The `frames` of the row `index` of `rows`: of a register, an input, a param or a live
/// control. 0 for a row that is not there.
fn row<'a>(rows: &'a [[f32; MAX_BLOCK]], index: usize, frames: &Range<usize>) -> &'a [f32] {
    match rows.get(index) {
        Some(samples) => part(samples, frames),
        None => part(&ZEROS, frames),
    }
}

/// `value` in every frame of the row of `register`, unless it holds that already.
fn fill(
    registers: &mut [[f32; MAX_BLOCK]],
    filled: &mut [Option<f32>],
    register: usize,
    value: f32,
) {
    if let (Some(row), Some(filled)) = (registers.get_mut(register), filled.get_mut(register))
        && filled.map(f32::to_bits) != Some(value.to_bits())
    {
        row.fill(value);
        *filled = Some(value);
    }
}

fn value_at(registers: &[[f32; MAX_BLOCK]], register: Register, frame: usize) -> f32 {
    (registers.get(usize::from(register)))
        .and_then(|row| row.get(frame))
        .copied()
        .unwrap_or(0.0)
}

fn copy(out: &mut [f32], values: &[f32]) {
    for (out, value) in out.iter_mut().zip(values) {
        *out = *value;
    }
}

/// What leaves the code from `register` into the `frames` of `output`.
fn held_into(
    output: &mut [f32; MAX_BLOCK],
    registers: &[[f32; MAX_BLOCK]],
    register: Register,
    frames: &Range<usize>,
) {
    let values = row(registers, usize::from(register), frames);
    let output = output.get_mut(frames.clone()).unwrap_or_default();
    for (output, value) in output.iter_mut().zip(values) {
        *output = held(*value);
    }
}

fn map(out: &mut [f32], x: &[f32], f: impl Fn(f32) -> f32) {
    for (out, x) in out.iter_mut().zip(x) {
        *out = f(*x);
    }
}

fn map2(out: &mut [f32], a: &[f32], b: &[f32], f: impl Fn(f32, f32) -> f32) {
    for ((out, a), b) in out.iter_mut().zip(a).zip(b) {
        *out = f(*a, *b);
    }
}

fn map3(out: &mut [f32], a: &[f32], b: &[f32], c: &[f32], f: impl Fn(f32, f32, f32) -> f32) {
    for (((out, a), b), c) in out.iter_mut().zip(a).zip(b).zip(c) {
        *out = f(*a, *b, *c);
    }
}

fn table_of<'a>(table: Table, arrays: &'a [Vec<f32>], buffers: &'a [Vec<f32>]) -> &'a [f32] {
    let values = match table {
        Table::Array(index) => arrays.get(usize::from(index)),
        Table::Buffer(index) => buffers.get(usize::from(index)),
    };
    values.map_or(&[], Vec::as_slice)
}

/// The slot of `index` in a table of `length`: the whole part, wrapped, so any index reads.
fn wrapped(length: usize, index: f32) -> Option<usize> {
    if length == 0 || !index.is_finite() {
        return None;
    }
    Some(index.floor().rem_euclid(length as f32) as usize % length)
}

fn read_at(values: &[f32], index: f32) -> f32 {
    wrapped(values.len(), index)
        .and_then(|slot| values.get(slot))
        .copied()
        .unwrap_or(0.0)
}

/// The table read at `phase` from 0 to 1 over its whole length, wrapped, with a straight line
/// between two values, as a wavetable or a sample is played.
fn look_up(values: &[f32], phase: f32) -> f32 {
    let length = values.len();
    if length == 0 || !phase.is_finite() {
        return 0.0;
    }
    let position = (phase - phase.floor()) * length as f32;
    let fraction = position - position.floor();
    let first = read_at(values, position);
    let second = read_at(values, position + 1.0);
    first + (second - first) * fraction
}

impl Filter {
    #[inline(always)]
    fn next(&mut self, kind: FilterKind, input: f32, hz: f32, q: f32, sample_rate: f32) -> f32 {
        if (hz != self.hz || q != self.q) && hz.is_finite() && q.is_finite() {
            let g = SvfFactors::cutoff_factor(hz.max(1.0), sample_rate);
            self.k = 1.0 / q.clamp(0.1, SVF_MAX_Q);
            self.factors = SvfFactors::new(g, self.k);
            self.hz = hz;
            self.q = q;
        }
        let input = finite(input);
        let (band, low) = self.section.band_and_low(&self.factors, input);
        let k = self.k;
        match kind {
            FilterKind::LowPass => low,
            FilterKind::BandPass => k * band,
            FilterKind::HighPass => input - k * band - low,
        }
    }
}

impl Smooth {
    fn next(&mut self, input: f32, ms: f32, sample_rate: f32) -> f32 {
        if ms != self.ms {
            let frames = ms * 0.001 * sample_rate;
            self.factor = if frames > 1.0 {
                (-1.0 / frames).exp()
            } else {
                0.0
            };
            self.ms = ms;
        }
        self.value = finite(input + (self.value - input) * self.factor);
        self.value
    }
}

impl Envelope {
    /// `times` are of the attack, the decay and the release, in milliseconds. A note that takes
    /// over a voice whose gate is still up, as one does under the sustain pedal, starts the
    /// attack at its `onset`: the gate it sees never fell.
    #[inline(always)]
    fn next(
        &mut self,
        gate: f32,
        onset: bool,
        times: [f32; 3],
        sustain: f32,
        sample_rate: f32,
    ) -> f32 {
        let [attack, decay, release] = times.map(|ms| (ms * 0.001 * sample_rate).max(1.0));
        // A sustain that is not a number would stay in the level for good.
        let sustain = finite(sustain).clamp(0.0, 1.0);
        let gate = gate > 0.0;
        if gate && (!self.gate || onset) {
            self.stage = Stage::Attack;
        } else if !gate && self.gate {
            self.stage = Stage::Release;
            self.release_step = self.level / release;
        }
        self.gate = gate;
        match self.stage {
            Stage::Idle => self.level = 0.0,
            Stage::Attack => {
                self.level += 1.0 / attack;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level -= (1.0 - sustain) / decay;
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = sustain,
            Stage::Release => {
                self.level -= self.release_step;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

fn truth(condition: bool) -> f32 {
    if condition { 1.0 } else { 0.0 }
}

/// The formula of `op`.
#[inline(always)]
fn unary(op: Unary, x: f32) -> f32 {
    match op {
        Unary::Negate => -x,
        Unary::Sin => x.sin(),
        Unary::Cos => x.cos(),
        Unary::Tan => x.tan(),
        Unary::Tanh => x.tanh(),
        Unary::Abs => x.abs(),
        Unary::Sqrt => x.max(0.0).sqrt(),
        Unary::Exp => x.exp(),
        Unary::Log => x.max(f32::MIN_POSITIVE).ln(),
        Unary::Floor => x.floor(),
        Unary::Wrap => x - x.floor(),
        Unary::Decibels => amplitude(x),
        Unary::Saturate => soft_clip(x),
    }
}

/// The formula of `op`.
#[inline(always)]
fn binary(op: Binary, a: f32, b: f32) -> f32 {
    match op {
        Binary::Add => a + b,
        Binary::Subtract => a - b,
        Binary::Multiply => a * b,
        Binary::Divide => a / b,
        Binary::Remainder => a.rem_euclid(b),
        Binary::Less => truth(a < b),
        Binary::Greater => truth(a > b),
        Binary::LessOrEqual => truth(a <= b),
        Binary::GreaterOrEqual => truth(a >= b),
        Binary::Equal => truth(a == b),
        Binary::NotEqual => truth(a != b),
        Binary::Min => a.min(b),
        Binary::Max => a.max(b),
        Binary::Power => a.powf(b),
    }
}

/// `op` over a span, matched once, so each loop over the frames is of one formula.
fn apply_unary(op: Unary, out: &mut [f32], x: &[f32]) {
    with_unary!(op, kind => map(out, x, |x| unary(kind, x)));
}

/// `op` over a span, matched once, so each loop over the frames is of one formula.
fn apply_binary(op: Binary, out: &mut [f32], a: &[f32], b: &[f32]) {
    with_binary!(op, kind => map2(out, a, b, |a, b| binary(kind, a, b)));
}

#[inline(always)]
fn clamp(x: f32, low: f32, high: f32) -> f32 {
    x.max(low).min(high)
}

#[inline(always)]
fn mix(a: f32, b: f32, amount: f32) -> f32 {
    a + (b - a) * amount
}

/// The phase a `phasor` is at, and moves it on by `hz`.
#[inline(always)]
fn phasor(phase: &mut f32, hz: f32, sample_rate: f32) -> f32 {
    let now = *phase;
    let next = now + hz / sample_rate;
    *phase = if next.is_finite() {
        next - next.floor()
    } else {
        0.0
    };
    now
}

/// What `line` holds from `ms` ago at `position`, and `input` into it there.
#[inline(always)]
fn delayed(line: &mut DelayLine, position: usize, input: f32, ms: f32, sample_rate: f32) -> f32 {
    let out = line.read_between(position, ms * 0.001 * sample_rate);
    line.write(position, finite(input));
    out
}

/// Whether a trigger fires on a frame: where `when` is above 0.
#[inline(always)]
fn fires(when: f32) -> bool {
    when > 0.0
}

/// What is `held`, which is `input` where `when` fires.
#[inline(always)]
fn hold(held: &mut f32, input: f32, when: f32) -> f32 {
    if fires(when) {
        *held = finite(input);
    }
    *held
}

/// 1 where `now` is above 0 and the value `before` was not.
#[inline(always)]
fn rise(before: &mut f32, now: f32) -> f32 {
    let rose = *before <= 0.0 && now > 0.0;
    *before = finite(now);
    truth(rose)
}

/// 1 where `now` is not the value `before`.
#[inline(always)]
fn change(before: &mut f32, now: f32) -> f32 {
    let now = finite(now);
    let changed = *before != now;
    *before = now;
    truth(changed)
}

/// `value` into the slot of `index` of a `buffer`, and what it wrote.
#[inline(always)]
fn write(buffer: Option<&mut Vec<f32>>, index: f32, value: f32) -> f32 {
    let value = finite(value);
    if let Some(values) = buffer
        && let Some(slot) = wrapped(values.len(), index)
        && let Some(sample) = values.get_mut(slot)
    {
        *sample = value;
    }
    value
}

/// What leaves the code: held to [`LIMIT`], and 0 where it is not a number.
fn held(sample: f32) -> f32 {
    finite(sample).clamp(-LIMIT, LIMIT)
}

/// A value that is not a number, or infinite, would stay in a memory for good. It is 0.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}
