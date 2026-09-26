//! The reverb processor: input filters, a pre-delay, a chain of diffusers, and a feedback delay
//! network of sixteen lines (Jot and Chaigne, "Digital delay networks for designing artificial
//! reverberators", 1991).
//!
//! Why this one: a feedback delay network is the plainest reverb whose decay time is a formula
//! and not a tuning. Every line loses the same dB per second, so the tail falls by 60 dB in
//! exactly the decay time, whatever the size. The lines are mixed by a Hadamard matrix, which
//! loses no energy, so a gain of 1 on every line holds the tail for ever: that is freeze. And
//! each line's loss is a one-pole low pass whose gain at 5 kHz is set by the same formula, so
//! the highs die in their own time. A plate of Dattorro's has a fixed structure whose decay is
//! set by ear; it gives none of this.
//!
//! What comes in goes through a low cut and a high cut, then the pre-delay, then four allpass
//! diffusers per channel that smear each click into a burst, then into the lines. The first time
//! each line comes out is an early reflection; after that the Hadamard matrix mixes them into
//! the tail. The tail is scaled by the loss of the loop, so it comes out at the level of the
//! input at every size and decay.
//!
//! No modulation and no randomness: a render is the same every time and does not depend on
//! where a session started.
//!
//! Every delay line is allocated when the processor is made and again when the sample rate is
//! set, for the largest size and pre-delay, and never in `process`. A change of size or
//! pre-delay does not move a read position: it fades over 20 ms from the old tap to the new one,
//! so nothing clicks and no pitch slides.

use std::f32::consts::{LOG2_10, PI, TAU};

use sound_core::{
    AudioInput, AudioOutput, CHANNELS, Ports, PrepareConfig, ProcessContext, Processor, Smoothed,
};

use crate::{PRE_DELAY, ReverbState};

/// How long a change takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// The delay lines of the network.
const LINES: usize = 16;

/// The lengths of the lines at size 1, in seconds. Spread from 37 to 97 ms on a ratio, with no
/// two of them in a simple ratio, so their echoes never pile up on one another.
const LINE_SECONDS: [f32; LINES] = [
    0.0371, 0.0403, 0.0437, 0.0469, 0.0503, 0.0539, 0.0577, 0.0613, 0.0651, 0.0691, 0.0733, 0.0779,
    0.0823, 0.0869, 0.0917, 0.0971,
];

/// Size is a ratio: size 0 makes every line a tenth of its length at size 1.
const SMALLEST_SCALE: f32 = 0.1;

/// The allpass diffusers of each channel, in seconds. Short and a little different for left
/// and right, so they blur each reflection without a delay of their own anyone would hear.
const DIFFUSERS: usize = 4;
const DIFFUSER_SECONDS: [[f32; DIFFUSERS]; CHANNELS] = [
    [0.002_13, 0.002_87, 0.003_71, 0.004_63],
    [0.002_29, 0.003_07, 0.003_53, 0.004_81],
];

/// The allpass factor at diffusion 1. Over 0.75 an allpass rings on its own.
const MOST_DIFFUSION: f32 = 0.7;

/// The frequency at which damping sets the decay of the highs.
pub const DAMPED_HZ: f32 = 5_000.0;

/// At damping 1 the highs die in this part of the decay time.
const SHORTEST_HIGHS: f32 = 0.1;

/// The damping filter of a line takes at most about 36 dB more from the highs than from the
/// rest on each pass. A pole at 1 would hold a constant for ever.
const MOST_POLE: f32 = 0.99;

/// The cuts stay under this part of the sample rate, below the Nyquist frequency.
const HIGHEST_PART: f32 = 0.45;

/// The level of the tail, on top of the loss of the loop. With it, noise comes out of the reverb
/// alone at about its own level, at every size and decay.
const OUTPUT: f32 = 0.533;

/// The frequencies at which the level of the tail is worked out, evenly over the band of most
/// sound, as a noise has its power.
const HEARD_HZ: [f32; 8] = [
    500.0, 1_500.0, 2_500.0, 3_500.0, 4_500.0, 5_500.0, 6_500.0, 7_500.0,
];

/// While factors move, they are worked out again this often.
const FACTOR_FRAMES: usize = 16;

