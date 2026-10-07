//! Runs compiled [`Code`] one frame at a time, for both channels, with the memory of every
//! `history`, `phasor`, `delay`, filter and `smooth` of the script. Made on the control
//! thread, where it allocates all of it; running it allocates nothing.

use sound_core::{
    CHANNELS, DelayLine, SVF_MAX_Q, Smoothed, SvfFactors, SvfSection, amplitude, soft_clip,
};

use crate::language::{
    Binary, Code, FilterKind, MAX_DELAY_MS, MAX_PARAMETERS, Operation, Register, Unary,
};

/// Where every param stands, in the order of the `param` lines.
pub type Values = [f32; MAX_PARAMETERS];

/// How long a new value of a param takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// What leaves the script is held to this, about 12 dB over full scale, so a feedback that
/// runs away is loud but not deafening.
const LIMIT: f32 = 4.0;

pub struct Machine {
    code: Code,
    sample_rate: f32,
    parameters: Vec<Smoothed>,
    /// The values of the parameters in this frame.
    current: Vec<f32>,
    registers: Vec<f32>,
    channels: [Memory; CHANNELS],
    /// Where the delays write the next frame.
    position: usize,
}

/// The memory of one channel.
struct Memory {
    histories: Vec<f32>,
    phases: Vec<f32>,
    noises: Vec<u32>,
    delays: Vec<DelayLine>,
    filters: Vec<Filter>,
    smooths: Vec<Smooth>,
}

#[derive(Default)]
struct Filter {
    section: SvfSection,
    factors: SvfFactors,
    /// The damping, `1 / Q`, which a band and a high pass need too.
    k: f32,
    /// The cutoff and Q the factors are for, so a `tan` runs only when they move.
    hz: f32,
    q: f32,
}

#[derive(Default)]
struct Smooth {
    value: f32,
    ms: f32,
    factor: f32,
}

impl Machine {
    /// Starts at `values`, with no glide in.
    pub fn new(code: Code, values: &Values, sample_rate: f32) -> Self {
        let slots = &code.slots;
        let delay_frames = (MAX_DELAY_MS * 0.001 * sample_rate) as usize;
        let channels = std::array::from_fn(|channel| Memory {
            histories: vec![0.0; usize::from(slots.histories)],
            phases: vec![0.0; usize::from(slots.phasors)],
            // A fixed seed per slot and channel, so a render is the same every time and the
            // two channels hear different noise.
            noises: (0..u32::from(slots.noises))
                .map(|slot| 0x9E37_79B9 ^ (slot * 2 + channel as u32 + 1).wrapping_mul(0x85EB_CA6B))
                .collect(),
            delays: (0..slots.delays)
                .map(|_| DelayLine::new(delay_frames + 4))
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
        });
        let parameters = values
            .iter()
            .take(code.parameters.len())
            .map(|value| Smoothed::new(*value))
            .collect();
        Self {
            sample_rate,
            parameters,
            current: values.iter().take(code.parameters.len()).copied().collect(),
            registers: vec![0.0; code.operations.len()],
            channels,
            position: 0,
            code,
        }
    }

    /// Which source this runs, to tell new code from new values.
    pub fn hash(&self) -> u64 {
        self.code.hash
    }

    /// Glides every param to its new value.
    pub fn aim(&mut self, values: &Values) {
        let ramp = RAMP_SECONDS * self.sample_rate;
        for (parameter, value) in self.parameters.iter_mut().zip(values) {
            parameter.set_target(*value, ramp);
        }
    }

    /// One frame of both channels.
    pub fn frame(&mut self, input: [f32; CHANNELS]) -> [f32; CHANNELS] {
        for (current, parameter) in self.current.iter_mut().zip(&mut self.parameters) {
            *current = parameter.advance(1);
        }
        let mut output = [0.0; CHANNELS];
        for (channel, sample) in output.iter_mut().enumerate() {
            *sample = self.run(channel, input[channel]);
        }
        self.position = self.position.wrapping_add(1);
        output
    }

    fn run(&mut self, channel: usize, input: f32) -> f32 {
        let Self {
            code,
            sample_rate,
            current,
            registers,
            channels,
            position,
            ..
        } = self;
        let sample_rate = *sample_rate;
        let Some(memory) = channels.get_mut(channel) else {
            return 0.0;
        };
        for (index, operation) in code.operations.iter().enumerate() {
            let read =
                |register: Register| registers.get(usize::from(register)).copied().unwrap_or(0.0);
            let value = match *operation {
                Operation::Constant(value) => value,
                Operation::Input => input,
                Operation::Channel => channel as f32,
                Operation::SampleRate => sample_rate,
                Operation::Parameter(index) => {
                    current.get(usize::from(index)).copied().unwrap_or(0.0)
                }
                Operation::History(slot) => memory
                    .histories
                    .get(usize::from(slot))
                    .copied()
                    .unwrap_or(0.0),
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
        let output = match code.output {
            Some(register) => registers.get(usize::from(register)).copied().unwrap_or(0.0),
            None => input,
        };
        finite(output).clamp(-LIMIT, LIMIT)
    }
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
    let truth = |condition: bool| if condition { 1.0 } else { 0.0 };
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

/// A value that is not a number, or infinite, would stay in a memory for good. It is 0.
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}
