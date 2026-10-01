//! The utility processor: a matrix of the two channels, the bass in mono, and a gain for each
//! channel, in that order.
//!
//! The matrix does which channels play, which are turned upside down, and the width. The width
//! keeps the sum of the two channels (the mid) and scales their difference (the side): 0 leaves
//! only the mid, which is mono, and 2 doubles the side.
//!
//! Bass mono takes the side away below its frequency, with a Linkwitz-Riley crossover of the
//! fourth order: two Butterworth sections in a row. The side goes through its high pass. The mid
//! goes through the all-pass that the low and the high pass of the crossover add up to, so the
//! mid and the side keep one phase and a mono sound comes out at its own level at every
//! frequency. Each section is the state variable filter of the Filter in its trapezoidal form
//! (Simper 2013), which stays stable while its frequency glides.
//!
//! Last, each channel gets its gain: the gain in dB, the pan with the pan law of a track, and
//! mute. Every value glides over 20 ms, the choices too: the entries of the matrix, the part of
//! the crossover that is heard and the gains. At the defaults nothing moves and every stage is
//! one, so the input is copied to the output and every sample comes out as it went in, to the
//! bit.

use std::f32::consts::{PI, SQRT_2};

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Targets, amplitude, pan_gains,
};

use crate::{BASS_MONO_HZ, Channels, GAIN, PAN, PARAMETERS, UtilityState, WIDTH};

/// Every number of the utility can be automated.
type UtilityTargets = Targets<UtilityState, { PARAMETERS.len() }>;

/// How long a change takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;

/// While the crossover frequency moves, its factors are worked out again this often.
const FACTOR_FRAMES: usize = 16;

/// The crossover stays under this part of the sample rate, below the Nyquist frequency where
/// its factors would run away.
const HIGHEST_PART: f32 = 0.45;

/// Input louder than this, or not a number, is held to it once the utility changes anything, as
/// the other effects hold theirs, so no sample of anyone else's can make the memory of the
/// crossover infinite or pass on what is not a number. +36 dBFS: nothing real comes near it.
const INPUT_LIMIT: f32 = 64.0;

/// While the input is silent, a memory smaller than this is let go of: -180 dB. So a crossover
/// after a sound that ended comes to rest and does no work.
const REST: f32 = 1e-9;

/// `1 / Q` of every section: Butterworth, so two in a row are the Linkwitz-Riley filter whose low
/// and high pass add up to an all-pass.
const DAMPING: f32 = SQRT_2;

/// Output channels by input channels: `[left, right]`, each `[from left, from right]`.
type Matrix = [[f32; CHANNELS]; CHANNELS];

/// Which channels play and which are turned upside down, then the width.
fn mix_matrix(state: &UtilityState) -> Matrix {
    let picked = match state.channels {
        Channels::Stereo => [[1.0, 0.0], [0.0, 1.0]],
        Channels::Left => [[1.0, 0.0], [1.0, 0.0]],
        Channels::Right => [[0.0, 1.0], [0.0, 1.0]],
        Channels::Swap => [[0.0, 1.0], [1.0, 0.0]],
    };
    let signs = [state.invert_left, state.invert_right].map(|invert| match invert {
        true => -1.0,
        false => 1.0,
    });
    let [left, right] = picked;
    let picked = [
        left.map(|part| part * signs[0]),
        right.map(|part| part * signs[1]),
    ];
    // A channel keeps `(1 + w) / 2` of itself and gets `(1 - w) / 2` of the other: the mid stays
    // and the side is `w` times as large. At 1 that is exactly the channel.
    let (same, other) = ((1.0 + state.width) / 2.0, (1.0 - state.width) / 2.0);
    multiply([[same, other], [other, same]], picked)
}

fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| a[row][0] * b[0][column] + a[row][1] * b[1][column])
    })
}

/// The gain of each channel at the end: the gain, the pan and mute.
fn gains(state: &UtilityState) -> [f32; CHANNELS] {
    pan_gains(f64::from(level(state)), state.pan)
}

/// The factor of the gain, 0 when muted.
fn level(state: &UtilityState) -> f32 {
    match state.mute {
        true => 0.0,
        false => amplitude(state.gain_db),
    }
}

/// What the utility does with a record, once every change has arrived: how much of each input
/// channel each output channel gets, with its sign. `[left, right]`, each `[from left, from
/// right]`. Above the bass mono frequency this is the whole effect; below it bass mono takes
/// the side away as well.
///
/// The tests hold the measured sound to it, and the card draws the stereo image from it.
pub fn matrix(state: &UtilityState) -> [[f32; CHANNELS]; CHANNELS] {
    let [left, right] = mix_matrix(state);
    let [left_gain, right_gain] = gains(state);
    [
        left.map(|part| part * left_gain),
        right.map(|part| part * right_gain),
    ]
}