/// Input louder than this, or not a number, is held to it, so no sample of anyone else's can
/// make the tail infinite. +36 dBFS: nothing real comes near it.
const INPUT_LIMIT: f32 = 64.0;

/// While the input is silent and nothing in the lines is louder than this, -180 dB, the reverb
/// has rung out: it does no work and its output is silent.
const REST: f32 = 1e-9;

/// The lengths of the lines at a size, in frames: each the prime nearest to its length, so no
/// two lines share a factor and their echoes never fall on one another.
fn line_frames(size: f32, sample_rate: f32) -> [usize; LINES] {
    let scale = SMALLEST_SCALE.powf(1.0 - size.clamp(0.0, 1.0));
    LINE_SECONDS.map(|seconds| nearest_prime(frames_of(seconds * scale, sample_rate)))
}

/// When each early reflection comes, after the pre-delay and the diffusers, as a part of when
/// the last one comes. The same at every size: size scales them all.
pub fn reflections() -> [f32; LINES] {
    LINE_SECONDS.map(|seconds| seconds / longest_line_seconds())
}

/// The prime nearest to `number`, the lower one of two as near. Trial division: a line is at most
/// some twenty thousand frames, so this is a few hundred steps, done on an update and not per
/// frame.
fn nearest_prime(number: usize) -> usize {
    let is_prime = |candidate: usize| {
        candidate >= 2
            && (2..)
                .take_while(|divisor| divisor * divisor <= candidate)
                .all(|divisor| !candidate.is_multiple_of(divisor))
    };
    (0..number)
        .flat_map(|distance| [number - distance, number + distance])
        .find(|candidate| is_prime(*candidate))
        .unwrap_or(2)
}

/// The longest line of the largest room.
fn longest_line_seconds() -> f32 {
    LINE_SECONDS[LINES - 1]
}

/// The time in which the tail falls by 60 dB at [`DAMPED_HZ`]. The rest falls by 60 dB in the
/// decay time.
pub fn high_decay_seconds(state: &ReverbState) -> f32 {
    state.decay_seconds * highs_part(state.damping)
}

fn highs_part(damping: f32) -> f32 {
    1.0 - (1.0 - SHORTEST_HIGHS) * damping
}

/// A delay line whose length is a power of two, so a position wraps with a mask and a read can
/// never be out of it.
struct DelayLine {
    buffer: Vec<f32>,
    mask: usize,
}

impl DelayLine {
    /// Holds at least `frames` frames of delay. Allocates.
    fn new(frames: usize) -> Self {
        let length = (frames + 1).next_power_of_two();
        Self {
            buffer: vec![0.0; length],
            mask: length - 1,
        }
    }

    /// What was written `delay` frames before `position`.
    fn read(&self, position: usize, delay: usize) -> f32 {
        // The mask keeps the index inside the buffer, whose length is the mask plus one.
        self.buffer[position.wrapping_sub(delay) & self.mask]
    }

    fn write(&mut self, position: usize, value: f32) {
        self.buffer[position & self.mask] = value;
    }
}

/// Where a group of delay lines is read. A new length does not move a read position: the read
/// fades from the old tap to the new one. A length that comes during a fade waits for its end,
/// so a drag through many sizes is a row of fades, each from where the last one ended.
#[derive(Clone, Copy)]
struct Taps<const N: usize> {
    from: [usize; N],
    to: [usize; N],
    next: [usize; N],
    /// From 0 at `from` to 1 at `to`.
    fade: f32,
    step: f32,
}

impl<const N: usize> Taps<N> {
    fn new(taps: [usize; N]) -> Self {
        Self {
            from: taps,
            to: taps,
            next: taps,
            fade: 1.0,
            step: 1.0,
        }
    }

    fn aim(&mut self, next: [usize; N], fade_frames: f32) {
        self.next = next;
        self.step = 1.0 / fade_frames;
        if !self.is_fading() {
            self.start();
        }
    }

    fn start(&mut self) {
        if self.next != self.to {
            self.from = self.to;
            self.to = self.next;
            self.fade = 0.0;
        }
    }

    fn is_fading(&self) -> bool {
        self.from != self.to
    }

