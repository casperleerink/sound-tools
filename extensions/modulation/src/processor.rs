//! The modulation processor: one LFO per channel, a delay line for the chorus and the flanger,
//! six allpass filters for the phaser, and the mix.
//!
//! Every mode is one value that the LFO swings on a ratio around a centre: the delay of a copy
//! of the sound for the chorus and the flanger, the frequency of the allpass filters for the
//! phaser. Depth is how many octaves of its reach it swings each way, so one knob means the same
//! in every mode. Spread holds the LFO of the right side behind the left by up to half a cycle.
//!
//! The chorus and the flanger read one delay line between its frames with a cubic
//! ([`DelayLine::read_between`]), so a delay that moves bends the pitch smoothly. The phaser is
//! six first-order allpass filters in a row: each turns the phase and keeps the level, and with
//! the dry sound the turns make three notches. Feedback sends the wet sound round again: into
//! the delay line, or into the first allpass filter a frame later. Both loops keep the level
//! of what goes round (an allpass and a delay pass every frequency at 1, and the cubic
//! passes none at more), so a feedback under 1 always dies away, also while the LFO moves. Feedback raises the
//! peaks between the notches; the wet sound is scaled by `√(1 - g²)` so that a noise keeps its
//! level at every feedback, and a mix means the same at every setting.
//!
//! The LFO is worked out every 16 frames, and the delays and the allpass factors move in a
//! straight line between, so a sweep has no steps. A change of mode fades from the old mode to
//! the new one over 20 ms, both running meanwhile, as the filter glides between its types.
//!
//! The delay line is allocated in `prepare`, for the sample rate and the longest delay, and
//! never in `process`.

use std::f32::consts::PI;
use std::f64::consts::TAU;

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, DelayLine, Lfo, LfoShape, Ports,
    PrepareConfig, ProcessContext, Processor, Smoothed, Targets, held, is_zero,
};

use crate::{DEPTH, FEEDBACK, MIX, Mode, ModulationState, PARAMETERS, RATE, SPREAD};

/// Every number of the modulation can be automated.
type ModulationTargets = Targets<ModulationState, { PARAMETERS.len() }>;

/// How long a change takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// Rate, depth and spread move where the delay is read, and a read that moves fast bends the
/// pitch: over 20 ms a change of depth would chirp, and a rate that jumps would make the pitch
/// jump. So they glide for longer.
const SWEEP_RAMP_SECONDS: f32 = 0.1;

/// The LFO is worked out this often, and the delays and factors glide in between.
const FACTOR_FRAMES: usize = 16;

/// The feedback at 1. Over it a flanger at full feedback rings for seconds and its peaks are
/// loud; at it the peaks are 13 dB over the level of a noise.
const MAX_FEEDBACK: f32 = 0.9;

/// The allpass filters of the phaser. Six make three notches.
const STAGES: usize = 6;

/// The allpass filters stay under this part of the sample rate, below the Nyquist frequency.
const HIGHEST_PART: f32 = 0.45;

/// While the input is silent and nothing in the effect is louder than this, -180 dB, it has
/// rung out: it does no work and its output is silent.
const REST: f32 = 1e-9;

/// What the LFO swings in a mode: a centre, in ms of delay or in Hz of the allpass filters, and
/// how many octaves it reaches each way at depth 1.
struct Swing {
    centre: f32,
    octaves: f32,
}

impl Swing {
    const fn of(mode: Mode) -> Self {
        match mode {
            // 7 to 20 ms: a copy far enough behind to be heard as a second voice, and not as
            // an echo. Its pitch wobbles by about 30 cents at the default rate and full depth.
            Mode::Chorus => Self {
                centre: 12.0,
                octaves: 0.7,
            },
            // 0.27 to 8.5 ms: the first notch sweeps from about 60 Hz to 1.9 kHz.
            Mode::Flanger => Self {
                centre: 1.5,
                octaves: 2.5,
            },
            // 250 Hz to 4 kHz: the three notches sweep from 67 Hz to 15 kHz.
            Mode::Phaser => Self {
                centre: 1_000.0,
                octaves: 2.0,
            },
        }
    }