/// The factors of one section for one frequency.
#[derive(Copy, Clone, Default)]
struct Factors {
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Factors {
    fn new(hz: f32, sample_rate: f32) -> Self {
        // Not `clamp`: nothing may panic on the audio thread.
        let hz = hz.min(HIGHEST_PART * sample_rate);
        let g = (PI * hz / sample_rate).tan();
        let a1 = 1.0 / (1.0 + g * (g + DAMPING));
        let a2 = g * a1;
        Self { a1, a2, a3: g * a2 }
    }
}

/// The memory of one section: the two integrators.
#[derive(Copy, Clone, Default)]
struct Section {
    ic1: f32,
    ic2: f32,
}

impl Section {
    /// One frame. Returns the band pass and the low pass, from which the others are made.
    fn next(&mut self, factors: &Factors, input: f32) -> (f32, f32) {
        let Factors { a1, a2, a3 } = *factors;
        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v1, v2)
    }

    fn high_pass(&mut self, factors: &Factors, input: f32) -> f32 {
        let (band, low) = self.next(factors, input);
        input - DAMPING * band - low
    }

    /// The low pass plus the high pass less the band: a flat gain that only turns the phase.
    fn all_pass(&mut self, factors: &Factors, input: f32) -> f32 {
        let (band, _) = self.next(factors, input);
        input - 2.0 * DAMPING * band
    }

    fn settle(&mut self) {
        for memory in [&mut self.ic1, &mut self.ic2] {
            if memory.abs() < REST {
                *memory = 0.0;
            }
        }
    }

    fn is_silent(&self) -> bool {
        self.ic1 == 0.0 && self.ic2 == 0.0
    }
}

/// The crossover of bass mono: the all-pass of the mid, and the two high passes of the side.
#[derive(Copy, Clone, Default)]
struct Crossover {
    mid: Section,
    side: [Section; 2],
}

impl Crossover {
    fn next(&mut self, factors: &Factors, mid: f32, side: f32) -> (f32, f32) {
        let mid = self.mid.all_pass(factors, mid);
        let [first, second] = &mut self.side;
        let side = second.high_pass(factors, first.high_pass(factors, side));
        (mid, side)
    }

    fn sections(&mut self) -> impl Iterator<Item = &mut Section> {
        std::iter::once(&mut self.mid).chain(&mut self.side)
    }

    fn is_silent(&self) -> bool {
        self.mid.is_silent() && self.side.iter().all(Section::is_silent)
    }
}

/// A sample as the utility takes it: held to [`INPUT_LIMIT`], and silence for anything that
/// is not a number.
fn held(sample: f32) -> f32 {
    if sample.is_nan() {
        return 0.0;
    }
    sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
}

pub struct Utility {
    /// The record, with the values of the lanes that automate it.
    state: Automated<UtilityState, { PARAMETERS.len() }>,
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    /// The entries of the matrix of the channels, the signs and the width.
    mix: [[Smoothed; CHANNELS]; CHANNELS],
    /// 0 is the sound as it came, 1 is the sound through the crossover.
    bass_mono: Smoothed,
    /// The crossover frequency as `log2` of hertz, so a glide moves in octaves.
    octaves: Smoothed,
    /// The factor of the gain at the end, 0 when muted, and the pan. They glide apart, each in
    /// its own ramp: the level frame by frame, the pan law of the channels from where the pan
    /// is at the end of each run of frames.
    level: Smoothed,
    pan: Smoothed,
    /// The pan law of each channel at the end of the last run of frames, where the next starts.
    panned: [f32; CHANNELS],
    /// Whether the factors have to be worked out again although nothing glides: after a snap,
    /// and before the first block.
    stale: bool,
    factors: Factors,
    crossover: Crossover,
}

impl Utility {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<UtilityState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    /// Starts at these values, so a utility that is added or opened does not glide in.
    pub fn new(state: UtilityState) -> Self {
        let mut utility = Self {
            state: Automated::new(Self::AUTOMATION, state),
            sample_rate: 48_000.0,
            ramp_frames: 1.0,
            mix: [[0.0; CHANNELS]; CHANNELS].map(|row| row.map(Smoothed::new)),
            bass_mono: Smoothed::new(0.0),
            octaves: Smoothed::new(0.0),
            level: Smoothed::new(0.0),
            pan: Smoothed::new(0.0),
            panned: [1.0; CHANNELS],
            stale: true,
            factors: Factors::default(),
            crossover: Crossover::default(),
        };
        utility.aim(&utility.state.targets(utility.ramp_frames));
        utility.snap();
        utility
    }

