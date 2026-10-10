//! Runs compiled [`Code`] over a span of frames of a block, for both channels, with the memory
//! of every feedback, `phasor`, `delay`, filter, envelope and buffer of the code: the memory of
//! one voice. Made on the control thread, where it allocates all of it; running it allocates
//! nothing.
//!
//! What every voice shares, the values of the record and of the interface and the transport,
//! the processor works out once per block as a [`Block`]. The note of a voice is the same over
//! the span the voice plays: the processor splits a block where a note comes or goes.

use std::ops::Range;

use sound_core::{
    CHANNELS, DelayLine, MAX_BLOCK, SVF_MAX_Q, SvfFactors, SvfSection, amplitude, soft_clip,
};

use crate::code::{
    Binary, Code, FilterKind, MAX_LIVES, MAX_PARAMETERS, Operation, Output, Register, Table, Unary,
};
use crate::program::{self, Step};

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
        // The buffers of every channel are as long.
        let buffers = channels.first().map_or(&[][..], |memory| &memory.buffers);
        let mut steps = program::schedule(&code).into_vec();
        // What is the same in every frame for as long as the machine lives is set once, and is
        // no step.
        steps.retain(|step| {
            let Step::Block(register) = step else {
                return true;
            };
            let register = usize::from(*register);
            let value = match code.operations.get(register) {
                Some(Operation::Constant(value)) => *value,
                Some(Operation::SampleRate) => sample_rate,
                Some(Operation::Length(table @ Table::Buffer(_))) => {
                    table_of(*table, &[], buffers).len() as f32
                }
                _ => return true,
            };
            if let Some(row) = registers.get_mut(register) {
                row.fill(value);
            }
            false
        });
        Self {
            per_channel: program::per_channel(&code, &steps),
            steps: steps.into_boxed_slice(),
            sample_rate,
            registers,
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
                    if let Some(operation) = operations.get(register) {
                        span.run(*operation, register, frames.clone(), registers, memory);
                    }
                }
                Step::Loop(looped) => {
                    for frame in frames.clone() {
                        for (register, operation) in looped.iter() {
                            span.run(
                                *operation,
                                usize::from(*register),
                                frame..frame + 1,
                                registers,
                                memory,
                            );
                        }
                    }
                }
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
    /// Inlined: a loop of a feedback calls it for every operation in every frame.
    ///
    /// A memory is kept in a local over the frames and put back after: left in its slot, it is
    /// stored and loaded again in every frame, which lengthens the chain from one frame to the
    /// next.
    #[inline(always)]
    fn run(
        &self,
        operation: Operation,
        register: usize,
        frames: Range<usize>,
        registers: &mut [[f32; MAX_BLOCK]],
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
                let source = self.feedbacks.get(usize::from(slot)).map(|source| {
                    let source = usize::from(*source);
                    match source.checked_sub(register + 1) {
                        Some(index) => after.get(index),
                        None => before.get(source),
                    }
                });
                self.history(slot, out, frames, source.flatten(), &memory.histories);
            }
            Operation::Unary(unary, x) => apply_unary(unary, out, read(x)),
            Operation::Binary(binary, a, b) => apply_binary(binary, out, read(a), read(b)),
            Operation::Clamp(x, low, high) => {
                map3(out, read(x), read(low), read(high), |x, low, high| {
                    x.max(low).min(high)
                });
            }
            Operation::Mix(a, b, amount) => {
                map3(out, read(a), read(b), read(amount), |a, b, amount| {
                    a + (b - a) * amount
                });
            }
            Operation::Phasor { hz, slot } => match memory.phases.get_mut(usize::from(slot)) {
                Some(kept) => {
                    let mut phase = *kept;
                    for (out, hz) in out.iter_mut().zip(read(hz)) {
                        *out = phase;
                        let next = phase + hz / sample_rate;
                        phase = if next.is_finite() {
                            next - next.floor()
                        } else {
                            0.0
                        };
                    }
                    *kept = phase;
                }
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
                            let frames = ms * 0.001 * sample_rate;
                            *out = line.read_between(position, frames);
                            line.write(position, finite(*input));
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
                Some(kept) => {
                    let mut filter = kept.clone();
                    let values = read(input).iter().zip(read(hz)).zip(read(q));
                    for (out, ((input, hz), q)) in out.iter_mut().zip(values) {
                        *out = filter.next(kind, *input, *hz, *q, sample_rate);
                    }
                    *kept = filter;
                }
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
                Some(kept) => {
                    let mut envelope = kept.clone();
                    let times = read(attack).iter().zip(read(decay)).zip(read(release));
                    let values = read(gate).iter().zip(read(sustain)).zip(times);
                    let outs = out.iter_mut().zip(frames.clone());
                    for ((out, frame), ((gate, sustain), ((attack, decay), release))) in
                        outs.zip(values)
                    {
                        let times = [*attack, *decay, *release]
                            .map(|ms| (ms * 0.001 * sample_rate).max(1.0));
                        // A sustain that is not a number would stay in the level for good.
                        let sustain = finite(*sustain).clamp(0.0, 1.0);
                        *out = envelope.next(*gate > 0.0, self.onset(frame), times, sustain);
                    }
                    *kept = envelope;
                }
                None => out.fill(0.0),
            },
            Operation::Hold { input, when, slot } => {
                match memory.memories.get_mut(usize::from(slot)) {
                    Some(kept) => {
                        let mut held = *kept;
                        for ((out, input), when) in out.iter_mut().zip(read(input)).zip(read(when))
                        {
                            if *when > 0.0 {
                                held = finite(*input);
                            }
                            *out = held;
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
                        let rose = before <= 0.0 && *now > 0.0;
                        before = finite(*now);
                        *out = truth(rose);
                    }
                    *kept = before;
                }
                None => out.fill(0.0),
            },
            Operation::Change { input, slot } => match memory.memories.get_mut(usize::from(slot)) {
                Some(kept) => {
                    let mut before = *kept;
                    for (out, now) in out.iter_mut().zip(read(input)) {
                        let now = finite(*now);
                        let changed = before != now;
                        before = now;
                        *out = truth(changed);
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
                    let value = finite(*value);
                    if let Some(values) = memory.buffers.get_mut(usize::from(buffer))
                        && let Some(slot) = wrapped(values.len(), *index)
                        && let Some(sample) = values.get_mut(slot)
                    {
                        *sample = value;
                    }
                    *out = value;
                }
            }
        }
    }

    /// A feedback read: what its `source` was in the frame before, and in the first frame of the
    /// span what was set in the span before. The source may come after it.
    fn history(
        &self,
        slot: u16,
        out: &mut [f32],
        frames: Range<usize>,
        source: Option<&[f32; MAX_BLOCK]>,
        histories: &[f32],
    ) {
        let set_before = histories.get(usize::from(slot)).copied().unwrap_or(0.0);
        for (out, frame) in out.iter_mut().zip(frames) {
            *out = match source {
                _ if frame == self.first => set_before,
                Some(source) => finite(source.get(frame - 1).copied().unwrap_or(0.0)),
                None => 0.0,
            };
        }
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
    /// `frames` are of the attack, the decay and the release. A note that takes over a voice
    /// whose gate is still up, as one does under the sustain pedal, starts the attack at its
    /// `onset`: the gate it sees never fell.
    fn next(
        &mut self,
        gate: bool,
        onset: bool,
        [attack, decay, release]: [f32; 3],
        sustain: f32,
    ) -> f32 {
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

fn apply_unary(unary: Unary, out: &mut [f32], x: &[f32]) {
    match unary {
        Unary::Negate => map(out, x, |x| -x),
        Unary::Sin => map(out, x, f32::sin),
        Unary::Cos => map(out, x, f32::cos),
        Unary::Tan => map(out, x, f32::tan),
        Unary::Tanh => map(out, x, f32::tanh),
        Unary::Abs => map(out, x, f32::abs),
        Unary::Sqrt => map(out, x, |x| x.max(0.0).sqrt()),
        Unary::Exp => map(out, x, f32::exp),
        Unary::Log => map(out, x, |x| x.max(f32::MIN_POSITIVE).ln()),
        Unary::Floor => map(out, x, f32::floor),
        Unary::Wrap => map(out, x, |x| x - x.floor()),
        Unary::Decibels => map(out, x, amplitude),
        Unary::Saturate => map(out, x, soft_clip),
    }
}

fn apply_binary(binary: Binary, out: &mut [f32], a: &[f32], b: &[f32]) {
    match binary {
        Binary::Add => map2(out, a, b, |a, b| a + b),
        Binary::Subtract => map2(out, a, b, |a, b| a - b),
        Binary::Multiply => map2(out, a, b, |a, b| a * b),
        Binary::Divide => map2(out, a, b, |a, b| a / b),
        Binary::Remainder => map2(out, a, b, f32::rem_euclid),
        Binary::Less => map2(out, a, b, |a, b| truth(a < b)),
        Binary::Greater => map2(out, a, b, |a, b| truth(a > b)),
        Binary::LessOrEqual => map2(out, a, b, |a, b| truth(a <= b)),
        Binary::GreaterOrEqual => map2(out, a, b, |a, b| truth(a >= b)),
        Binary::Equal => map2(out, a, b, |a, b| truth(a == b)),
        Binary::NotEqual => map2(out, a, b, |a, b| truth(a != b)),
        Binary::Min => map2(out, a, b, f32::min),
        Binary::Max => map2(out, a, b, f32::max),
        Binary::Power => map2(out, a, b, f32::powf),
    }
}

/// What leaves the code: held to [`LIMIT`], and 0 where it is not a number.
fn held(sample: f32) -> f32 {
    finite(sample).clamp(-LIMIT, LIMIT)
}

/// A value that is not a number, or infinite, would stay in a memory for good. It is 0.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}