    /// Where the value is at `depth` and an LFO value from -1 to 1.
    fn at(&self, depth: f32, lfo: f32) -> f32 {
        self.centre * (self.octaves * depth * lfo).exp2()
    }
}

/// The lowest and the highest value the LFO reaches in the mode of `state`: the delay in ms for
/// the chorus and the flanger, the frequency of the allpass filters in Hz for the phaser.
pub fn sweep(state: &ModulationState) -> [f32; 2] {
    let swing = Swing::of(state.mode);
    [swing.at(state.depth, -1.0), swing.at(state.depth, 1.0)]
}

/// The longest delay in ms: the chorus at full depth.
fn longest_ms() -> f32 {
    Swing::of(Mode::Chorus).at(1.0, 1.0)
}

/// The factor of an allpass filter whose phase turns by 90° at `hz`.
fn allpass_factor(hz: f32, sample_rate: f32) -> f32 {
    let hz = hz.min(HIGHEST_PART * sample_rate);
    let t = (PI * hz / sample_rate).tan();
    (t - 1.0) / (t + 1.0)
}

fn weight(mode: Mode) -> [f32; 3] {
    match mode {
        Mode::Chorus => [1.0, 0.0, 0.0],
        Mode::Flanger => [0.0, 1.0, 0.0],
        Mode::Phaser => [0.0, 0.0, 1.0],
    }
}

/// The level of the wet sound at a feedback `g`: what keeps a noise at its level.
fn wet_level(g: f32) -> f32 {
    (1.0 - g * g).max(0.0).sqrt()
}

/// The gain at `hz` of the effect with this record, with the LFO in its middle, as a factor:
/// what a quiet steady sine comes out with, once every change has arrived. So it is the gain at
/// every moment at depth 0, and at depth over 0 the gain when the LFO passes its middle.
///
/// This is the exact response of the processor: the delay loop with the cubic read at its
/// fraction of a frame, or the loop of the six allpass filters, with the wet level and the mix.
pub fn response(state: &ModulationState, hz: f32, sample_rate: f32) -> f32 {
    let omega = TAU * f64::from(hz) / f64::from(sample_rate);
    let g = f64::from(MAX_FEEDBACK * state.feedback);
    let swing = Swing::of(state.mode);
    let wet = match state.mode {
        Mode::Chorus | Mode::Flanger => {
            let delay = f64::from(swing.centre * sample_rate / 1_000.0).max(2.0);
            let read = cubic_response(delay, omega);
            // What is written is `x + g y`, what is read is `read` of it.
            divide(read, (1.0 - g * read.0, -g * read.1))
        }
        Mode::Phaser => {
            let a = f64::from(allpass_factor(swing.centre, sample_rate));
            let late = cis(-omega);
            let one = divide((a + late.0, late.1), (1.0 + a * late.0, a * late.1));
            let chain = (0..STAGES).fold((1.0, 0.0), |chain, _| multiply(chain, one));
            // The first filter takes `x + g p` with the output `p` of the frame before.
            let round = multiply(late, chain);
            divide(chain, (1.0 - g * round.0, -g * round.1))
        }
    };
    let (mix, level) = (f64::from(state.mix), f64::from(wet_level(g as f32)));
    let real = 1.0 - mix + mix * level * wet.0;
    let imaginary = mix * level * wet.1;
    real.hypot(imaginary) as f32
}

/// The response of [`DelayLine::read_between`] at `delay` frames: its four weights on the frames
/// around the delay.
fn cubic_response(delay: f64, omega: f64) -> (f64, f64) {
    let whole = delay.floor();
    let t = delay - whole;
    let (t2, t3) = (t * t, t * t * t);
    // The weights of the frames at `whole - 1`, `whole`, `whole + 1` and `whole + 2`, from the
    // cubic of `read_between`.
    let weights = [
        -0.5 * t + t2 - 0.5 * t3,
        1.0 - 2.5 * t2 + 1.5 * t3,
        0.5 * t + 2.0 * t2 - 1.5 * t3,
        -0.5 * t2 + 0.5 * t3,
    ];
    weights
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(real, imaginary), (index, weight)| {
            let (cos, sin) = cis(-omega * (whole - 1.0 + index as f64));
            (real + weight * cos, imaginary + weight * sin)
        })
}

