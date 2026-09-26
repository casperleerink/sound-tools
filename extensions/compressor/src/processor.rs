//! The compressor processor: a peak detector, a gain computer with a soft knee, the attack and
//! release, and the gain on the sound, which a lookahead may delay.
//!
//! The layout is the one of Giannoulis, Massberg and Reiss, "Digital Dynamic Range Compressor
//! Design: A Tutorial and Analysis" (2012): the gain computer works on the level in dB, and the
//! attack and release smooth the reduction it asks for, in dB. The level is the peak of the
//! last 10 ms of both channels. A steady tone has the same peak in every stretch of 10 ms, so
//! its level is still, the reduction arrives exactly where the static curve says, and the gain
//! does not wobble with the wave. That is what makes [`static_gain_db`] exact.
//!
//! The two channels share one level and one gain, so the stereo image does not move.

use std::f32::consts::LN_10;

use sound_core::{
    AudioInput, AudioOutput, CHANNELS, Peaks, Ports, PrepareConfig, ProcessContext, Processor,
    Smoothed,
};

use crate::{CompressorState, Lookahead};

/// How long a change of threshold, ratio, knee, makeup, mix or lookahead takes to arrive. A
/// jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// The detector keeps the peak of this many stretches of 1 ms, and of the one it is in: the
/// peak of the last 10 to 11 ms. Every tone from 50 Hz up has a peak in each such stretch, so
/// its level is steady.
const SEGMENTS: usize = 10;
const SEGMENT_SECONDS: f32 = 0.001;

/// How long a peak stays in the level after it passed: the release starts that much later.
pub const HOLD_SECONDS: f32 = SEGMENTS as f32 * SEGMENT_SECONDS;

/// Input louder than this is held to it, and input that is not a number or smaller than
/// [`TINY`] is silence, so nothing that comes in can make the output infinite or leave numbers
/// too small for the processor to work with at full speed. +36 dBFS: nothing real comes near.
const INPUT_LIMIT: f32 = 64.0;
const TINY: f32 = 1e-30;

/// A level under this is -180 dB, far under the lowest threshold and its knee.
const FLOOR: f32 = 1e-9;

/// A reduction this close to where it is going has arrived: 0.0001 dB, a gain error of one in
/// a hundred thousand. So a compressor that lets go comes to exactly 0 dB, and to rest.
const ARRIVED_DB: f64 = 1e-4;

/// How many dB the gain computer takes off a level of `level_db`: nothing under the knee, the
/// part `slope` of what is over the threshold above it, and a quadratic bend between, which
/// meets both without a corner. `slope` is `1 - 1 / ratio`.
fn reduction(level_db: f32, threshold_db: f32, slope: f32, knee_db: f32) -> f32 {
    let over = level_db - threshold_db;
    if 2.0 * over <= -knee_db {
        return 0.0;
    }
    if 2.0 * over >= knee_db {
        return slope * over;
    }
    let into = over + knee_db / 2.0;
    slope * into * into / (2.0 * knee_db)
}

fn slope_of(ratio: f32) -> f32 {
    1.0 - 1.0 / ratio
}

/// The reduction in dB that the compressor settles at for a steady sound whose peak is at
/// `input_db` dBFS: the static curve of the threshold, the ratio and the knee, 0 or more. The
/// card draws it, and the dot of the level on the card sits on it once the sound is steady.
pub fn reduction_db(state: &CompressorState, input_db: f32) -> f32 {
    reduction(
        input_db,
        state.threshold_db,
        slope_of(state.ratio),
        state.knee_db,
    )
}

/// The gain in dB of the whole compressor for a steady sound whose peak is at `input_db` dBFS,
/// once it has settled and every change has arrived: the reduction, the makeup and the mix of
/// the compressed and the dry sound. The dry sound goes through the same lookahead, so the two
/// are in phase and the mix is a sum of gains.
///
/// This is what the processor does, not a drawing of it: the tests hold the measured sound to
/// it. A tone from 50 Hz up is steady; its peak is the largest sample, which for a sine is its
/// amplitude to within 0.02 dB up to 1 kHz at 48 kHz.
pub fn static_gain_db(state: &CompressorState, input_db: f32) -> f32 {
    let gain = decibels_to_gain(state.makeup_db - reduction_db(state, input_db));
    20.0 * (1.0 + state.mix * (gain - 1.0)).log10()
}

