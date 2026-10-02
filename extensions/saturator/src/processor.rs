//! The saturator processor: the drive into the curve at four times the sample rate, the
//! automatic gain, a DC blocker, the tone tilt, the output and the mix.
//!
//! A curve bends every peak it rounds off into harmonics, and a hard bend makes them far above
//! the top of hearing. At the sample rate those fold back as tones that are not in tune with
//! anything. So the curve runs at four times the rate, between two stages of oversampling that
//! take the high harmonics away first, see [`Oversampler`]. The dry sound
//! waits in a delay as long as the oversampling, so the two stay in phase in the mix.
//!
//! The automatic gain turns the saturated sound down by what the drive adds to a sine at
//! -12 dBFS, so the level stays where it was as the drive rises. A curve that leans to one
//! side adds a little DC, which a one-pole high pass at 5 Hz takes away.
//!
//! Every change glides over 20 ms. A change of curve is a glide from the one curve to the
//! other: both run while it lasts, weighted.

use std::f32::consts::PI;

use crate::{Curve, DRIVE, MIX, OUTPUT, PARAMETERS, SaturatorState, TONE};
use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, OnePole, Oversampler,
    OversamplingFilters, Ports, PrepareConfig, ProcessContext, Processor, Smoothed, Targets, held,
    soft_clip,
};

/// Every number of the saturator can be automated.
type SaturatorTargets = Targets<SaturatorState, { PARAMETERS.len() }>;

/// How many frames the saturator delays the sound, at every sample rate: 1.3 ms at 48 kHz. It
/// is reported as its latency, so the track stays in time.
pub const LATENCY: u32 = Oversampler::DELAY_FRAMES as u32;

/// How long a change takes to arrive. A jump would click, or step in the sound.
const RAMP_SECONDS: f32 = 0.02;

/// The level at which the drive leaves the level of a sine as it was: -12 dBFS, the peak of a
/// sound in a mix with room above it.
const REFERENCE: f32 = 0.25;

/// How far the tube curve leans: an offset into `tanh`. At a drive that takes a sine to the
/// knee, its second harmonic is about 17 dB under the tone.
const TUBE_BIAS: f32 = 0.3;

/// The DC blocker after the curve.
const DC_HZ: f32 = 5.0;

/// The tone tilts around this frequency: it keeps its level at every tone.
const PIVOT_HZ: f32 = 1_000.0;

/// A tone crossover the tilt pushes up stays under a part of the sample rate, below the Nyquist
/// frequency where its bent frequency would run away.
const HIGHEST_PART: f32 = 0.45;

/// While the tone moves, its factors are worked out again this often. Four times per block of
/// the engine: a sweep has no steps anyone can hear, and a `tan` per frame is not needed.
const FACTOR_FRAMES: usize = 16;

/// Frames of silent input after which the dry delay and the oversampling hold only zeros: more
/// than the delay and the memory of every stage.
const QUIET_FRAMES: usize = 2 * Oversampler::DELAY_FRAMES + 32;

/// The dry sound waits this many frames at most: a power of two over the latency.
const DRY_FRAMES: usize = 128;

/// A curve at `sample`, after the drive. Every curve has a slope of 1 at 0, and 0 at 0.
fn shape(curve: Curve, sample: f32) -> f32 {
    match curve {
        Curve::Soft => tanh(sample),
        Curve::Tape => tape(sample),
        Curve::Tube => tube(sample),
        Curve::Clip => soft_clip(sample),
    }
}

fn tape(sample: f32) -> f32 {
    sample / (1.0 + sample * sample).sqrt()
}

/// `tanh` moved along by the bias, and back to 0 at 0 with a slope of 1 there.
fn tube(sample: f32) -> f32 {
    let bias = tanh(TUBE_BIAS);
    (tanh(sample + TUBE_BIAS) - bias) / (1.0 - bias * bias)
}