    /// The part of the new tap in this frame.
    fn weight(&self) -> f32 {
        if self.is_fading() { self.fade } else { 1.0 }
    }

    /// One frame along, after every read of the frame: a fade that ends here may start the
    /// next one, between other taps.
    fn advance(&mut self) {
        if !self.is_fading() {
            return;
        }
        self.fade += self.step;
        if self.fade >= 1.0 {
            self.from = self.to;
            self.fade = 1.0;
            self.start();
        }
    }

    /// Takes the newest taps at once.
    fn snap(&mut self) {
        *self = Self::new(self.next);
    }

    /// The length of tap `index` now, between the two it fades between.
    fn length(&self, index: usize) -> f32 {
        let (from, to) = (self.from[index] as f32, self.to[index] as f32);
        from + (to - from) * self.fade
    }

    /// Reads tap `index` of `line` at `position`, with `fade` of the new tap.
    fn read(&self, line: &DelayLine, index: usize, position: usize, fade: f32) -> f32 {
        let to = line.read(position, self.to[index]);
        if fade >= 1.0 {
            return to;
        }
        let from = line.read(position, self.from[index]);
        from + (to - from) * fade
    }
}

/// A one-pole filter in its trapezoidal form: stable at every cutoff, also while it moves.
#[derive(Clone, Copy, Default)]
struct OnePole {
    memory: f32,
}

impl OnePole {
    /// The factor of a cutoff: `g / (1 + g)` with `g = tan(π cutoff / sample rate)`.
    fn factor(hz: f32, sample_rate: f32) -> f32 {
        let hz = hz.max(1.0).min(HIGHEST_PART * sample_rate);
        let g = (PI * hz / sample_rate).tan();
        g / (1.0 + g)
    }

    /// The low pass of one sample.
    fn low(&mut self, factor: f32, input: f32) -> f32 {
        let step = (input - self.memory) * factor;
        let low = step + self.memory;
        self.memory = low + step;
        low
    }

    /// The high pass of one sample.
    fn high(&mut self, factor: f32, input: f32) -> f32 {
        input - self.low(factor, input)
    }
}

/// The sign of line `index` in the left and the right output, and in what the left and the
/// right input put into it. Two rows of a Hadamard matrix: each line is in both sides, and the
/// two sides are as different as sixteen lines allow.
fn signs(index: usize) -> [f32; CHANNELS] {
    let sign = |bit: usize| if index & bit == 0 { 1.0 } else { -1.0 };
    [sign(1), sign(2)]
}

/// The Hadamard matrix of sixteen, over 4: every line goes into every other with the same
/// weight, a quarter, and a sign. It is orthogonal, so it keeps the energy exactly, and a
/// quarter is exact in floating point, so a frozen tail is only rounded, never scaled. Four
/// rounds of sums and differences, the fast Walsh-Hadamard transform.
fn hadamard(mut lines: [f32; LINES]) -> [f32; LINES] {
    let mut half = 1;
    while half < LINES {
        for start in (0..LINES).step_by(2 * half) {
            for index in start..start + half {
                let (a, b) = (lines[index], lines[index + half]);
                lines[index] = a + b;
                lines[index + half] = a - b;
            }
        }
        half *= 2;
    }
    lines.map(|line| line * 0.25)
}

/// The loss of one line for one pass: its gain at 0 Hz and the pole of its low pass.
///
/// A line of `length` frames loses `3 length / (decay × sample rate)` of 60 dB per pass, so the
/// tail falls by 60 dB in the decay time. The one-pole low pass `k (1 - p) / (1 - p z⁻¹)` keeps
/// that at 0 Hz and takes more at [`DAMPED_HZ`], so there the tail falls by 60 dB in the part
/// of the decay time that the damping says. The pole comes from solving
/// `|H(damped)| = k^(1 / part)` for `p`.
fn line_loss(length: f32, decay: f32, part: f32, damped_cos: f32, sample_rate: f32) -> (f32, f32) {
    let gain = (-3.0 * LOG2_10 * length / (decay * sample_rate)).exp2();
    // What the highs lose on top of the rest, as a factor: `k^(1 / part - 1)`.
    let more = gain.powf(1.0 / part - 1.0);
    let square = more * more;
    let a = 1.0 - square;
    let b = 1.0 - square * damped_cos;
    // `(b - a)(b + a)`, written so that nothing cancels when `more` is near 1.
    let root = (square * (1.0 - damped_cos) * (2.0 - square * (1.0 + damped_cos))).sqrt();
    let pole = (a / (b + root)).min(MOST_POLE);
    (gain, pole)
}

