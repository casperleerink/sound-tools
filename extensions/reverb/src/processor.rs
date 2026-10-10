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
//! Every delay line is allocated in `prepare`, for the sample rate and the largest size and
//! pre-delay, and never in `process`. A change of size or
//! pre-delay does not move a read position: it fades over 20 ms from the old tap to the new one,
//! so nothing clicks and no pitch slides.

use std::f32::consts::{LOG2_10, TAU};

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, OnePole, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Taps, Targets, held,
};

use crate::{
    DAMPING, DECAY, DIFFUSION, HIGH_CUT, LOW_CUT, MIX, PARAMETERS, PRE_DELAY, ReverbState, WIDTH,
};

/// Every number of the reverb can be automated.
type ReverbTargets = Targets<ReverbState, { PARAMETERS.len() }>;

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

/// The frequencies the loss is worked out at stay under this part of the sample rate, below
/// the Nyquist frequency, as the cuts do.
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

fn frames_of(seconds: f32, sample_rate: f32) -> usize {
    ((seconds * sample_rate).round() as usize).max(1)
}

/// Delay lines that are written together, stored frame by frame: one write stores what all of
/// them take in a frame, and each is read at its own delay. In buffers of their own, a power of
/// two long, the sixteen lines of the network wrote to addresses that share one set of the CPU
/// cache, which has room for eight, so nearly every write missed the cache. The length is a
/// power of two, as in a [`DelayLine`](sound_core::DelayLine), so a position wraps with a mask.
struct Frames<T> {
    frames: Vec<T>,
    mask: usize,
}

impl<T: Copy + Default> Frames<T> {
    /// Holds at least `frames` frames, as a [`DelayLine`](sound_core::DelayLine) does.
    /// Allocates.
    fn new(frames: usize) -> Self {
        let length = (frames + 1).next_power_of_two();
        Self {
            frames: vec![T::default(); length],
            mask: length - 1,
        }
    }

    fn frames(&self) -> usize {
        self.frames.len()
    }

    /// The frame written `delay` frames before `position`.
    #[inline]
    fn at(&self, position: usize, delay: usize) -> &T {
        // The length is the mask plus one. Cut to it, the compiler sees that a masked index is
        // inside and checks no bound per read.
        let frames = &self.frames[..=self.mask];
        &frames[position.wrapping_sub(delay) & self.mask]
    }

    #[inline]
    fn write(&mut self, position: usize, frame: T) {
        let frames = &mut self.frames[..=self.mask];
        frames[position & self.mask] = frame;
    }
}

pub struct Reverb {
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    /// The record, with the values of the lanes that automate it.
    state: Automated<ReverbState, { PARAMETERS.len() }>,
    /// Where every delay line writes the next frame.
    position: usize,
    pre_delay: Frames<[f32; CHANNELS]>,
    pre_delay_tap: Taps<1>,
    diffusers: Frames<[[f32; DIFFUSERS]; CHANNELS]>,
    diffuser_frames: [[usize; DIFFUSERS]; CHANNELS],
    lines: Frames<[f32; LINES]>,
    line_taps: Taps<LINES>,
    /// The memory of the damping filter of each line.
    damped: [f32; LINES],
    /// The factor on what each line reads, `k (1 - p)`, and the pole of its damping filter.
    line_gains: [f32; LINES],
    poles: [f32; LINES],
    /// The factor on what goes into the lines, at the end of the last run of frames and where it
    /// is going in the run now, so it moves frame by frame and makes no step.
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
    pub const AUTOMATION: AutomationInput<ReverbState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    /// Its delay lines are allocated in `prepare`, which also takes the record at once.
    pub fn new(state: ReverbState) -> Self {
        Self {
            sample_rate: 0.0,
            ramp_frames: 1.0,
            state: Automated::new(Self::AUTOMATION, state),
            position: 0,
            pre_delay: Frames::new(1),
            pre_delay_tap: Taps::new([1]),
            diffusers: Frames::new(1),
            diffuser_frames: [[1; DIFFUSERS]; CHANNELS],
            lines: Frames::new(1),
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
        }
    }