/// `tanh` as a ratio of two polynomials, to within 3e-7 of it: the one of Eigen
/// (`generic_fast_tanh_float`). It runs four times per frame and channel, and the one of the
/// system library took as long as all of the oversampling.
fn tanh(sample: f32) -> f32 {
    // Past this `tanh` is 1 in `f32`, and the polynomials would part from it.
    const END: f32 = 7.905_311;
    const ABOVE: [f32; 7] = [
        4.893_524_6e-3,
        6.372_619_3e-4,
        1.485_722_4e-5,
        5.122_297e-8,
        -8.604_671_5e-11,
        2.000_187_9e-13,
        -2.760_768_5e-16,
    ];
    const BELOW: [f32; 4] = [4.893_525e-3, 2.268_434_6e-3, 1.185_347_1e-4, 1.198_258_4e-6];
    let sample = sample.clamp(-END, END);
    let square = sample * sample;
    let above = ABOVE
        .iter()
        .rev()
        .fold(0.0, |sum, factor| sum * square + factor);
    let below = BELOW
        .iter()
        .rev()
        .fold(0.0, |sum, factor| sum * square + factor);
    sample * above / below
}

/// The weight each curve has in the sound: one for the curve of a record, and in between
/// during a glide.
fn weights(curve: Curve) -> [f32; 4] {
    Curve::ALL.map(|each| if each == curve { 1.0 } else { 0.0 })
}

/// The curves at their weights.
fn blend(weights: &[f32; 4], sample: f32) -> f32 {
    let mut sum = 0.0;
    for (curve, weight) in Curve::ALL.into_iter().zip(weights) {
        if *weight != 0.0 {
            sum += weight * shape(curve, sample);
        }
    }
    sum
}

/// Every sample at four times the rate through one curve, with the drive and the automatic gain
/// of its frame.
fn shape_all(four: &mut [f32], moves: &[Frame], curve: impl Fn(f32) -> f32) {
    for (samples, frame) in four.chunks_exact_mut(4).zip(moves) {
        for sample in samples {
            *sample = frame.level * curve(frame.drive * *sample);
        }
    }
}

/// The same while one curve glides to another, with the weights of each frame.
fn shape_all_blended(four: &mut [f32], moves: &[Frame]) {
    for (samples, frame) in four.chunks_exact_mut(4).zip(moves) {
        for sample in samples {
            *sample = frame.level * blend(&frame.weights, frame.drive * *sample);
        }
    }
}

/// The gain after the curve that gives a sine at [`REFERENCE`] its level back: the reference
/// over what the curve makes of it after the drive. For a curve that leans, the middle of what
/// it makes of both peaks.
fn level_gain(shape: impl Fn(f32) -> f32, drive: f32) -> f32 {
    let driven = drive * REFERENCE;
    REFERENCE * 2.0 / (shape(driven) - shape(-driven))
}

fn decibels_to_gain(db: f32) -> f32 {
    10_f32.powf(db / 20.0)
}

/// The automatic gain of a curve at a drive, as a factor: what the saturator turns the curved
/// sound down by, so a sine at -12 dBFS keeps its level.
pub fn auto_gain(curve: Curve, drive_db: f32) -> f32 {
    level_gain(|sample| shape(curve, sample), decibels_to_gain(drive_db))
}

/// What comes out for a steady `input` from -1 to 1 once every change has arrived, with the
/// drive, the curve, the automatic gain, the output and the mix. The tone and the DC blocker
/// are left out: they are about frequency, and this is the shape. The card draws it.
pub fn transfer(state: &SaturatorState, input: f32) -> f32 {
    let drive = decibels_to_gain(state.drive_db);
    let wet = decibels_to_gain(state.output_db)
        * auto_gain(state.curve, state.drive_db)
        * shape(state.curve, drive * input);
    input + state.mix * (wet - input)
}

/// A frequency as a one-pole filter in the trapezoidal form sees it: `tan(π hz / rate)`.
fn bent(hz: f32, sample_rate: f32) -> f32 {
    (PI * hz / sample_rate).tan()
}