fn cis(angle: f64) -> (f64, f64) {
    (angle.cos(), angle.sin())
}

fn multiply(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

fn divide(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let size = b.0 * b.0 + b.1 * b.1;
    (
        (a.0 * b.0 + a.1 * b.1) / size,
        (a.1 * b.0 - a.0 * b.1) / size,
    )
}

/// What one channel keeps from frame to frame.
struct Channel {
    line: DelayLine,
    /// The memory of each allpass filter of the phaser.
    stages: [f32; STAGES],
    /// What the phaser gave in the frame before, for its feedback.
    phased: f32,
    /// The delays of the chorus and the flanger in frames, and the allpass factor of the
    /// phaser, at the end of the last run of frames and where they are going in the run now.
    from: [f32; 3],
    to: [f32; 3],
}

impl Channel {
    fn new(frames: usize) -> Self {
        Self {
            line: DelayLine::new(frames),
            stages: [0.0; STAGES],
            phased: 0.0,
            from: [0.0; 3],
            to: [0.0; 3],
        }
    }

    /// One frame of the six allpass filters, each `y = a x + s`, `s = x - a y`.
    fn phase(&mut self, factor: f32, input: f32) -> f32 {
        self.stages.iter_mut().fold(input, |sound, memory| {
            let out = factor * sound + *memory;
            *memory = sound - factor * out;
            out
        })
    }

    fn clear_phaser(&mut self) {
        self.stages = [0.0; STAGES];
        self.phased = 0.0;
    }

    /// The loudest of what the phaser holds.
    fn phaser_level(&self) -> f32 {
        self.stages
            .iter()
            .fold(self.phased.abs(), |loudest, memory| {
                loudest.max(memory.abs())
            })
    }
}

pub struct Modulation {
    /// The record, with the values of the lanes that automate it.
    state: Automated<ModulationState, { PARAMETERS.len() }>,
    sample_rate: f32,
    /// The frames a change takes, and a change of rate, depth or spread.
    ramp_frames: f32,
    sweep_ramp_frames: f32,
    /// Where the delay line writes the next frame.
    position: usize,
    channels: [Channel; CHANNELS],
    lfo: Lfo,
    rate_hz: Smoothed,
    depth: Smoothed,
    /// How far the right side is behind, in cycles: half the spread.
    lag: Smoothed,
    /// The feedback as the factor of the loop.
    feedback: Smoothed,
    mix: Smoothed,
    /// How much of the chorus, the flanger and the phaser is heard.
    modes: [Smoothed; 3],
    /// Whether the delays and factors take where they are going at once, with nothing to glide
    /// from: after a snap, and before the first block.
    stale: bool,
    /// Frames in a row with a silent input and nothing audible in the effect.
    quiet_frames: usize,
}

impl Modulation {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<ModulationState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    /// Its delay line is allocated in `prepare`, which also takes the record at once.
    pub fn new(state: ModulationState) -> Self {
        Self {
            state: Automated::new(Self::AUTOMATION, state),
            sample_rate: 0.0,
            ramp_frames: 1.0,
            sweep_ramp_frames: 1.0,
            position: 0,
            channels: [(); CHANNELS].map(|_| Channel::new(1)),
            lfo: Lfo::default(),
            rate_hz: Smoothed::new(state.rate_hz),
            depth: Smoothed::new(0.0),
            lag: Smoothed::new(0.0),
            feedback: Smoothed::new(0.0),
            mix: Smoothed::new(0.0),
            modes: [0.0; 3].map(Smoothed::new),
            stale: true,
            quiet_frames: 0,
        }
    }

    /// Makes the delay line for a sample rate, empty, and takes the record at once, so a
    /// modulation that is added or opened does not glide in.
    fn allocate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        self.sweep_ramp_frames = (SWEEP_RAMP_SECONDS * sample_rate).max(1.0);
        // Two frames past the delay for the cubic, and one to spare.
        let frames = (longest_ms() * sample_rate / 1_000.0).ceil() as usize + 3;
        self.channels = [(); CHANNELS].map(|_| Channel::new(frames));
        self.position = 0;
        self.quiet_frames = 0;
        self.aim(&self.state.targets(self.sweep_ramp_frames));
        self.snap();
    }

    /// Sets every target from the record and its lanes, each reached in its own ramp. The edit
    /// glide of the lanes is the long one of a sweep, see [`SWEEP_RAMP_SECONDS`]; the feedback,
    /// the mix and the mode take at most the 20 ms of a change.
    fn aim(&mut self, targets: &ModulationTargets) {
        let state = *self.state;
        self.rate_hz.set_target(state.rate_hz, targets.ramp(&RATE));
        self.depth.set_target(state.depth, targets.ramp(&DEPTH));
        self.lag
            .set_target(0.5 * state.spread, targets.ramp(&SPREAD));
        let ramp = |parameter| targets.ramp(parameter).min(self.ramp_frames);
        self.feedback
            .set_target(MAX_FEEDBACK * state.feedback, ramp(&FEEDBACK));
        self.mix.set_target(state.mix, ramp(&MIX));
        for (mode, target) in self.modes.iter_mut().zip(weight(state.mode)) {
            mode.set_target(target, self.ramp_frames);
        }
        // A sweep that took its value at once has nothing to glide from.
        self.stale |= targets.snaps();
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        let [chorus, flanger, phaser] = &mut self.modes;
        [
            &mut self.rate_hz,
            &mut self.depth,
            &mut self.lag,
            &mut self.feedback,
            &mut self.mix,
            chorus,
            flanger,
            phaser,
        ]
        .into_iter()
    }

    /// Takes every target at once. For an effect nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        self.smoothers().for_each(Smoothed::snap);
        self.stale = true;
    }

    /// Moves the LFO, depth and spread `frames` along, and works out where the delays and the
    /// allpass factor of each channel go in the run of frames now.
    fn move_sweeps(&mut self, frames: usize) {
        let depth = self.depth.advance(frames);
        let lag = self.lag.advance(frames);
        let rate_hz = self.rate_hz.advance(frames);
        self.lfo.advance(frames, rate_hz, self.sample_rate);
        let frames_per_ms = self.sample_rate / 1_000.0;
        let [chorus, flanger, phaser] = Mode::ALL.map(Swing::of);
        for (channel, lag) in self.channels.iter_mut().zip([0.0, lag]) {
            let lfo = self.lfo.value(LfoShape::Sine, lag);
            channel.from = channel.to;
            channel.to = [
                chorus.at(depth, lfo) * frames_per_ms,
                flanger.at(depth, lfo) * frames_per_ms,
                allpass_factor(phaser.at(depth, lfo), self.sample_rate),
            ];
        }
        if std::mem::take(&mut self.stale) {
            for channel in &mut self.channels {
                channel.from = channel.to;
            }
        }
    }
}