/// A sample of the input as the reverb takes it: held to [`INPUT_LIMIT`], and silence for
/// anything that is not a number.
fn held(sample: f32) -> f32 {
    if sample.is_nan() {
        return 0.0;
    }
    sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
}

fn frames_of(seconds: f32, sample_rate: f32) -> usize {
    ((seconds * sample_rate).round() as usize).max(1)
}

pub struct Reverb {
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    /// The last record, so that a new sample rate can aim at it again.
    state: ReverbState,
    /// Where every delay line writes the next frame.
    position: usize,
    pre_delay: [DelayLine; CHANNELS],
    pre_delay_tap: Taps<1>,
    diffusers: [[DelayLine; DIFFUSERS]; CHANNELS],
    diffuser_frames: [[usize; DIFFUSERS]; CHANNELS],
    lines: [DelayLine; LINES],
    line_taps: Taps<LINES>,
    /// The memory of the damping filter of each line.
    damped: [f32; LINES],
    /// The factor on what each line reads, `k (1 - p)`, and the pole of its damping filter.
    line_gains: [f32; LINES],
    poles: [f32; LINES],
    /// The factor on the tail, at the end of the last run of frames and where it is going in the
    /// run now, so it moves frame by frame and makes no step.
    tail_gain: f32,
    tail_gain_target: f32,
    /// The low cut and the high cut of each channel, and their factors.
    cuts: [[OnePole; 2]; CHANNELS],
    cut_factors: [f32; 2],
    /// As `log2` of seconds and of hertz, so a glide moves on a ratio.
    decay: Smoothed,
    low_cut: Smoothed,
    high_cut: Smoothed,
    damping: Smoothed,
    diffusion: Smoothed,
    width: Smoothed,
    mix: Smoothed,
    /// 1 while frozen: every line keeps all it has.
    freeze: Smoothed,
    /// 0 while frozen: nothing new comes into the lines.
    input: Smoothed,
    /// Whether the factors have to be worked out again although nothing glides.
    stale: bool,
    /// Whether the tail factor takes its target at once, with nothing to glide from.
    snapped: bool,
    /// Frames in a row with a silent input and nothing audible in the lines.
    quiet_frames: usize,
}

impl Reverb {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Allocates its delay lines for 48 kHz, and again in `prepare` for another rate. Starts at
    /// these values, so a reverb that is added or opened does not glide in.
    pub fn new(state: ReverbState) -> Self {
        let sample_rate = 48_000.0;
        let mut reverb = Self {
            sample_rate,
            ramp_frames: RAMP_SECONDS * sample_rate,
            state,
            position: 0,
            pre_delay: [(); CHANNELS].map(|_| DelayLine::new(1)),
            pre_delay_tap: Taps::new([1]),
            diffusers: [(); CHANNELS].map(|_| [(); DIFFUSERS].map(|_| DelayLine::new(1))),
            diffuser_frames: [[1; DIFFUSERS]; CHANNELS],
            lines: [(); LINES].map(|_| DelayLine::new(1)),
            line_taps: Taps::new([1; LINES]),
            damped: [0.0; LINES],
            line_gains: [0.0; LINES],
            poles: [0.0; LINES],
            tail_gain: 0.0,
            tail_gain_target: 0.0,
            cuts: [[OnePole::default(); 2]; CHANNELS],
            cut_factors: [0.0; 2],
            decay: Smoothed::new(0.0),
            low_cut: Smoothed::new(0.0),
            high_cut: Smoothed::new(0.0),
            damping: Smoothed::new(0.0),
            diffusion: Smoothed::new(0.0),
            width: Smoothed::new(0.0),
            mix: Smoothed::new(0.0),
            freeze: Smoothed::new(0.0),
            input: Smoothed::new(0.0),
            stale: true,
            snapped: true,
            quiet_frames: 0,
        };
        reverb.allocate(sample_rate);
        reverb
    }