/// The crossover of the tone, bent, and its gains below and above it. A tilt of `db` is a gain
/// of `db / 2` above and `-db / 2` below. With `k` the gain above, a first order split at the
/// pivot `ω` times `k` keeps the pivot at its level: `(k s + ω) / (s + ω k)` has size 1 at
/// `s = jω`. It is exact for the bent frequencies, so the crossover is the bent pivot times `k`.
fn tilt(tone_db: f32, sample_rate: f32) -> (f32, f32, f32) {
    let above = decibels_to_gain(tone_db / 2.0);
    let highest = bent(HIGHEST_PART * sample_rate, sample_rate);
    let crossover = (bent(PIVOT_HZ, sample_rate) * above).min(highest);
    (crossover, 1.0 / above, above)
}

/// The gain at `hz` of a saturator with this record, as a factor, once every change has
/// arrived: what a quiet steady sine comes out with. For a quiet sound every curve is a straight
/// line of slope 1, so this is the drive and the automatic gain, the DC blocker, the tone, the
/// output and the mix.
///
/// This is the exact response of the processor up to 20 kHz at 44.1 kHz and above, not a
/// drawing of one: the tone and the DC blocker are one-pole filters in the trapezoidal form,
/// which is the analog filter with its frequencies bent by `tan(π f / sample rate)`. The
/// oversampling is flat to 0.001 dB up to 0.45 of the sample rate and to 0.01 dB at 20 kHz at
/// 44.1 kHz, and the dry sound waits as long as the saturated one. The tests hold the measured
/// sound to it.
pub fn response(state: &SaturatorState, hz: f32, sample_rate: f32) -> f32 {
    let at = (f64::from(PI) * f64::from(hz) / f64::from(sample_rate)).tan();
    // A one-pole low pass at a bent corner, as a complex number: `1 / (1 + j at / corner)`.
    let low_pass = |corner: f32| {
        let ratio = at / f64::from(corner);
        let size = 1.0 + ratio * ratio;
        (1.0 / size, -ratio / size)
    };
    let (crossover, below, above) = tilt(state.tone_db, sample_rate);
    let (below, above) = (f64::from(below), f64::from(above));
    let low = low_pass(crossover);
    // The high pass is 1 less the low pass.
    let tone = (
        below * low.0 + above * (1.0 - low.0),
        below * low.1 - above * low.1,
    );
    let dc = low_pass(bent(DC_HZ, sample_rate));
    let dc = (1.0 - dc.0, -dc.1);
    let gain = f64::from(
        decibels_to_gain(state.drive_db)
            * auto_gain(state.curve, state.drive_db)
            * decibels_to_gain(state.output_db),
    );
    let wet = (
        gain * (tone.0 * dc.0 - tone.1 * dc.1),
        gain * (tone.0 * dc.1 + tone.1 * dc.0),
    );
    let mix = f64::from(state.mix);
    let (real, imaginary) = (1.0 - mix + mix * wet.0, mix * wet.1);
    real.hypot(imaginary) as f32
}

/// What one frame of the curve and the mix is: where every glide is at that frame.
#[derive(Clone, Copy, Default)]
struct Frame {
    drive: f32,
    weights: [f32; 4],
    level: f32,
    output: f32,
    mix: f32,
}

/// Everything of one channel that remembers.
#[derive(Clone, Copy)]
struct Channel {
    oversampler: Oversampler,
    dc: OnePole,
    tone: OnePole,
    /// The dry sound, for the frames of the latency.
    dry: [f32; DRY_FRAMES],
}

impl Channel {
    fn new() -> Self {
        Self {
            oversampler: Oversampler::new(),
            dc: OnePole::default(),
            tone: OnePole::default(),
            dry: [0.0; DRY_FRAMES],
        }
    }
}