    /// Sets every target from the record and its lanes, each reached in its own ramp. A choice
    /// changes only in an edit, where every number takes the edit glide, so the matrix takes
    /// the ramp of the width.
    fn aim(&mut self, targets: &UtilityTargets) {
        let (state, edit) = (*self.state, targets.edit());
        let ramp = targets.ramp(&WIDTH);
        for (row, mix) in self.mix.iter_mut().zip(mix_matrix(&state)) {
            for (part, target) in row.iter_mut().zip(mix) {
                part.set_target(target, ramp);
            }
        }
        let bass_mono = if state.bass_mono { 1.0 } else { 0.0 };
        self.bass_mono.set_target(bass_mono, edit);
        self.octaves
            .set_target(state.bass_mono_hz.log2(), targets.ramp(&BASS_MONO_HZ));
        self.level.set_target(level(&state), targets.ramp(&GAIN));
        self.pan.set_target(state.pan, targets.ramp(&PAN));
        // A number that took its value at once does not move: so nothing else says the factors
        // are old, and the next run of frames starts at the pan.
        if targets.snaps() {
            self.stale = true;
            self.panned = self.move_pan(0);
        }
    }

    /// Moves the pan `frames` along, and gives the pan law of the channels there.
    fn move_pan(&mut self, frames: usize) -> [f32; CHANNELS] {
        pan_gains(1.0, self.pan.advance(frames))
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        let [left, right] = &mut self.mix;
        left.iter_mut().chain(right).chain([
            &mut self.bass_mono,
            &mut self.octaves,
            &mut self.level,
            &mut self.pan,
        ])
    }

    /// Takes every target at once. For a utility nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        self.smoothers().for_each(Smoothed::snap);
        self.panned = self.move_pan(0);
        self.stale = true;
    }

    /// Whether every sample comes out as it went in: nothing moves, the matrix is one, the
    /// crossover is not heard and both gains are one.
    fn passes_through(&self) -> bool {
        let at =
            |smoothed: &Smoothed, value: f32| !smoothed.is_moving() && smoothed.current() == value;
        let [[left_left, left_right], [right_left, right_right]] = &self.mix;
        at(left_left, 1.0)
            && at(left_right, 0.0)
            && at(right_left, 0.0)
            && at(right_right, 1.0)
            && at(&self.bass_mono, 0.0)
            && at(&self.level, 1.0)
            && at(&self.pan, 0.0)
    }

    /// Whether the output is silent whatever comes in: muted, and the fade to it done.
    fn is_muted(&self) -> bool {
        !self.level.is_moving() && self.level.current() == 0.0
    }

    /// Whether the crossover is heard, or is on its way in or out.
    fn crossover_is_heard(&self) -> bool {
        self.bass_mono.is_moving() || self.bass_mono.current() != 0.0
    }

    /// Moves the crossover frequency `frames` along, and works out the factors for where it is
    /// when it moves.
    fn move_factors(&mut self, frames: usize) {
        let changes = self.stale || self.octaves.is_moving();
        let octaves = self.octaves.advance(frames);
        if changes {
            self.factors = Factors::new(octaves.exp2(), self.sample_rate);
            self.stale = false;
        }
    }
}