fn decibels_to_gain(db: f32) -> f32 {
    (db * LN_10 / 20.0).exp()
}

/// The peak of the last 10 to 11 ms: the largest sample of each finished stretch of 1 ms in a
/// ring, and of the stretch it is in now. Only the end of a stretch looks at the ring.
struct Detector {
    stretches: [f32; SEGMENTS],
    /// Where the next finished stretch goes in the ring.
    next: usize,
    /// The largest of the ring.
    held: f32,
    /// The stretch it is in now: its largest sample and its frames so far.
    current: f32,
    frames: usize,
    stretch_frames: usize,
}

impl Detector {
    fn new(sample_rate: f32) -> Self {
        Self {
            stretches: [0.0; SEGMENTS],
            next: 0,
            held: 0.0,
            current: 0.0,
            frames: 0,
            stretch_frames: ((SEGMENT_SECONDS * sample_rate).round() as usize).max(1),
        }
    }

    /// Takes the next frame's peak of both channels, and gives the level now.
    fn next(&mut self, peak: f32) -> f32 {
        self.current = self.current.max(peak);
        let level = self.held.max(self.current);
        self.frames += 1;
        if self.frames == self.stretch_frames {
            self.stretches[self.next] = self.current;
            self.next = (self.next + 1) % SEGMENTS;
            self.held = self.stretches.iter().copied().fold(0.0, f32::max);
            self.current = 0.0;
            self.frames = 0;
        }
        level
    }

    /// The frames after which a silence has left the detector.
    fn window_frames(&self) -> usize {
        (SEGMENTS + 1) * self.stretch_frames
    }
}

/// The one-pole factor for a time constant: after `seconds` a step is 63 % of the way. In
/// `f64`, as is the reduction it moves: in `f32` a slow pole stops short of where it goes, when
/// the step it would take is smaller than the precision of the reduction.
fn pole(seconds: f32, sample_rate: f32) -> f64 {
    (-1.0 / (f64::from(seconds) * f64::from(sample_rate))).exp()
}

/// A sample as the compressor takes it: held to [`INPUT_LIMIT`], and silence for anything that
/// is not a number or is too small to matter.
fn held(sample: f32) -> f32 {
    if !(sample.abs() >= TINY) {
        return 0.0;
    }
    sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
}

/// What the compressor shows on its card, from the audio thread: the largest level of each
/// block as an amplitude, and the largest reduction of each block in dB, both on channel 0.
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

pub struct Compressor {
    sample_rate: f32,
    meters: Meters,
    /// The last record, to aim again when the sample rate is known.
    state: CompressorState,
    /// The frames a change takes.
    ramp_frames: f32,
    threshold: Smoothed,
    /// `1 - 1 / ratio`: 0 is no compression, 1 is a limiter. A glide of it is even in how
    /// much of the level over the threshold is taken off.
    slope: Smoothed,
    knee: Smoothed,
    makeup: Smoothed,
    mix: Smoothed,
    attack: f64,
    release: f64,
    detector: Detector,
    /// The reduction now, in dB, 0 or more.
    reduction: f64,
    /// The lookahead: every frame of the input, both channels, for 10 ms and one frame.
    delay: Vec<[f32; CHANNELS]>,
    /// Where the next frame goes.
    write: usize,
    /// The delay of the lookahead before a change and after it, in frames, and how far the
    /// sound has faded from the one to the other, 0 to 1. A new lookahead is a glide between
    /// two taps of one line, not a jump.
    from_frames: usize,
    to_frames: usize,
    fade: Smoothed,
    /// Frames in a row of silent input, up to what a silence needs to leave everything.
    quiet: usize,
}