pub struct Saturator {
    /// The record, with the values of the lanes that automate it.
    state: Automated<SaturatorState, { PARAMETERS.len() }>,
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    filters: OversamplingFilters,
    /// The gain into the curve, as a factor.
    drive: Smoothed,
    /// The weight of each curve, in the order of [`Curve::ALL`].
    weights: [Smoothed; 4],
    tone: Smoothed,
    /// The gain on the saturated sound, as a factor.
    output: Smoothed,
    mix: Smoothed,
    /// The automatic gain for where the drive and the weights are now.
    level: f32,
    /// The factors of the DC blocker and of the tone crossover.
    dc_factor: f32,
    tone_factor: f32,
    /// The gains of the tone below and above its crossover at the end of the last run of
    /// frames, and where they are going in the run now. They move frame by frame inside a run,
    /// so a glide of the tone makes no step.
    gains: [f32; 2],
    gains_target: [f32; 2],
    /// Whether the factors have to be worked out again although nothing glides: after an update
    /// that snapped, and before the first block.
    stale: bool,
    channels: [Channel; CHANNELS],
    /// Where the next dry frame goes.
    write: usize,
    /// Frames in a row of silent input, up to what a silence needs to leave everything.
    quiet: usize,
}

impl Saturator {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<SaturatorState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    /// Starts at these values, so a saturator that is added or opened does not glide in.
    pub fn new(state: SaturatorState) -> Self {
        let sample_rate = 48_000.0;
        let mut saturator = Self {
            state: Automated::new(Self::AUTOMATION, state),
            sample_rate,
            ramp_frames: 1.0,
            filters: OversamplingFilters::new(),
            drive: Smoothed::new(1.0),
            weights: [0.0; 4].map(Smoothed::new),
            tone: Smoothed::new(0.0),
            output: Smoothed::new(1.0),
            mix: Smoothed::new(1.0),
            level: 1.0,
            dc_factor: OnePole::factor(DC_HZ, sample_rate),
            tone_factor: 0.0,
            gains: [1.0; 2],
            gains_target: [1.0; 2],
            stale: true,
            channels: [Channel::new(); CHANNELS],
            write: 0,
            quiet: QUIET_FRAMES,
        };
        saturator.aim(&saturator.state.targets(saturator.ramp_frames));
        saturator.snap();
        saturator
    }

    /// Sets every target from the record and its lanes, each reached in its own ramp.
    fn aim(&mut self, targets: &SaturatorTargets) {
        let state = *self.state;
        self.drive
            .set_target(decibels_to_gain(state.drive_db), targets.ramp(&DRIVE));
        for (weight, target) in self.weights.iter_mut().zip(weights(state.curve)) {
            weight.set_target(target, targets.edit());
        }
        self.tone.set_target(state.tone_db, targets.ramp(&TONE));
        self.output
            .set_target(decibels_to_gain(state.output_db), targets.ramp(&OUTPUT));
        self.mix.set_target(state.mix, targets.ramp(&MIX));
        // A number that took its value at once does not move, so nothing else says the
        // factors are old.
        self.stale |= targets.snaps();
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        let [soft, tape, tube, clip] = &mut self.weights;
        [
            &mut self.drive,
            soft,
            tape,
            tube,
            clip,
            &mut self.tone,
            &mut self.output,
            &mut self.mix,
        ]
        .into_iter()
    }

    /// Takes every target at once. For a saturator nobody hears, which has nothing to glide
    /// for.
    fn snap(&mut self) {
        self.smoothers().for_each(Smoothed::snap);
        self.stale = true;
    }

    /// Moves the tone `frames` along, and works out its factors when it moves.
    fn move_tone(&mut self, frames: usize) {
        self.gains = self.gains_target;
        if !self.stale && !self.tone.is_moving() {
            return;
        }
        let tone = self.tone.advance(frames);
        let (crossover, below, above) = tilt(tone, self.sample_rate);
        self.tone_factor = OnePole::bent_factor(crossover);
        self.gains_target = [below, above];
        // After a snap there is nothing to glide from.
        if self.stale {
            self.gains = self.gains_target;
        }
    }

