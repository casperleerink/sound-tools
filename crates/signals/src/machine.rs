//! Runs compiled [`Code`] one frame at a time, for both channels, with the memory of every
//! feedback, `phasor`, `delay`, filter, envelope and buffer of the code: the memory of one
//! voice. Made on the control thread, where it allocates all of it; running it allocates
//! nothing.
//!
//! What every voice shares, the values of the record and of the interface, the note and the
//! transport, the processor gives each frame as [`Inputs`].

use sound_core::{CHANNELS, DelayLine, SVF_MAX_Q, SvfFactors, SvfSection, amplitude, soft_clip};

use crate::code::{
    Binary, Code, FilterKind, MAX_PARAMETERS, Operation, Output, Register, Table, Unary,
};

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
    /// The first frame of the note.
    pub(crate) onset: bool,
}

/// What every voice reads in one frame, from the processor.
pub(crate) struct Inputs<'a> {
    pub(crate) input: [f32; CHANNELS],
    pub(crate) parameters: &'a [f32],
    pub(crate) lives: &'a [f32],
    pub(crate) arrays: &'a [Vec<f32>],
    /// One bit per trigger, set in the frame it fires.
    pub(crate) triggers: u32,
    pub(crate) beat: f64,
    pub(crate) bpm: f32,
    pub(crate) playing: bool,
    pub(crate) note: Note,
}

