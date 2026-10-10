//! The gate processor: the peak detector of the compressor, a gate that opens over the
//! threshold and holds, and a transient shaper on the same level.
//!
//! The gate's gain moves between 1 and the floor (`-range_db`) as a one-pole glide, at the
//! attack when it opens and the release when it closes, so a closing gate fades as a tail
//! does. While something feeds the sidechain input, the gate hears that instead of the sound.
//!
//! The shaper hears the sound itself, never the key. Two followers of the level tell the start
//! of a hit from its tail: one rises over 20 ms and falls at once, so the level is above it
//! only while a hit starts; the other rises at once and falls over 500 ms, so it is above the
//! level only while a hit dies away. The part of the level each gap is weighs the transient
//! and the sustain gains. A steady sound has no gap, so it passes at exactly its own level.
//!
//! The two channels share one level and one gain, so the stereo image does not move.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, PeakDetector, Peaks, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Targets, all_held_silent, amplitude, held, pole,
};

use crate::{GateState, PARAMETERS, RANGE, SUSTAIN, TRANSIENT};

type GateTargets = Targets<GateState, { PARAMETERS.len() }>;

/// How long a change of range, transient or sustain takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// How long the start of a hit counts as its transient: the rise of the slow follower.
const TRANSIENT_SECONDS: f32 = 0.02;

/// How long the tail of a hit counts as its sustain: the fall of the slow follower.
const SUSTAIN_SECONDS: f32 = 0.5;

/// A gain or a follower this close to where it goes has arrived, as a part of it. So an open
/// gate comes to exactly 1, and a steady sound to exactly no shaping.
const ARRIVED: f64 = 1e-6;

/// The gain of the gate for a steady sound whose peak is at `input_db` dBFS, once it has
/// settled: 0 dB from the threshold up, `-range_db` under it. A steady sound has no transient
/// and no tail, so the shaper leaves it alone. The tests hold the measured sound to it.
pub fn static_gain_db(state: &GateState, input_db: f32) -> f32 {
    match input_db >= state.threshold_db {
        true => 0.0,
        false => -state.range_db,
    }
}

/// What the gate shows on its card, from the audio thread: the largest level it heard in each
/// block as an amplitude, and the most it turned down in each block in dB, both on channel 0.
#[derive(Clone, Debug, Default)]
pub struct Meters {
    pub level: Peaks,
    pub reduction: Peaks,
}

impl Meters {
    /// The names the behaviour keeps them under, for [`sound_core::Project::peaks`].
    pub const LEVEL: &str = "level";
    pub const REDUCTION: &str = "reduction";
}

pub struct Gate {
    sample_rate: f32,
    meters: Meters,
    /// The record, with the values of the lanes that automate it.
    state: Automated<GateState, { PARAMETERS.len() }>,
    ramp_frames: f32,
    /// The threshold as an amplitude. A jump of it does not click: the gain still glides.
    threshold: f32,
    hold_frames: usize,
    attack: f64,
    release: f64,
    /// The gain of a closed gate.
    floor: Smoothed,
    /// The gains on a transient and on a tail, as factors.
    transient: Smoothed,
    sustain: Smoothed,
    /// The level of the sound, and of the sidechain while one is connected.
    detector: PeakDetector,
    key_detector: PeakDetector,
    /// The gain of the gate now, between the floor and 1.
    gain: f64,
    /// Frames the gate stays open for, after the level fell under the threshold.
    holding: usize,
    /// Follows the level up over [`TRANSIENT_SECONDS`] and down at once.
    rising: f64,
    /// Follows the level up at once and down over [`SUSTAIN_SECONDS`].
    falling: f64,
    rising_pole: f64,
    falling_pole: f64,
    /// Frames in a row of silence in the input and the sidechain.
    quiet: usize,
}

impl Gate {
    pub const INPUT: AudioInput = AudioInput::new(0);
    /// What the gate hears instead of the input, while anything feeds it.
    pub const SIDECHAIN: AudioInput = AudioInput::new(1);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<GateState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    /// Starts at these values, so a gate that is added or opened does not glide in.
    pub fn new(state: GateState, meters: Meters) -> Self {
        let sample_rate = 48_000.0;
        let mut gate = Self {
            sample_rate,
            meters,
            state: Automated::new(Self::AUTOMATION, state),
            ramp_frames: 1.0,
            threshold: 0.0,
            hold_frames: 0,
            attack: 0.0,
            release: 0.0,
            floor: Smoothed::new(0.0),
            transient: Smoothed::new(1.0),
            sustain: Smoothed::new(1.0),
            detector: PeakDetector::new(sample_rate),
            key_detector: PeakDetector::new(sample_rate),
            gain: 0.0,
            holding: 0,
            rising: 0.0,
            falling: 0.0,
            rising_pole: 0.0,
            falling_pole: 0.0,
            quiet: 0,
        };
        gate.start(sample_rate);
        gate
    }

    /// Everything for a sample rate, at rest and with every value where the record says.
    fn start(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        self.detector = PeakDetector::new(sample_rate);
        self.key_detector = PeakDetector::new(sample_rate);
        self.rising_pole = pole(TRANSIENT_SECONDS, sample_rate);
        self.falling_pole = pole(SUSTAIN_SECONDS, sample_rate);
        self.aim(&self.state.targets(self.ramp_frames));
        self.rest();
        self.quiet = self.detector.window_frames();
    }