    /// Makes every delay line for a sample rate, empty, and takes the record at once.
    fn allocate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        let frames = |seconds| frames_of(seconds, sample_rate);
        self.pre_delay = [(); CHANNELS].map(|_| DelayLine::new(frames(PRE_DELAY.max / 1_000.0)));
        self.diffuser_frames = DIFFUSER_SECONDS.map(|channel| channel.map(frames));
        self.diffusers = self
            .diffuser_frames
            .map(|channel| channel.map(DelayLine::new));
        let longest = frames(longest_line_seconds());
        self.lines = [(); LINES].map(|_| DelayLine::new(longest));
        self.damped = [0.0; LINES];
        self.cuts = [[OnePole::default(); 2]; CHANNELS];
        self.position = 0;
        self.quiet_frames = 0;
        let state = self.state;
        self.aim(&state);
        // Also a frozen reverb needs lines of its size to begin with.
        self.line_taps = Taps::new(line_frames(state.size, sample_rate));
        self.snap();
    }

    /// Sets every target from a record.
    fn aim(&mut self, state: &ReverbState) {
        self.state = *state;
        let (ramp, rate) = (self.ramp_frames, self.sample_rate);
        let pre_delay = frames_of(state.pre_delay_ms / 1_000.0, rate);
        self.pre_delay_tap.aim([pre_delay], ramp);
        // A frozen tail keeps its lines: every fade between taps loses a little of it, and
        // nothing comes in to fill it again. The size arrives when freeze ends.
        if !state.freeze {
            self.line_taps.aim(line_frames(state.size, rate), ramp);
        }
        self.decay.set_target(state.decay_seconds.log2(), ramp);
        self.low_cut.set_target(state.low_cut_hz.log2(), ramp);
        self.high_cut.set_target(state.high_cut_hz.log2(), ramp);
        self.damping.set_target(state.damping, ramp);
        self.diffusion
            .set_target(state.diffusion * MOST_DIFFUSION, ramp);
        self.width.set_target(state.width, ramp);
        self.mix.set_target(state.mix, ramp);
        let frozen = if state.freeze { 1.0 } else { 0.0 };
        self.freeze.set_target(frozen, ramp);
        self.input.set_target(1.0 - frozen, ramp);
    }

    fn smoothers(&mut self) -> [&mut Smoothed; 9] {
        [
            &mut self.decay,
            &mut self.low_cut,
            &mut self.high_cut,
            &mut self.damping,
            &mut self.diffusion,
            &mut self.width,
            &mut self.mix,
            &mut self.freeze,
            &mut self.input,
        ]
    }

    /// Takes every target at once. For a reverb nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        self.smoothers().into_iter().for_each(Smoothed::snap);
        self.pre_delay_tap.snap();
        self.line_taps.snap();
        self.stale = true;
        self.snapped = true;
    }

    /// Moves the decay, the damping, freeze and the cuts `frames` along, and works out the
    /// factors for where they are when anything of them moves.
    fn move_factors(&mut self, frames: usize) {
        self.tail_gain = self.tail_gain_target;
        let lines_move = self.stale
            || self.decay.is_moving()
            || self.damping.is_moving()
            || self.freeze.is_moving()
            || self.line_taps.is_fading();
        let cuts_move = self.stale || self.low_cut.is_moving() || self.high_cut.is_moving();
        self.stale = false;
        let decay = self.decay.advance(frames).exp2();
        let damping = self.damping.advance(frames);
        let freeze = self.freeze.advance(frames);
        let low_cut = self.low_cut.advance(frames).exp2();
        let high_cut = self.high_cut.advance(frames).exp2();
        let rate = self.sample_rate;
        if cuts_move {
            self.cut_factors = [
                OnePole::factor(low_cut, rate),
                OnePole::factor(high_cut, rate),
            ];
        }
        if !lines_move {
            return;
        }
        let part = highs_part(damping);
        let damped_cos = (TAU * DAMPED_HZ.min(HIGHEST_PART * rate) / rate).cos();
        let heard = HEARD_HZ.map(|hz| (TAU * hz.min(HIGHEST_PART * rate) / rate).cos());
        let mut lost = [0.0; HEARD_HZ.len()];
        for index in 0..LINES {
            let length = self.line_taps.length(index);
            let (gain, pole) = line_loss(length, decay, part, damped_cos, rate);
            // What a pass loses of the power at each heard frequency.
            let top = gain * (1.0 - pole);
            for (lost, cos) in lost.iter_mut().zip(heard) {
                *lost += 1.0 - top * top / (1.0 - 2.0 * pole * cos + pole * pole);
            }
            // Freeze glides the loss to none: a gain of exactly 1 and no damping, written so
            // that a whole freeze gives exactly those.
            let gain = 1.0 - (1.0 - gain) * (1.0 - freeze);
            let pole = pole * (1.0 - freeze);
            self.line_gains[index] = gain * (1.0 - pole);
            self.poles[index] = pole;
        }
        // What comes in stays in the lines until the loop loses it, so at each frequency the
        // lines hold the power of the input over the part the loop loses per pass, on average
        // over the lines, and what comes out of them is the part a pass keeps of that. Over the
        // heard frequencies that is the power of the tail of a noise; scaled by its root, the
        // tail comes out at the level of the noise at every size, decay and damping. A frozen
        // reverb keeps the factor it had, so it holds its level.
        if freeze == 0.0 {
            let held: f32 = lost.iter().map(|lost| (LINES as f32 - lost) / lost).sum();
            let held = (held / HEARD_HZ.len() as f32).max(f32::MIN_POSITIVE);
            self.tail_gain_target = OUTPUT / held.sqrt();
        }
        if std::mem::take(&mut self.snapped) {
            self.tail_gain = self.tail_gain_target;
        }
    }

    /// The largest delay a sound can take through the reverb before it is in the lines.
    fn longest_path(&self) -> usize {
        let diffusers: usize = self.diffuser_frames.iter().flatten().sum();
        self.pre_delay[0].buffer.len() + diffusers + self.lines[0].buffer.len()
    }

    /// One frame of the reverb, from the input of each channel to the wet sound of each.
    fn frame(&mut self, input: [f32; CHANNELS]) -> [f32; CHANNELS] {
        let position = self.position;
        let (line_fade, pre_fade) = (self.line_taps.weight(), self.pre_delay_tap.weight());
        let diffusion = self.diffusion.advance(1);
        let input_gain = self.input.advance(1);
        let [low_factor, high_factor] = self.cut_factors;
        let mut into = [0.0; CHANNELS];
        for channel in 0..CHANNELS {
            let [low_cut, high_cut] = &mut self.cuts[channel];
            let cut = high_cut.low(high_factor, low_cut.high(low_factor, input[channel]));
            let pre_delay = &mut self.pre_delay[channel];
            pre_delay.write(position, cut);
            let mut sound = self.pre_delay_tap.read(pre_delay, 0, position, pre_fade);
            let diffusers = self.diffusers[channel].iter_mut();
            for (diffuser, frames) in diffusers.zip(self.diffuser_frames[channel]) {
                let delayed = diffuser.read(position, frames);
                let kept = sound + diffusion * delayed;
                diffuser.write(position, kept);
                sound = delayed - diffusion * kept;
            }
            into[channel] = sound * input_gain;
        }

        let mut out = [0.0_f32; LINES];
        for (index, out) in out.iter_mut().enumerate() {
            let read = self
                .line_taps
                .read(&self.lines[index], index, position, line_fade);
            let damped = self.line_gains[index] * read + self.poles[index] * self.damped[index];
            self.damped[index] = damped;
            *out = damped;
        }
        let mut wet = [0.0; CHANNELS];
        for (index, out) in out.iter().enumerate() {
            let [left, right] = signs(index);
            wet[0] += left * out;
            wet[1] += right * out;
        }
        let mixed = hadamard(out);
        let mut loudest = 0.0_f32;
        for (index, mixed) in mixed.iter().enumerate() {
            let [left, right] = signs(index);
            let written = mixed + 0.5 * (left * into[0] + right * into[1]);
            self.lines[index].write(position, written);
            loudest = loudest.max(written.abs());
        }
        self.position = position.wrapping_add(1);
        self.line_taps.advance();
        self.pre_delay_tap.advance();
        if loudest < REST {
            self.quiet_frames = self.quiet_frames.saturating_add(1);
        } else {
            self.quiet_frames = 0;
        }
        wet
    }
}