impl Compressor {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    /// Starts at these values, so a compressor that is added or opened does not glide in.
    pub fn new(state: CompressorState, meters: Meters) -> Self {
        let sample_rate = 48_000.0;
        let lookahead = state.lookahead.frames(sample_rate);
        let mut compressor = Self {
            sample_rate,
            meters,
            state,
            ramp_frames: 1.0,
            threshold: Smoothed::new(0.0),
            slope: Smoothed::new(0.0),
            knee: Smoothed::new(0.0),
            makeup: Smoothed::new(0.0),
            mix: Smoothed::new(0.0),
            attack: 0.0,
            release: 0.0,
            detector: Detector::new(sample_rate),
            reduction: 0.0,
            delay: Vec::new(),
            write: 0,
            from_frames: lookahead,
            to_frames: lookahead,
            fade: Smoothed::new(1.0),
            quiet: 0,
        };
        compressor.start(sample_rate);
        compressor
    }

    /// Everything for a sample rate, from silence and with every value where the record says.
    /// Allocates: the control thread only.
    fn start(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.ramp_frames = (RAMP_SECONDS * sample_rate).max(1.0);
        self.detector = Detector::new(sample_rate);
        self.delay = vec![[0.0; CHANNELS]; Lookahead::Ten.frames(sample_rate) + 1];
        self.write = 0;
        self.reduction = 0.0;
        let lookahead = self.state.lookahead.frames(sample_rate);
        (self.from_frames, self.to_frames) = (lookahead, lookahead);
        self.aim(self.state);
        self.snap();
    }

    /// Sets every target from a record.
    fn aim(&mut self, state: CompressorState) {
        self.state = state;
        let ramp = self.ramp_frames;
        self.threshold.set_target(state.threshold_db, ramp);
        self.slope.set_target(slope_of(state.ratio), ramp);
        self.knee.set_target(state.knee_db, ramp);
        self.makeup.set_target(state.makeup_db, ramp);
        self.mix.set_target(state.mix, ramp);
        self.attack = pole(state.attack_ms / 1_000.0, self.sample_rate);
        self.release = pole(state.release_ms / 1_000.0, self.sample_rate);
        let lookahead = state.lookahead.frames(self.sample_rate);
        if lookahead != self.to_frames {
            // A change during the fade of the one before starts from where that one goes.
            self.from_frames = self.to_frames;
            self.to_frames = lookahead;
            self.fade = Smoothed::new(0.0);
            self.fade.set_target(1.0, ramp);
        }
    }

    fn smoothers(&mut self) -> [&mut Smoothed; 6] {
        [
            &mut self.threshold,
            &mut self.slope,
            &mut self.knee,
            &mut self.makeup,
            &mut self.mix,
            &mut self.fade,
        ]
    }

    /// Takes every target at once. For a compressor nobody hears, which has nothing to glide
    /// for.
    fn snap(&mut self) {
        self.smoothers().into_iter().for_each(Smoothed::snap);
        self.from_frames = self.to_frames;
    }

    /// Whether a silent block changes nothing: the silence has left the detector and the
    /// lookahead, and the reduction has let go.
    fn is_resting(&self) -> bool {
        self.reduction == 0.0 && self.quiet >= self.delay.len() + self.detector.window_frames()
    }

    fn tap(&self, frames: usize) -> [f32; CHANNELS] {
        let length = self.delay.len();
        self.delay[(self.write + length - frames.min(length - 1)) % length]
    }
}