    /// Moves everything but the tone one frame along, and works out the automatic gain for
    /// where the drive and the weights are when they move.
    fn move_frame(&mut self) -> Frame {
        let changes =
            self.stale || self.drive.is_moving() || self.weights.iter().any(Smoothed::is_moving);
        let drive = self.drive.advance(1);
        let weights = self.weights.each_mut().map(|weight| weight.advance(1));
        if changes {
            self.level = level_gain(|sample| blend(&weights, sample), drive);
        }
        Frame {
            drive,
            weights,
            level: self.level,
            output: self.output.advance(1),
            mix: self.mix.advance(1),
        }
    }

    /// The curve of every frame from here to the end of a run of frames, unless it glides from
    /// one curve to another. The weights move only after an update, which comes between blocks.
    fn steady_curve(&self) -> Option<Curve> {
        if self.weights.iter().any(Smoothed::is_moving) {
            return None;
        }
        let mut curves = Curve::ALL.into_iter().zip(&self.weights);
        curves
            .find(|(_, weight)| weight.current() == 1.0)
            .map(|(curve, _)| curve)
    }

    fn is_resting(&self) -> bool {
        self.quiet >= QUIET_FRAMES
            && self
                .channels
                .iter()
                .all(|channel| channel.dc.is_silent() && channel.tone.is_silent())
    }
}

