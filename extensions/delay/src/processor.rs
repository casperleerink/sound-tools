//! The delay processor: a delay line per channel, read at the time of the record, and what it
//! reads goes back in through a low cut and a high cut, times the feedback.
//!
//! Why it stays stable: the feedback is at most 0.95 and the cuts are one-pole filters, which
//! are never louder than what goes in, so each pass through the loop is quieter than the last
//! at every frequency. Ping-pong mixes the two lines into each other, crossed; that mix is a
//! weighted average of the two, so it adds no level either.
//!
//! In ping-pong the sound comes in as one, left and right together, into the left line only,
//! and each line feeds the other: the first repeat is on the left, the second on the right, and
//! so on. Its switch glides like every other change, from the lines side by side to crossed.
//!
//! A synced time follows the tempo of the transport where the block starts, playing or not, so
//! a tempo change moves the repeats with it. A new time, from the record or from the tempo, does
//! not move a read position: it fades over 20 ms from the old tap to the new one, so nothing
//! clicks and no pitch slides. The lines are allocated in `prepare`, for the sample rate and
//! [`LONGEST_SECONDS`], and never in `process`.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, DelayLine, OnePole, Ports,
    PrepareConfig, ProcessContext, Processor, Smoothed, Taps, Targets, held,
};

use crate::{
    DelayState, FEEDBACK, HIGH_CUT, LONGEST_SECONDS, LOW_CUT, MIX, PARAMETERS, delay_seconds,
};

/// Every number of the delay can be automated.
type DelayTargets = Targets<DelayState, { PARAMETERS.len() }>;

/// How long a change takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// While the cuts move, their factors are worked out again this often.
const FACTOR_FRAMES: usize = 16;

/// While the input is silent and nothing in the lines is louder than this, -180 dB, the delay
/// has rung out: it does no work and its output is silent.
const REST: f32 = 1e-9;

/// The level of the first repeat of a sine at `hz`, against the sine: one pass through the low
/// cut and the high cut. Each later repeat is this times the feedback times the one before. The
/// response of record: the tests hold the measured repeats to it.
///
/// A one-pole filter in its trapezoidal form is the analog one with the frequency bent by
/// `tan(π f / sample rate)`, the same bend as its factor, so each is the analog gain at the
/// ratio of two such bent frequencies.
pub fn response(state: &DelayState, hz: f32, sample_rate: f32) -> f32 {
    let bent = |factor: f32| factor / (1.0 - factor);
    let frequency = (std::f32::consts::PI * hz / sample_rate).tan();
    let [low, high] = [state.low_cut_hz, state.high_cut_hz]
        .map(|cutoff| frequency / bent(OnePole::factor(cutoff, sample_rate)));
    let low_cut = low / (1.0 + low * low).sqrt();
    let high_cut = 1.0 / (1.0 + high * high).sqrt();
    low_cut * high_cut
}

fn frames_of(seconds: f32, sample_rate: f32) -> usize {
    ((seconds * sample_rate).round() as usize).max(1)
}

pub struct Delay {
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    /// The record, with the values of the lanes that automate it, so that each block can work
    /// out its time at the tempo there.
    state: Automated<DelayState, { PARAMETERS.len() }>,
    /// The longest time, in frames. Every read is at most this far back.
    longest: usize,
    /// Where both lines write the next frame.
    position: usize,
    lines: [DelayLine; CHANNELS],
    tap: Taps<1>,
    /// The low cut and the high cut of each line, and their factors.
    cuts: [[OnePole; 2]; CHANNELS],
    cut_factors: [f32; 2],
    /// As `log2` of hertz, so a glide moves on a ratio.
    low_cut: Smoothed,
    high_cut: Smoothed,
    feedback: Smoothed,
    /// 0 with the lines side by side, 1 crossed.
    ping_pong: Smoothed,
    mix: Smoothed,
    /// Whether the cut factors have to be worked out again although nothing glides.
    stale: bool,
    /// Frames in a row with a silent input and nothing audible in the lines.
    quiet_frames: usize,
}

impl Delay {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<DelayState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    /// Its lines are allocated in `prepare`, which also takes the record at once.
    pub fn new(state: DelayState) -> Self {
        Self {
            sample_rate: 0.0,
            ramp_frames: 1.0,
            state: Automated::new(Self::AUTOMATION, state),
            longest: 1,
            position: 0,
            lines: [(); CHANNELS].map(|_| DelayLine::new(1)),
            tap: Taps::new([1]),
            cuts: [[OnePole::default(); 2]; CHANNELS],
            cut_factors: [0.0; 2],
            low_cut: Smoothed::new(0.0),
            high_cut: Smoothed::new(0.0),
            feedback: Smoothed::new(0.0),
            ping_pong: Smoothed::new(0.0),
            mix: Smoothed::new(0.0),
            stale: true,
            quiet_frames: 0,
        }
    }