    /// Makes every delay line for a sample rate, empty, and takes the record at once, so a
    /// reverb that is added or opened does not glide in.
    fn allocate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        let frames = |seconds| frames_of(seconds, sample_rate);
        self.pre_delay = Frames::new(frames(PRE_DELAY.max / 1_000.0));
        self.diffuser_frames = DIFFUSER_SECONDS.map(|channel| channel.map(frames));
        let longest_diffuser = self.diffuser_frames.into_iter().flatten().max();
        self.diffusers = Frames::new(longest_diffuser.unwrap_or(1));
        self.lines = Frames::new(frames(longest_line_seconds()));
        self.damped = [0.0; LINES];
        self.cuts = [[OnePole::default(); 2]; CHANNELS];
        self.position = 0;
        self.quiet_frames = 0;
        self.aim(&self.state.targets(self.ramp_frames));
        // Also a frozen reverb needs lines of its size to begin with.
        self.line_taps = Taps::new(line_frames(self.state.size, sample_rate));
        self.snap();
    }

    /// Sets every target from the record and its lanes, each reached in its own ramp. A new
    /// pre-delay or size is a fade between taps, which takes the glide of an edit: so a lane of
    /// them moves in a row of 20 ms fades, as a drag of the knob does.
    fn aim(&mut self, targets: &ReverbTargets) {
        let (state, ramp, rate) = (*self.state, targets.edit(), self.sample_rate);
        let pre_delay = frames_of(state.pre_delay_ms / 1_000.0, rate);
        self.pre_delay_tap.aim([pre_delay], ramp);
        // A frozen tail keeps its lines: every fade between taps loses a little of it, and
        // nothing comes in to fill it again. The size arrives when freeze ends.
        if !state.freeze {
            self.line_taps.aim(line_frames(state.size, rate), ramp);
        }
        self.decay
            .set_target(state.decay_seconds.log2(), targets.ramp(&DECAY));
        self.low_cut
            .set_target(state.low_cut_hz.log2(), targets.ramp(&LOW_CUT));
        self.high_cut
            .set_target(state.high_cut_hz.log2(), targets.ramp(&HIGH_CUT));
        self.damping
            .set_target(state.damping, targets.ramp(&DAMPING));
        self.diffusion
            .set_target(state.diffusion * MOST_DIFFUSION, targets.ramp(&DIFFUSION));
        self.width.set_target(state.width, targets.ramp(&WIDTH));
        self.mix.set_target(state.mix, targets.ramp(&MIX));
        let frozen = if state.freeze { 1.0 } else { 0.0 };
        self.freeze.set_target(frozen, ramp);
        self.input.set_target(1.0 - frozen, ramp);
        // A number that took its value at once does not move, so nothing else says the
        // factors are old.
        // And a pre-delay or a size that took the value of its lane at once is there from the
        // first frame.
        if targets.snaps() {
            self.stale = true;
            self.snapped = true;
            self.pre_delay_tap.snap();
            self.line_taps.snap();
        }
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
        // over the lines, and what comes out is what they hold. Over the heard frequencies that
        // is the power of the tail of a noise. What goes in is scaled by its root, so the tail
        // comes out at the level of the noise at every size, decay and damping. The factor is
        // on the way in and not on the way out, so a change of decay changes only how fast the
        // lines fill and drain, and never what the sound already in them comes out at. It is
        // worked out from the loss before freeze, which lets nothing in anyway.
        let held: f32 = lost.iter().map(|lost| LINES as f32 / lost).sum();
        let held = (held / HEARD_HZ.len() as f32).max(f32::MIN_POSITIVE);
        self.tail_gain_target = OUTPUT / held.sqrt();
        if std::mem::take(&mut self.snapped) {
            self.tail_gain = self.tail_gain_target;
        }
    }

    /// The largest delay a sound can take through the reverb before it is in the lines.
    fn longest_path(&self) -> usize {
        let diffusers: usize = self.diffuser_frames.iter().flatten().sum();
        self.pre_delay.frames() + diffusers + self.lines.frames()
    }

    /// One frame of the reverb, from the input of each channel to the wet sound of each.
    fn frame(&mut self, input: [f32; CHANNELS], tail_gain: f32) -> [f32; CHANNELS] {
        let position = self.position;
        let (line_fade, pre_fade) = (self.line_taps.weight(), self.pre_delay_tap.weight());
        let diffusion = self.diffusion.advance(1);
        let input_gain = self.input.advance(1);
        let [low_factor, high_factor] = self.cut_factors;
        let cut = std::array::from_fn(|channel| {
            let [low_cut, high_cut] = &mut self.cuts[channel];
            high_cut.low(high_factor, low_cut.high(low_factor, input[channel]))
        });
        // Every delay is a frame or more, so no read below sees what this frame writes.
        self.pre_delay.write(position, cut);
        let mut kept = [[0.0; DIFFUSERS]; CHANNELS];
        let mut into = [0.0; CHANNELS];
        for channel in 0..CHANNELS {
            let pre_delay = |delay| self.pre_delay.at(position, delay)[channel];
            let mut sound = self.pre_delay_tap.read_with(0, pre_fade, pre_delay);
            for (diffuser, frames) in self.diffuser_frames[channel].into_iter().enumerate() {
                let delayed = self.diffusers.at(position, frames)[channel][diffuser];
                let kept = &mut kept[channel][diffuser];
                *kept = sound + diffusion * delayed;
                sound = delayed - diffusion * *kept;
            }
            into[channel] = sound * input_gain * tail_gain;
        }
        self.diffusers.write(position, kept);

        // What comes out is what the lines hold, so the tail is what a freeze keeps: a frozen
        // loop holds the energy of the lines, and the tail was that energy all along. The loss
        // of a pass is on the way back in, after the mix, on what goes into each line.
        let out: [f32; LINES] = std::array::from_fn(|index| {
            let line = |delay| self.lines.at(position, delay)[index];
            self.line_taps.read_with(index, line_fade, line)
        });
        let mut wet = [0.0; CHANNELS];
        for (index, out) in out.iter().enumerate() {
            let [left, right] = signs(index);
            wet[0] += left * out;
            wet[1] += right * out;
        }
        let mixed = hadamard(out);
        // The signs of a line repeat every four lines, so four sums are all the input there is.
        let inputs: [f32; 4] = std::array::from_fn(|index| {
            let [left, right] = signs(index);
            0.5 * (left * into[0] + right * into[1])
        });
        self.damped = std::array::from_fn(|index| {
            self.line_gains[index] * mixed[index] + self.poles[index] * self.damped[index]
        });
        let written: [f32; LINES] =
            std::array::from_fn(|index| self.damped[index] + inputs[index % 4]);
        self.lines.write(position, written);
        // Or-ed, with no branch or maximum per line, so the processor compares them all at once.
        let audible = written
            .iter()
            .fold(false, |audible, written| audible | (written.abs() >= REST));
        self.position = position.wrapping_add(1);
        self.line_taps.advance();
        self.pre_delay_tap.advance();
        if audible {
            self.quiet_frames = 0;
        } else {
            self.quiet_frames = self.quiet_frames.saturating_add(1);
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
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.allocate(config.sample_rate as f32);
    }

    fn update(&mut self, update: &mut ReverbState) {
        let targets = self.state.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
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
                let tail_gain = from + (to - from) * (index + 1) as f32 / length;
                let [left, right] = self.frame([held(*left_in), held(*right_in)], tail_gain);
                let width = self.width.advance(1);
                let mix = self.mix.advance(1);
                let middle = (left + right) * 0.5;
                let side = (left - right) * 0.5 * width;
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
        reverb.prepare(&PrepareConfig {
            sample_rate: 48_000,
            offline: true,
        });
        reverb.move_factors(16);
        assert_eq!(reverb.line_gains, [1.0; LINES]);
        assert_eq!(reverb.poles, [0.0; LINES]);
    }
}