impl Processor for Reverb {
    type Update = ReverbState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        let sample_rate = config.sample_rate as f32;
        if sample_rate != self.sample_rate {
            self.allocate(sample_rate);
        }
    }

    fn update(&mut self, update: &mut ReverbState) {
        self.aim(update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = left_in.iter().chain(right_in).all(|sample| *sample == 0.0);
        if silent_input && self.quiet_frames > self.longest_path() {
            // Nothing sounds and nothing is left in the lines: no glide can be heard, and the
            // output is already silent.
            self.snap();
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let chunks = left_in
            .chunks(FACTOR_FRAMES)
            .zip(right_in.chunks(FACTOR_FRAMES))
            .zip(left_out.chunks_mut(FACTOR_FRAMES))
            .zip(right_out.chunks_mut(FACTOR_FRAMES));
        for (((left_in, right_in), left_out), right_out) in chunks {
            self.move_factors(left_in.len());
            let length = left_in.len() as f32;
            let (from, to) = (self.tail_gain, self.tail_gain_target);
            let frames = left_in
                .iter()
                .zip(right_in)
                .zip(left_out.iter_mut())
                .zip(right_out.iter_mut());
            for (index, (((left_in, right_in), left_out), right_out)) in frames.enumerate() {
                let [left, right] = self.frame([held(*left_in), held(*right_in)]);
                let tail_gain = from + (to - from) * (index + 1) as f32 / length;
                let width = self.width.advance(1);
                let mix = self.mix.advance(1);
                let middle = (left + right) * 0.5 * tail_gain;
                let side = (left - right) * 0.5 * width * tail_gain;
                // The dry sound as it came in: only what goes into the reverb is held.
                *left_out = (1.0 - mix) * *left_in + mix * (middle + side);
                *right_out = (1.0 - mix) * *right_in + mix * (middle - side);
            }
        }
        if !silent_input {
            self.quiet_frames = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loss of a line at the damped frequency is what the damping asks, and at 0 Hz what
    /// the decay asks.
    #[test]
    fn the_loss_of_a_line_is_the_decay_at_zero_and_the_damped_decay_at_five_kilohertz() {
        let rate = 48_000.0;
        let damped_cos = (TAU * DAMPED_HZ / rate).cos();
        for (length, decay, part) in [
            (2_000.0, 2.0, 0.55),
            (600.0, 0.5, 0.3),
            (4_000.0, 20.0, 0.1),
        ] {
            let (gain, pole) = line_loss(length, decay, part, damped_cos, rate);
            let dc_db = 20.0 * gain.log10();
            let expected = -60.0 * length / (decay * rate);
            assert!((dc_db - expected).abs() < 1e-3, "{dc_db} {expected}");
            let at = (1.0 - pole) / (1.0 - 2.0 * pole * damped_cos + pole * pole).sqrt();
            let high_db = 20.0 * (gain * at).log10();
            let expected = expected / part;
            assert!((high_db - expected).abs() < 1e-3, "{high_db} {expected}");
        }
    }

    /// Every line is a prime number of frames, and no two are the same, at every size and rate.
    #[test]
    fn the_lines_are_different_primes() {
        assert_eq!(nearest_prime(1_800), 1_801);
        assert_eq!(nearest_prime(4), 3);
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            for size in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let lines = line_frames(size, rate);
                for (index, line) in lines.iter().enumerate() {
                    assert!((2..*line).all(|divisor| line % divisor != 0), "{line}");
                    assert!(!lines[index + 1..].contains(line), "{lines:?}");
                }
            }
        }
    }

    #[test]
    fn a_frozen_line_has_a_gain_of_exactly_one_and_no_damping() {
        let mut reverb = Reverb::new(ReverbState {
            freeze: true,
            ..ReverbState::default()
        });
        reverb.move_factors(16);
        assert_eq!(reverb.line_gains, [1.0; LINES]);
        assert_eq!(reverb.poles, [0.0; LINES]);
    }
}