#[derive(Clone)]
pub struct Machine {
    code: Code,
    sample_rate: f32,
    registers: Vec<f32>,
    channels: [Memory; CHANNELS],
    /// Where the delays and buffers write the next frame.
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
        let channels = std::array::from_fn(|channel| Memory {
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
        Self {
            sample_rate,
            registers: vec![0.0; code.operations.len()],
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

    /// One frame of both channels.
    pub(crate) fn frame(&mut self, inputs: &Inputs<'_>) -> [f32; CHANNELS] {
        let mut output = [0.0; CHANNELS];
        if let Output::Stereo(left, right) = self.code.output {
            // Once, with the memory of the left channel, for both.
            self.run(0, inputs);
            self.keep_watches();
            output = [left, right].map(|register| held(self.register(register)));
        } else {
            for (channel, sample) in output.iter_mut().enumerate() {
                *sample = self.run(channel, inputs);
                if channel == 0 {
                    self.keep_watches();
                }
            }
        }
        self.position = self.position.wrapping_add(1);
        output
    }

    fn register(&self, register: Register) -> f32 {
        self.registers
            .get(usize::from(register))
            .copied()
            .unwrap_or(0.0)
    }

    /// The watches show the left channel.
    fn keep_watches(&mut self) {
        for (watched, register) in self.watched.iter_mut().zip(&self.code.watch_registers) {
            *watched = self
                .registers
                .get(usize::from(*register))
                .copied()
                .unwrap_or(0.0);
        }
    }

    fn run(&mut self, channel: usize, inputs: &Inputs<'_>) -> f32 {
        let Self {
            code,
            sample_rate,
            registers,
            channels,
            position,
            ..
        } = self;
        let sample_rate = *sample_rate;
        let Some(memory) = channels.get_mut(channel) else {
            return 0.0;
        };
        let input = inputs.input.get(channel).copied().unwrap_or(0.0);
        let note = inputs.note;
        for (index, operation) in code.operations.iter().enumerate() {
            let read =
                |register: Register| registers.get(usize::from(register)).copied().unwrap_or(0.0);
            let value = match *operation {
                Operation::Constant(value) => value,
                Operation::Input => input,
                Operation::InputLeft => inputs.input[0],
                Operation::InputRight => inputs.input[1],
                Operation::Channel => channel as f32,
                Operation::SampleRate => sample_rate,
                Operation::Beat => inputs.beat as f32,
                Operation::Bpm => inputs.bpm,
                Operation::Playing => truth(inputs.playing),
                Operation::Frequency => note.frequency,
                Operation::Pitch => note.pitch,
                Operation::Gate => truth(note.gate),
                Operation::Velocity => note.velocity,
                Operation::Onset => truth(note.onset),
                Operation::Parameter(index) => at(inputs.parameters, index),
                Operation::Live(index) => at(inputs.lives, index),
                Operation::Trigger(index) => truth(inputs.triggers & (1 << index) != 0),
                Operation::History(slot) => at(&memory.histories, slot),
                Operation::Unary(unary, x) => apply_unary(unary, read(x)),
                Operation::Binary(binary, a, b) => apply_binary(binary, read(a), read(b)),
                Operation::Clamp(x, low, high) => read(x).max(read(low)).min(read(high)),
                Operation::Mix(a, b, amount) => {
                    let (a, b) = (read(a), read(b));
                    a + (b - a) * read(amount)
                }
                Operation::Phasor { hz, slot } => match memory.phases.get_mut(usize::from(slot)) {
                    Some(phase) => {
                        let value = *phase;
                        let next = *phase + read(hz) / sample_rate;
                        *phase = if next.is_finite() {
                            next - next.floor()
                        } else {
                            0.0
                        };
                        value
                    }
                    None => 0.0,
                },
                Operation::Noise { slot } => match memory.noises.get_mut(usize::from(slot)) {
                    Some(state) => {
                        // xorshift32
                        *state ^= *state << 13;
                        *state ^= *state >> 17;
                        *state ^= *state << 5;
                        *state as f32 / u32::MAX as f32 * 2.0 - 1.0
                    }
                    None => 0.0,
                },
                Operation::Delay { input, ms, slot } => {
                    match memory.delays.get_mut(usize::from(slot)) {
                        Some(line) => {
                            let frames = read(ms) * 0.001 * sample_rate;
                            let value = line.read_between(*position, frames);
                            line.write(*position, finite(read(input)));
                            value
                        }
                        None => 0.0,
                    }
                }
                Operation::Filter {
                    kind,
                    input,
                    hz,
                    q,
                    slot,
                } => match memory.filters.get_mut(usize::from(slot)) {
                    Some(filter) => filter.next(kind, read(input), read(hz), read(q), sample_rate),
                    None => 0.0,
                },
                Operation::Smooth { input, ms, slot } => {
                    match memory.smooths.get_mut(usize::from(slot)) {
                        Some(smooth) => smooth.next(read(input), read(ms), sample_rate),
                        None => 0.0,
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
                    Some(envelope) => {
                        let times = [read(attack), read(decay), read(release)]
                            .map(|ms| (ms * 0.001 * sample_rate).max(1.0));
                        // A sustain that is not a number would stay in the level for good.
                        let sustain = finite(read(sustain)).clamp(0.0, 1.0);
                        envelope.next(read(gate) > 0.0, note.onset, times, sustain)
                    }
                    None => 0.0,
                },
                Operation::Hold { input, when, slot } => {
                    match memory.memories.get_mut(usize::from(slot)) {
                        Some(held) => {
                            if read(when) > 0.0 {
                                *held = finite(read(input));
                            }
                            *held
                        }
                        None => 0.0,
                    }
                }
                Operation::Rise { input, slot } => match memory.memories.get_mut(usize::from(slot))
                {
                    Some(before) => {
                        let now = read(input);
                        let rose = *before <= 0.0 && now > 0.0;
                        *before = finite(now);
                        truth(rose)
                    }
                    None => 0.0,
                },
                Operation::Change { input, slot } => {
                    match memory.memories.get_mut(usize::from(slot)) {
                        Some(before) => {
                            let now = finite(read(input));
                            let changed = *before != now;
                            *before = now;
                            truth(changed)
                        }
                        None => 0.0,
                    }
                }
                Operation::Read { table, index } => {
                    let values = table_of(table, inputs.arrays, &memory.buffers);
                    read_at(values, read(index))
                }
                Operation::Lookup { table, phase } => {
                    let values = table_of(table, inputs.arrays, &memory.buffers);
                    look_up(values, read(phase))
                }
                Operation::Length(table) => {
                    table_of(table, inputs.arrays, &memory.buffers).len() as f32
                }
                Operation::Write {
                    buffer,
                    index,
                    value,
                } => {
                    let value = finite(read(value));
                    if let Some(values) = memory.buffers.get_mut(usize::from(buffer))
                        && let Some(slot) = wrapped(values.len(), read(index))
                        && let Some(sample) = values.get_mut(slot)
                    {
                        *sample = value;
                    }
                    value
                }
            };
            if let Some(register) = registers.get_mut(index) {
                *register = value;
            }
        }
        for (slot, register) in &code.history_writes {
            let value = registers
                .get(usize::from(*register))
                .copied()
                .unwrap_or(0.0);
            if let Some(history) = memory.histories.get_mut(usize::from(*slot)) {
                *history = finite(value);
            }
        }
        held(match code.output {
            Output::Mono(register) => registers.get(usize::from(register)).copied().unwrap_or(0.0),
            Output::Stereo(..) => input,
        })
    }
}

fn at(values: &[f32], index: u16) -> f32 {
    values.get(usize::from(index)).copied().unwrap_or(0.0)
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

fn apply_unary(unary: Unary, x: f32) -> f32 {
    match unary {
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

fn apply_binary(binary: Binary, a: f32, b: f32) -> f32 {
    match binary {
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

/// What leaves the code: held to [`LIMIT`], and 0 where it is not a number.
fn held(sample: f32) -> f32 {
    finite(sample).clamp(-LIMIT, LIMIT)
}

/// A value that is not a number, or infinite, would stay in a memory for good. It is 0.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}