impl Processor for Modulation {
    type Update = ModulationState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.allocate(config.sample_rate as f32);
    }

    fn update(&mut self, update: &mut ModulationState) {
        let targets = self.state.set_record(update, self.sweep_ramp_frames);
        self.aim(&targets);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.sweep_ramp_frames) {
            self.aim(&targets);
        }
        let frames = context.frames;
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = is_zero(left_in) && is_zero(right_in);
        if silent_input && self.quiet_frames > self.channels[0].line.frames() {
            // Nothing sounds and nothing is left in the effect: no glide can be heard, and the
            // output is already silent. The LFO goes on, so where it is does not depend on the
            // silence.
            self.snap();
            self.lfo
                .advance(frames, self.rate_hz.current(), self.sample_rate);
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let chunks = left_in
            .chunks(FACTOR_FRAMES)
            .zip(right_in.chunks(FACTOR_FRAMES))
            .zip(left_out.chunks_mut(FACTOR_FRAMES))
            .zip(right_out.chunks_mut(FACTOR_FRAMES));
        for (((left_in, right_in), left_out), right_out) in chunks {
            let length = left_in.len();
            self.move_sweeps(length);
            // A mode that is not heard and is not fading in or out does no work. The phaser
            // lets go of what it held, so it starts clean when it fades in again.
            let running = self
                .modes
                .each_ref()
                .map(|mode| mode.is_moving() || mode.current() != 0.0);
            if !running[2] {
                self.channels.iter_mut().for_each(Channel::clear_phaser);
            }
            let frames = left_in
                .iter()
                .zip(right_in)
                .zip(left_out.iter_mut())
                .zip(right_out.iter_mut());
            for (index, (((left_in, right_in), left_out), right_out)) in frames.enumerate() {
                let along = (index + 1) as f32 / length as f32;
                let [chorus, flanger, phaser] = self.modes.each_mut().map(|mode| mode.advance(1));
                let feedback = self.feedback.advance(1);
                let mix = self.mix.advance(1);
                let level = wet_level(feedback);
                let position = self.position;
                let mut loudest = 0.0_f32;
                let [left, right] = &mut self.channels;
                for (channel, input, output) in
                    [(left, left_in, left_out), (right, right_in, right_out)]
                {
                    let dry = held(*input);
                    let [delay_chorus, delay_flanger, factor] = std::array::from_fn(|mode| {
                        let (from, to) = (channel.from[mode], channel.to[mode]);
                        from + (to - from) * along
                    });
                    let read = |running, delay| match running {
                        true => channel.line.read_between(position, delay),
                        false => 0.0,
                    };
                    let delayed = chorus * read(running[0], delay_chorus)
                        + flanger * read(running[1], delay_flanger);
                    let written = dry + feedback * delayed;
                    channel.line.write(position, written);
                    let mut wet = delayed;
                    if running[2] {
                        let phased = channel.phase(factor, dry + feedback * channel.phased);
                        channel.phased = phased;
                        wet += phaser * phased;
                        loudest = loudest.max(channel.phaser_level());
                    }
                    loudest = loudest.max(written.abs());
                    // The dry sound as it came in: only what goes into the effect is held.
                    *output = (1.0 - mix) * *input + mix * level * wet;
                }
                self.position = position.wrapping_add(1);
                if loudest < REST {
                    self.quiet_frames = self.quiet_frames.saturating_add(1);
                } else {
                    self.quiet_frames = 0;
                }
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

    /// `cubic_response` is what `DelayLine::read_between` does to a sine: a line holding a
    /// cosine and one holding a sine, read at a fraction of a frame, give the real and the
    /// imaginary part of the response times where the sine is.
    #[test]
    fn the_response_of_the_cubic_is_what_the_delay_line_reads() {
        let omega = 0.3_f64;
        let (mut cosine, mut sine) = (DelayLine::new(64), DelayLine::new(64));
        let now = 40;
        for position in 0..now {
            let angle = omega * position as f64;
            cosine.write(position, angle.cos() as f32);
            sine.write(position, angle.sin() as f32);
        }
        for delay in [2.0, 7.25, 12.5, 20.9] {
            let expected = multiply(cis(omega * now as f64), cubic_response(delay, omega));
            let read = (
                f64::from(cosine.read_between(now, delay as f32)),
                f64::from(sine.read_between(now, delay as f32)),
            );
            assert!((read.0 - expected.0).abs() < 1e-5, "{delay}: {read:?}");
            assert!((read.1 - expected.1).abs() < 1e-5, "{delay}: {read:?}");
        }
    }

    #[test]
    fn every_mode_sweeps_around_its_centre_and_holds_it_at_depth_zero() {
        for mode in Mode::ALL {
            let centre = Swing::of(mode).centre;
            let still = ModulationState {
                mode,
                depth: 0.0,
                ..ModulationState::default()
            };
            assert_eq!(sweep(&still), [centre, centre]);
            let [low, high] = sweep(&ModulationState {
                mode,
                depth: 1.0,
                ..ModulationState::default()
            });
            assert!((low * high - centre * centre).abs() < 1e-3 * centre * centre);
            assert!(low < centre && centre < high);
        }
        let [_, longest] = sweep(&ModulationState {
            depth: 1.0,
            ..ModulationState::default()
        });
        assert_eq!(longest, longest_ms());
    }
}