    /// Makes both lines for a sample rate, empty, and takes the record at once: at 120 bpm
    /// until the first block says the tempo, so a delay that is added or opened does not glide
    /// in.
    fn allocate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        self.longest = frames_of(LONGEST_SECONDS, sample_rate);
        self.lines = [(); CHANNELS].map(|_| DelayLine::new(self.longest));
        self.cuts = [[OnePole::default(); 2]; CHANNELS];
        self.position = 0;
        // Empty lines have rung out.
        self.quiet_frames = usize::MAX;
        self.aim(&self.state.targets(self.ramp_frames));
        self.aim_time(120.0);
        self.snap();
    }

    /// Sets every target from the record and its lanes, each reached in its own ramp, apart
    /// from the time, which needs the tempo.
    fn aim(&mut self, targets: &DelayTargets) {
        let state = *self.state;
        self.low_cut
            .set_target(state.low_cut_hz.log2(), targets.ramp(&LOW_CUT));
        self.high_cut
            .set_target(state.high_cut_hz.log2(), targets.ramp(&HIGH_CUT));
        self.feedback
            .set_target(state.feedback, targets.ramp(&FEEDBACK));
        let crossed = if state.ping_pong { 1.0 } else { 0.0 };
        self.ping_pong.set_target(crossed, targets.edit());
        self.mix.set_target(state.mix, targets.ramp(&MIX));
        // A cut that took its value at once does not move, so nothing else says the factors
        // are old.
        self.stale |= targets.snaps();
    }

    /// Aims the read at the time of the record at a tempo. The same time again changes nothing.
    /// A lane of the time moves the read in a row of 20 ms fades between taps, as a drag of the
    /// knob does, not in a sweep with a pitch slide.
    fn aim_time(&mut self, bpm: f64) {
        let seconds = delay_seconds(&self.state, bpm);
        let frames = frames_of(seconds, self.sample_rate).min(self.longest);
        self.tap.aim([frames], self.ramp_frames);
    }

    fn smoothers(&mut self) -> [&mut Smoothed; 5] {
        [
            &mut self.low_cut,
            &mut self.high_cut,
            &mut self.feedback,
            &mut self.ping_pong,
            &mut self.mix,
        ]
    }

    /// Takes every target at once. For a delay nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        self.smoothers().into_iter().for_each(Smoothed::snap);
        self.tap.snap();
        self.stale = true;
    }

    /// Moves the cuts `frames` along, and works out their factors when they move.
    fn move_cuts(&mut self, frames: usize) {
        let moving = self.stale || self.low_cut.is_moving() || self.high_cut.is_moving();
        self.stale = false;
        let low_cut = self.low_cut.advance(frames).exp2();
        let high_cut = self.high_cut.advance(frames).exp2();
        if moving {
            let rate = self.sample_rate;
            self.cut_factors = [
                OnePole::factor(low_cut, rate),
                OnePole::factor(high_cut, rate),
            ];
        }
    }

    /// One frame of the delay, from the input of each channel to the repeats of each.
    fn frame(&mut self, input: [f32; CHANNELS]) -> [f32; CHANNELS] {
        let position = self.position;
        let fade = self.tap.weight();
        let read: [f32; CHANNELS] =
            std::array::from_fn(|channel| self.tap.read(&self.lines[channel], 0, position, fade));
        let feedback = self.feedback.advance(1);
        let crossed = self.ping_pong.advance(1);
        let side_by_side = 1.0 - crossed;
        let both = (input[0] + input[1]) * 0.5;
        // Side by side each line takes its own channel and its own repeats. Crossed, the left
        // line takes both channels as one and the repeats of the right line, and the right
        // line only the repeats of the left.
        let into = [
            side_by_side * input[0] + crossed * both,
            side_by_side * input[1],
        ];
        let back = [
            side_by_side * read[0] + crossed * read[1],
            side_by_side * read[1] + crossed * read[0],
        ];
        let [low_factor, high_factor] = self.cut_factors;
        let mut loudest = 0.0_f32;
        for channel in 0..CHANNELS {
            let [low_cut, high_cut] = &mut self.cuts[channel];
            let sound = into[channel] + feedback * back[channel];
            let written = high_cut.low(high_factor, low_cut.high(low_factor, sound));
            self.lines[channel].write(position, written);
            loudest = loudest.max(written.abs());
        }
        self.position = position.wrapping_add(1);
        self.tap.advance();
        if loudest < REST {
            self.quiet_frames = self.quiet_frames.saturating_add(1);
        } else {
            self.quiet_frames = 0;
        }
        read
    }
}

impl Processor for Delay {
    type Update = DelayState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.allocate(config.sample_rate as f32);
    }

    fn update(&mut self, update: &mut DelayState) {
        let targets = self.state.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let followed = self.state.follow(context, self.ramp_frames);
        if let Some(targets) = &followed {
            self.aim(targets);
        }
        let transport = &context.transport;
        let bpm = transport.clock.tempo_at(transport.tick_range.start).bpm();
        self.aim_time(bpm);
        // A time that took the value of its lane at once reads there from the first frame.
        if followed.is_some_and(|targets| targets.snaps()) {
            self.tap.snap();
        }
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = left_in.iter().chain(right_in).all(|sample| *sample == 0.0);
        if self.quiet_frames > self.lines[0].frames() {
            // Nothing is left in the lines, so a new time cannot be heard: it starts at once.
            self.tap.snap();
            if silent_input {
                // Nothing sounds either: no glide can be heard, and the output is already
                // silent.
                self.snap();
                return;
            }
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let chunks = left_in
            .chunks(FACTOR_FRAMES)
            .zip(right_in.chunks(FACTOR_FRAMES))
            .zip(left_out.chunks_mut(FACTOR_FRAMES))
            .zip(right_out.chunks_mut(FACTOR_FRAMES));
        for (((left_in, right_in), left_out), right_out) in chunks {
            self.move_cuts(left_in.len());
            let frames = left_in
                .iter()
                .zip(right_in)
                .zip(left_out.iter_mut())
                .zip(right_out.iter_mut());
            for (((left_in, right_in), left_out), right_out) in frames {
                let [left, right] = self.frame([held(*left_in), held(*right_in)]);
                let mix = self.mix.advance(1);
                // The dry sound as it came in: only what goes into the lines is held.
                *left_out = (1.0 - mix) * *left_in + mix * left;
                *right_out = (1.0 - mix) * *right_in + mix * right;
            }
        }
        if !silent_input {
            self.quiet_frames = 0;
        }
    }
}