    /// Sets every target from the record and its lanes. The `exp` of the times runs here, when
    /// a value moves, and never per frame.
    fn aim(&mut self, targets: &GateTargets) {
        let state = *self.state;
        self.threshold = amplitude(state.threshold_db);
        self.hold_frames = (state.hold_ms / 1_000.0 * self.sample_rate).round() as usize;
        self.attack = pole(state.attack_ms / 1_000.0, self.sample_rate);
        self.release = pole(state.release_ms / 1_000.0, self.sample_rate);
        self.floor
            .set_target(amplitude(-state.range_db), targets.ramp(&RANGE));
        self.transient
            .set_target(amplitude(state.transient_db), targets.ramp(&TRANSIENT));
        self.sustain
            .set_target(amplitude(state.sustain_db), targets.ramp(&SUSTAIN));
    }

    /// The state silence leads to: closed, nothing held or followed, every value arrived. Only
    /// while the output is silent whatever the gain, so nobody hears the jump; and the sound
    /// that comes next is then treated the same whatever played before.
    fn rest(&mut self) {
        for smoothed in [&mut self.floor, &mut self.transient, &mut self.sustain] {
            smoothed.snap();
        }
        self.detector.restart();
        self.key_detector.restart();
        self.gain = f64::from(self.floor.current());
        self.holding = 0;
        self.rising = 0.0;
        self.falling = 0.0;
    }

    /// Whether the input and the sidechain have been silent long enough to have left the
    /// detectors.
    fn is_resting(&self) -> bool {
        self.quiet >= self.detector.window_frames()
    }

    /// The gain of the gate for the next frame, from the level it hears.
    #[inline]
    fn gate(&mut self, level: f32) -> f64 {
        let open = match level >= self.threshold {
            true => {
                self.holding = self.hold_frames;
                true
            }
            false if self.holding > 0 => {
                self.holding -= 1;
                true
            }
            false => false,
        };
        let floor = f64::from(self.floor.advance(1));
        let target = if open { 1.0 } else { floor };
        let pole = match target > self.gain {
            true => self.attack,
            false => self.release,
        };
        self.gain = target + pole * (self.gain - target);
        if (self.gain - target).abs() <= ARRIVED * target {
            self.gain = target;
        }
        self.gain
    }

    /// The gain of the shaper for the next frame, from the level of the sound.
    #[inline]
    fn shape(&mut self, level: f32) -> f64 {
        let level = f64::from(level);
        self.rising = match level <= self.rising {
            true => level,
            false => level + self.rising_pole * (self.rising - level),
        };
        if level - self.rising <= ARRIVED * level {
            self.rising = level;
        }
        self.falling = match level >= self.falling {
            true => level,
            false => level + self.falling_pole * (self.falling - level),
        };
        if self.falling - level <= ARRIVED * self.falling {
            self.falling = level;
        }
        let transient = f64::from(self.transient.advance(1));
        let sustain = f64::from(self.sustain.advance(1));
        if level == 0.0 {
            return 1.0;
        }
        // How much of the level is a start, and how much of the follower is a tail: 0 to 1.
        let start = 1.0 - self.rising / level;
        let tail = 1.0 - level / self.falling;
        (1.0 + (transient - 1.0) * start) * (1.0 + (sustain - 1.0) * tail)
    }
}

impl Processor for Gate {
    type Update = GateState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .side_audio_input(Self::SIDECHAIN)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.start(config.sample_rate as f32);
    }

    fn update(&mut self, update: &mut GateState) {
        let targets = self.state.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        let inputs = &context.audio_inputs;
        let [left_in, right_in] = inputs.get(Self::INPUT);
        let keyed = inputs.is_connected(Self::SIDECHAIN);
        let [left_key, right_key] = match keyed {
            true => inputs.get(Self::SIDECHAIN),
            false => [left_in, right_in],
        };
        let silent_input = [left_in, right_in, left_key, right_key]
            .into_iter()
            .all(all_held_silent);
        if silent_input && self.is_resting() {
            // Silence in and nothing left in the detectors: the output is silent already.
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let (mut loudest, mut lowest) = (0.0_f32, 1.0_f64);
        let frames = left_in
            .iter()
            .zip(right_in)
            .zip(left_key.iter().zip(right_key))
            .zip(left_out.iter_mut().zip(right_out.iter_mut()));
        for (((left, right), (left_key, right_key)), (left_out, right_out)) in frames {
            let input = [held(*left), held(*right)];
            let key = [held(*left_key), held(*right_key)];
            let silent = input == [0.0; 2] && key == [0.0; 2];
            if !silent && self.is_resting() {
                self.rest();
            }
            self.quiet = match silent {
                true => self.quiet.saturating_add(1),
                false => 0,
            };
            let level = self.detector.next(input[0].abs().max(input[1].abs()));
            let heard = match keyed {
                true => self.key_detector.next(key[0].abs().max(key[1].abs())),
                false => level,
            };
            loudest = loudest.max(heard);
            let gate = self.gate(heard);
            lowest = lowest.min(gate);
            let gain = (gate * self.shape(level)) as f32;
            *left_out = input[0] * gain;
            *right_out = input[1] * gain;
        }
        self.meters.level.record(0, loudest);
        // One `log10` per block, for the card.
        self.meters
            .reduction
            .record(0, (-20.0 * lowest.max(1e-9).log10()) as f32);
    }
}
