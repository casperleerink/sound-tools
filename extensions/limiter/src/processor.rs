//! The limiter processor: the gain on the way in, then [`PeakLimiter`] of the core, the limiter
//! of the master. The two channels share one gain, so the stereo image does not move.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, PeakLimiter, Peaks, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Targets, amplitude, held,
};

use crate::{GAIN, LimiterState, Lookahead, RELEASE};

/// The numbers a lane can move. The ceiling is left out: it takes a new value at once, also
/// for the frames already in the lookahead, so a lane would step it every block.
const AUTOMATED: [&crate::Parameter; 2] = [&GAIN, &RELEASE];

type LimiterTargets = Targets<LimiterState, { AUTOMATED.len() }>;

/// How long a change of the gain takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// What the limiter shows on its card, from the audio thread: the peaks of what it sends out,
/// and the largest reduction of each block on channel 0, as the factor the sound was above what
/// came out. The master keeps the same two, so both show one display.
#[derive(Clone, Debug, Default)]
pub struct Meters {
    pub output: Peaks,
    pub reduction: Peaks,
}

impl Meters {
    /// The names the behaviour keeps them under, for [`sound_core::Project::peaks`].
    pub const OUTPUT: &str = "output";
    pub const REDUCTION: &str = "reduction";
}

pub struct Limiter {
    meters: Meters,
    /// The record, with the values of the lanes that automate it.
    state: Automated<LimiterState, { AUTOMATED.len() }>,
    ramp_frames: f32,
    gain: Smoothed,
    ceiling: f32,
    limiter: PeakLimiter,
    /// Frames in a row of silent input, up to what a silence needs to leave the delay.
    quiet: usize,
}

impl Limiter {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<LimiterState, { AUTOMATED.len() }> =
        AutomationInput::new(0, AUTOMATED);

    /// Starts at these values, so a limiter that is added or opened does not glide in.
    pub fn new(state: LimiterState, meters: Meters) -> Self {
        Self {
            meters,
            state: Automated::new(Self::AUTOMATION, state),
            ramp_frames: 1.0,
            gain: Smoothed::new(amplitude(state.gain_db)),
            ceiling: amplitude(state.ceiling_db),
            limiter: PeakLimiter::new(),
            quiet: 0,
        }
    }

    /// Sets every target from the record and its lanes.
    fn aim(&mut self, targets: &LimiterTargets) {
        let state = *self.state;
        self.gain
            .set_target(amplitude(state.gain_db), targets.ramp(&GAIN));
        self.ceiling = amplitude(state.ceiling_db);
        self.limiter.set_release(state.release_ms / 1_000.0);
        self.limiter.set_lookahead(state.lookahead.seconds());
    }

    /// Whether a silent block changes nothing: the silence has filled the delay and the gain is
    /// back at 1.
    fn is_resting(&self) -> bool {
        self.limiter.is_resting() && self.quiet >= self.limiter.latency()
    }
}

impl Processor for Limiter {
    type Update = LimiterState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        let sample_rate = config.sample_rate as f32;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        // Room for the longest lookahead, so a pick of another one allocates nothing.
        self.limiter.prepare(sample_rate, Lookahead::Five.seconds());
        self.aim(&self.state.targets(self.ramp_frames));
        self.gain.snap();
        self.quiet = 0;
    }

    fn update(&mut self, update: &mut LimiterState) {
        let targets = self.state.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn latency(&self) -> u32 {
        u32::try_from(self.limiter.latency()).unwrap_or(u32::MAX)
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        let frames = context.frames;
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = left_in
            .iter()
            .chain(right_in)
            .all(|sample| held(*sample) == 0.0);
        if silent_input && self.is_resting() {
            // Nothing sounds, nothing is left in the delay and nothing is turned down: the output
            // is silent already, and no glide can be heard.
            self.gain.snap();
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let gain_before = self.gain.current();
        let gain_step = (self.gain.advance(frames) - gain_before) / frames as f32;
        let ceiling = self.ceiling;
        let mut most_reduced = 1.0_f32;
        let inputs = left_in.iter().zip(right_in);
        let outputs = left_out.iter_mut().zip(right_out.iter_mut());
        for (index, ((left, right), (left_out, right_out))) in inputs.zip(outputs).enumerate() {
            let input = [held(*left), held(*right)];
            self.quiet = match input == [0.0; 2] {
                true => self.quiet.saturating_add(1),
                false => 0,
            };
            let gain = gain_before + gain_step * (index + 1) as f32;
            let frame = input.map(|sample| sample * gain);
            let peak = frame[0].abs().max(frame[1].abs());
            let (delayed, applied) = self.limiter.next(peak, frame, ceiling);
            most_reduced = most_reduced.min(applied);
            [*left_out, *right_out] = PeakLimiter::limit(delayed, applied, ceiling);
        }
        self.meters.output.record_block([&*left_out, &*right_out]);
        if most_reduced < 1.0 {
            self.meters.reduction.record(0, 1.0 / most_reduced);
        }
    }
}