impl Processor for Compressor {
    type Update = CompressorState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.start(config.sample_rate as f32);
    }

    fn update(&mut self, update: &mut CompressorState) {
        self.aim(*update);
    }

    fn latency(&self) -> u32 {
        self.to_frames as u32
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = left_in
            .iter()
            .chain(right_in)
            .all(|sample| held(*sample) == 0.0);
        if silent_input && self.is_resting() {
            // Nothing sounds, nothing is left in the lookahead and nothing is turned down: the
            // output is silent already, and no glide can be heard.
            self.snap();
            return;
        }
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let (mut loudest, mut most) = (0.0_f32, 0.0_f64);
        let frames = left_in
            .iter()
            .zip(right_in)
            .zip(left_out.iter_mut())
            .zip(right_out.iter_mut());
        for (((left_in, right_in), left_out), right_out) in frames {
            let input = [held(*left_in), held(*right_in)];
            self.quiet = match input == [0.0; CHANNELS] {
                true => self.quiet.saturating_add(1),
                false => 0,
            };
            self.delay[self.write] = input;
            let level = self.detector.next(input[0].abs().max(input[1].abs()));
            loudest = loudest.max(level);
            let level_db = 20.0 * level.max(FLOOR).log10();
            let threshold = self.threshold.advance(1);
            let slope = self.slope.advance(1);
            let knee = self.knee.advance(1);
            let target = f64::from(reduction(level_db, threshold, slope, knee));
            let pole = match target > self.reduction {
                true => self.attack,
                false => self.release,
            };
            self.reduction = target + pole * (self.reduction - target);
            if (self.reduction - target).abs() < ARRIVED_DB {
                self.reduction = target;
            }
            most = most.max(self.reduction);
            let gain = decibels_to_gain(self.makeup.advance(1) - self.reduction as f32);
            let gain = 1.0 + self.mix.advance(1) * (gain - 1.0);
            let fade = self.fade.advance(1);
            let (from, to) = (self.tap(self.from_frames), self.tap(self.to_frames));
            *left_out = (from[0] + fade * (to[0] - from[0])) * gain;
            *right_out = (from[1] + fade * (to[1] - from[1])) * gain;
            self.write = (self.write + 1) % self.delay.len();
        }
        self.meters.level.record(0, loudest);
        self.meters.reduction.record(0, most as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_the_knee_nothing_is_taken_off_and_above_it_the_ratio_is() {
        let state = CompressorState {
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 6.0,
            ..CompressorState::default()
        };
        assert_eq!(reduction_db(&state, -30.0), 0.0);
        assert_eq!(reduction_db(&state, -23.0), 0.0);
        // 12 dB over the threshold comes out 3 dB over it: 9 dB off.
        assert_eq!(reduction_db(&state, -8.0), 9.0);
        // In the middle of the knee, a quarter of the width times the slope, over two.
        let middle = reduction_db(&state, -20.0);
        assert!((middle - 0.75 * 9.0 / 12.0).abs() < 1e-6, "{middle}");
    }

    /// The bend meets the straight parts at both ends of the knee, so the curve has no corner.
    #[test]
    fn the_knee_meets_both_straight_parts() {
        for knee_db in [0.5, 6.0, 18.0] {
            let state = CompressorState {
                knee_db,
                ..CompressorState::default()
            };
            let (low, high) = (
                state.threshold_db - knee_db / 2.0,
                state.threshold_db + knee_db / 2.0,
            );
            let slope = slope_of(state.ratio);
            assert!(reduction_db(&state, low + 1e-3) < 1e-6);
            let at_high = reduction_db(&state, high - 1e-3);
            assert!((at_high - slope * (knee_db / 2.0)).abs() < 1e-3);
        }
    }

    #[test]
    fn makeup_and_mix_add_up_as_gains() {
        let state = CompressorState {
            threshold_db: -30.0,
            ratio: 2.0,
            knee_db: 0.0,
            makeup_db: 6.0,
            mix: 0.5,
            ..CompressorState::default()
        };
        // 20 dB over: 10 dB off, 6 dB back, and half of that with half of the dry sound.
        let wet = 10_f32.powf(-4.0 / 20.0);
        let expected = 20.0 * (0.5 + 0.5 * wet).log10();
        assert!((static_gain_db(&state, -10.0) - expected).abs() < 1e-5);
    }

    #[test]
    fn a_steady_level_is_the_peak_of_the_last_ten_milliseconds() {
        let mut detector = Detector::new(48_000.0);
        assert_eq!(detector.next(0.5), 0.5);
        for _ in 0..480 {
            assert_eq!(detector.next(0.1), 0.5);
        }
        // After 11 ms the peak has left.
        for _ in 0..48 {
            detector.next(0.1);
        }
        assert_eq!(detector.next(0.1), 0.1);
    }
}