impl Processor for Saturator {
    type Update = SaturatorState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.ramp_frames = (RAMP_SECONDS * self.sample_rate).max(1.0);
        self.dc_factor = OnePole::factor(DC_HZ, self.sample_rate);
        self.stale = true;
    }

    fn update(&mut self, update: &mut SaturatorState) {
        let targets = self.state.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn latency(&self) -> u32 {
        LATENCY
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = left_in
            .iter()
            .chain(right_in)
            .all(|sample| held(*sample) == 0.0);
        if silent_input && self.is_resting() {
            // Nothing sounds and nothing is left in the delays: the output is silent already,
            // and no glide can be heard.
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
            let length = left_in.len();
            self.move_tone(length);
            let steady = self.steady_curve();
            let mut moves = [Frame::default(); FACTOR_FRAMES];
            let moves = &mut moves[..length];
            for (frame, (left, right)) in moves.iter_mut().zip(left_in.iter().zip(right_in)) {
                *frame = self.move_frame();
                self.quiet = match held(*left) == 0.0 && held(*right) == 0.0 {
                    true => self.quiet.saturating_add(1),
                    false => 0,
                };
            }
            let [left, right] = &mut self.channels;
            for (channel, input, output) in
                [(left, left_in, left_out), (right, right_in, right_out)]
            {
                let mut frames = [0.0; FACTOR_FRAMES];
                let frames = &mut frames[..length];
                let mut four = [0.0; 4 * FACTOR_FRAMES];
                let four = &mut four[..4 * length];
                for (index, (frame, input)) in frames.iter_mut().zip(input).enumerate() {
                    *frame = held(*input);
                    channel.dry[(self.write + index) % DRY_FRAMES] = *frame;
                }
                channel.oversampler.up(&self.filters, frames, four);
                match steady {
                    Some(Curve::Soft) => shape_all(four, moves, tanh),
                    Some(Curve::Tape) => shape_all(four, moves, tape),
                    Some(Curve::Tube) => shape_all(four, moves, tube),
                    Some(Curve::Clip) => shape_all(four, moves, soft_clip),
                    None => shape_all_blended(four, moves),
                }
                channel.oversampler.down(&self.filters, four, frames);
                let ([below, above], [to_below, to_above]) = (self.gains, self.gains_target);
                let frames = frames.iter().zip(output.iter_mut()).zip(moves.iter());
                for (index, ((curved, output), frame)) in frames.enumerate() {
                    let delayed = self.write + index + DRY_FRAMES - Oversampler::DELAY_FRAMES;
                    let dry = channel.dry[delayed % DRY_FRAMES];
                    let blocked = curved - channel.dc.low(self.dc_factor, *curved);
                    let low = channel.tone.low(self.tone_factor, blocked);
                    let along = (index + 1) as f32 / length as f32;
                    let below = below + (to_below - below) * along;
                    let above = above + (to_above - above) * along;
                    let wet = frame.output * (below * low + above * (blocked - low));
                    *output = dry + frame.mix * (wet - dry);
                }
            }
            self.write = (self.write + length) % DRY_FRAMES;
            self.stale = false;
        }
        if silent_input {
            for channel in &mut self.channels {
                channel.dc.settle();
                channel.tone.settle();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fast_tanh_is_tanh() {
        for step in -200_000..=200_000 {
            let sample = step as f32 * 1e-4;
            let error = (tanh(sample) - sample.tanh()).abs();
            assert!(error < 5e-7, "{sample}: {error}");
        }
        assert_eq!(tanh(0.0), 0.0);
        assert_eq!(tanh(1e9), -tanh(-1e9));
    }

    #[test]
    fn every_curve_is_zero_at_zero_with_a_slope_of_one() {
        for curve in Curve::ALL {
            assert_eq!(shape(curve, 0.0), 0.0, "{curve:?}");
            let slope = (shape(curve, 1e-4) - shape(curve, -1e-4)) / 2e-4;
            assert!((slope - 1.0).abs() < 1e-3, "{curve:?}: {slope}");
        }
    }

    #[test]
    fn the_tube_leans_and_the_others_do_not() {
        for curve in Curve::ALL {
            let (up, down) = (shape(curve, 2.0), -shape(curve, -2.0));
            match curve {
                Curve::Tube => assert!(down > up * 1.5, "{up} {down}"),
                _ => assert_eq!(up, down, "{curve:?}"),
            }
        }
    }

    /// A sine at -12 dBFS keeps its peak through the drive and the automatic gain.
    #[test]
    fn the_automatic_gain_keeps_the_reference_where_it_was() {
        for curve in Curve::ALL {
            for drive_db in [0.0, 6.0, 18.0, 36.0] {
                let state = SaturatorState {
                    curve,
                    drive_db,
                    ..SaturatorState::default()
                };
                let (up, down) = (transfer(&state, REFERENCE), -transfer(&state, -REFERENCE));
                assert!(
                    ((up + down) / 2.0 - REFERENCE).abs() < 1e-6,
                    "{curve:?} {drive_db}"
                );
            }
        }
    }

    /// Under its ceiling the clip curve is a straight line, so up to the drive it only turns the
    /// sound up and the automatic gain takes that off again.
    #[test]
    fn under_its_ceiling_the_clip_curve_is_the_sound_as_it_came() {
        let state = SaturatorState {
            curve: Curve::Clip,
            drive_db: 12.0,
            ..SaturatorState::default()
        };
        for input in [-0.25, -0.1, 0.0, 0.2, 0.25] {
            assert!((transfer(&state, input) - input).abs() < 1e-6);
        }
        assert!(transfer(&state, 1.0) < 0.4);
    }

    #[test]
    fn the_tone_keeps_the_pivot_and_tilts_the_ends() {
        for tone_db in [-12.0, -5.0, 0.0, 7.0, 12.0] {
            let state = SaturatorState {
                tone_db,
                drive_db: 0.0,
                curve: Curve::Clip,
                ..SaturatorState::default()
            };
            let db = |hz| 20.0 * response(&state, hz, 48_000.0).log10();
            assert!(db(PIVOT_HZ).abs() < 0.01, "{tone_db}: {}", db(PIVOT_HZ));
            assert!((db(20_000.0) - tone_db / 2.0).abs() < 0.4, "{tone_db}");
            assert!((db(50.0) + tone_db / 2.0).abs() < 0.4, "{tone_db}");
        }
    }
}