impl Processor for Utility {
    type Update = UtilityState;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::AUTOMATION.port())
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.ramp_frames = (RAMP_SECONDS * self.sample_rate).max(1.0);
        self.stale = true;
    }

    fn update(&mut self, update: &mut UtilityState) {
        let targets = self.state.set_record(update, self.ramp_frames);
        self.aim(&targets);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        let crossover = self.crossover_is_heard();
        if !crossover {
            // A frequency set while bass mono is off is where it starts when it is turned on.
            self.octaves.snap();
        }
        if self.passes_through() {
            let channels = [(left_in, left_out), (right_in, right_out)];
            for (input, output) in channels {
                output
                    .iter_mut()
                    .zip(input)
                    .for_each(|(out, sample)| *out = *sample);
            }
            return;
        }
        let silent_input = left_in.iter().chain(right_in).all(|sample| *sample == 0.0);
        if self.is_muted() || (silent_input && self.crossover.is_silent()) {
            // Nothing sounds, or nothing may: the output is already silent, and no glide can be
            // heard. The crossover starts again from rest.
            self.snap();
            self.crossover = Crossover::default();
            return;
        }
        let chunks = left_in
            .chunks(FACTOR_FRAMES)
            .zip(right_in.chunks(FACTOR_FRAMES))
            .zip(left_out.chunks_mut(FACTOR_FRAMES))
            .zip(right_out.chunks_mut(FACTOR_FRAMES));
        for (((left_in, right_in), left_out), right_out) in chunks {
            let length = left_in.len();
            if crossover {
                self.move_factors(length);
            }
            let before = self.panned;
            self.panned = self.move_pan(length);
            let steps =
                [0, 1].map(|channel| (self.panned[channel] - before[channel]) / length as f32);
            let frames = left_in
                .iter()
                .zip(right_in)
                .zip(left_out.iter_mut())
                .zip(right_out.iter_mut());
            for (index, (((left_in, right_in), left_out), right_out)) in frames.enumerate() {
                let [left, right] = self
                    .mix
                    .each_mut()
                    .map(|row| row.each_mut().map(|part| part.advance(1)));
                let (left_in, right_in) = (held(*left_in), held(*right_in));
                let mut sound = [
                    left[0] * left_in + left[1] * right_in,
                    right[0] * left_in + right[1] * right_in,
                ];
                if crossover {
                    let heard = self.bass_mono.advance(1);
                    let mid = (sound[0] + sound[1]) * 0.5;
                    let side = (sound[0] - sound[1]) * 0.5;
                    let through = self.crossover.next(&self.factors, mid, side);
                    let (mid_through, side_through) = through;
                    let mid = mid + heard * (mid_through - mid);
                    let side = side + heard * (side_through - side);
                    sound = [mid + side, mid - side];
                }
                let (along, level) = ((index + 1) as f32, self.level.advance(1));
                let [left_gain, right_gain] =
                    [0, 1].map(|channel| level * (before[channel] + steps[channel] * along));
                *left_out = sound[0] * left_gain;
                *right_out = sound[1] * right_gain;
            }
        }
        if crossover && !self.crossover_is_heard() {
            // Faded out: the next fade in starts from rest, at the frequency of the record.
            self.crossover = Crossover::default();
            self.octaves.snap();
            self.stale = true;
        }
        if silent_input {
            self.crossover.sections().for_each(Section::settle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At the defaults every stage is exactly one, so the processor copies its input.
    #[test]
    fn the_defaults_are_exactly_one() {
        let state = UtilityState::default();
        assert_eq!(mix_matrix(&state), [[1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(gains(&state), [1.0, 1.0]);
        assert_eq!(matrix(&state), [[1.0, 0.0], [0.0, 1.0]]);
        assert!(Utility::new(state).passes_through());
    }

    #[test]
    fn width_zero_is_the_mid_in_both_and_two_doubles_the_side() {
        let mono = UtilityState {
            width: 0.0,
            ..UtilityState::default()
        };
        assert_eq!(matrix(&mono), [[0.5, 0.5], [0.5, 0.5]]);
        let wide = UtilityState {
            width: 2.0,
            ..UtilityState::default()
        };
        assert_eq!(matrix(&wide), [[1.5, -0.5], [-0.5, 1.5]]);
    }

    #[test]
    fn channels_pick_and_invert_turns_upside_down() {
        let with = |channels, invert_left, invert_right| {
            matrix(&UtilityState {
                channels,
                invert_left,
                invert_right,
                ..UtilityState::default()
            })
        };
        assert_eq!(with(Channels::Left, false, false), [[1.0, 0.0], [1.0, 0.0]]);
        assert_eq!(
            with(Channels::Right, false, false),
            [[0.0, 1.0], [0.0, 1.0]]
        );
        assert_eq!(with(Channels::Swap, false, false), [[0.0, 1.0], [1.0, 0.0]]);
        assert_eq!(
            with(Channels::Stereo, true, false),
            [[-1.0, 0.0], [0.0, 1.0]]
        );
        assert_eq!(with(Channels::Swap, false, true), [[0.0, 1.0], [-1.0, 0.0]]);
    }

    #[test]
    fn mute_is_exactly_silent_and_pan_follows_the_law_of_a_track() {
        let muted = UtilityState {
            mute: true,
            gain_db: 12.0,
            ..UtilityState::default()
        };
        assert_eq!(matrix(&muted), [[0.0; 2]; 2]);
        let left = UtilityState {
            pan: -1.0,
            ..UtilityState::default()
        };
        assert_eq!(matrix(&left), [[SQRT_2, 0.0], [0.0, 0.0]]);
    }
}
