//! One voice: the sound of one note. Two wavetable oscillators with their unison copies and
//! their effects, a sub, two filters, the amp envelope, two more envelopes and two LFOs, and
//! the routes of the matrix.
//!
//! A voice works its modulation out once per block and lets what the routes move glide across
//! the block from where the last block left it, so nothing steps. It keeps a mono path while
//! nothing spreads it across the stereo field, which halves the work of its filters.

use sound_core::{
    EnvelopeState, Lfo, MAX_BLOCK, Oversampler, SvfFactors, SvfSection, pan_gains, soft_clip,
};
use sound_notes::{Velocity, frequency_hz};

use crate::dsp::{fold, sine};
use crate::matrix::{Destination, KEY_SEMITONES, Modulation, Source, Sources};
use crate::state::Effect;
use crate::synth::{Block, FACTOR_FRAMES, FilterBlock, MAX_UNISON, OscillatorBlock, Ramp};
use crate::tables::{LevelView, Wavetable};

/// Above this a cycle is about two frames: a note this high plays at this pitch.
const HIGHEST_PHASE_STEP: f32 = 0.45;

/// How far the unison copies spread at amount 1, either way.
const UNISON_CENTS: f32 = 50.0;

/// How far FM moves where the table is read at amount 1, in cycles either way.
const FM_DEPTH: f32 = 1.0;
/// How many times faster than the rest warp reads the start of a cycle at amount 1, less one.
const WARP_SQUEEZE: f32 = 7.0;
/// Sync reads up to 2 to the power of this many times faster.
const SYNC_OCTAVES: f32 = 4.0;
/// Fold drives up to this many times, less one.
const FOLD_DRIVE: f32 = 7.0;

/// Where each unison copy starts its cycle, in cycles: evenly apart on the golden ratio, so no
/// two copies start together, and the same on every render. The first starts at 0.
const START_PHASES: [f32; MAX_UNISON] = {
    let mut phases = [0.0; MAX_UNISON];
    let mut index = 0;
    while index < MAX_UNISON {
        let turns = index as f64 * 0.618_033_988_749_894_9;
        phases[index] = (turns - (turns as u64) as f64) as f32;
        index += 1;
    }
    phases
};

/// What a voice reads from the synth to start a note.
pub(crate) struct NoteStart {
    /// The LFOs of the note: new ones that start their cycle, or copies of the free-running
    /// ones.
    pub lfos: [Lfo; 2],
    /// The random value of the note, from -1 to 1.
    pub random: f32,
}

/// One oscillator of one voice.
#[derive(Clone)]
struct OscillatorVoice {
    /// The phase of each unison copy, in cycles from 0 to 1.
    phases: [f32; MAX_UNISON],
    /// Left and right, for an effect at four times the rate.
    oversamplers: [Oversampler; 2],
}

impl OscillatorVoice {
    const START: Self = Self {
        phases: START_PHASES,
        oversamplers: [Oversampler::new(); 2],
    };
}

/// One filter of one voice.
#[derive(Clone)]
struct FilterVoice {
    /// Left and right, each two sections.
    sections: [[SvfSection; 2]; 2],
    /// Left and right, for the drive at four times the rate.
    oversamplers: [Oversampler; 2],
    /// The level of the low and high pass of the first section, which moves with the resonance.
    level: f32,
    /// The cutoff in octaves, the resonance and the slope the factors are for, so they are
    /// worked out again only when one moves.
    factors_for: [f32; 3],
    factors: [SvfFactors; 2],
    factors_level: f32,
    /// The oversamplers hold sound, so they are cleared once when the drive goes, and not
    /// every block: they are large.
    driving: bool,
}

impl FilterVoice {
    fn start() -> Self {
        Self {
            sections: [[SvfSection::default(); 2]; 2],
            oversamplers: [Oversampler::new(); 2],
            level: 1.0,
            factors_for: [f32::NAN; 3],
            factors: [SvfFactors::default(); 2],
            factors_level: 1.0,
            driving: false,
        }
    }
}

/// Where the values the routes move stand at the end of a block: the start of the next
/// block's glide.
#[derive(Copy, Clone, Default)]
struct Targets {
    position: [f32; 2],
    effect: [f32; 2],
    sub: f32,
    /// In octaves, the base 2 logarithm of the frequency.
    cutoff: [f32; 2],
    resonance: [f32; 2],
    /// The amp level of the routes, and the pan, as the gain of each side.
    output: [f32; 2],
    /// The gains of each unison copy of each oscillator, left and right: its pan, its part of
    /// the unison level and the gain of its oscillator.
    unison: [[[f32; 2]; MAX_UNISON]; 2],
    /// How far the unison copies spread, from 0 to 1. Their pitches step to it once per block.
    unison_amount: f32,
}

#[derive(Clone)]
pub(crate) struct Voice {
    /// In semitones, on its way to the key while it glides.
    pitch: f32,
    velocity: f32,
    random: f32,
    amp: EnvelopeState,
    /// Env 2 and env 3.
    envelopes: [EnvelopeState; 2],
    lfos: [Lfo; 2],
    /// The factor on the rate of each LFO from the routes of the last block.
    lfo_rates: [f32; 2],
    oscillators: [OscillatorVoice; 2],
    sub_phase: f32,
    filters: [FilterVoice; 2],
    last: Targets,
    /// Just started from silence: the first block starts where the routes put it.
    fresh: bool,
    /// Its two sides have differed since it started, so it renders both until it ends: their
    /// filters ring on apart.
    stereo: bool,
}

/// A voice's work for one block: a channel each, only the left one while it is mono.
type Buffer = [[f32; MAX_BLOCK]; 2];

impl sound_notes::Voice for Voice {
    type Context = NoteStart;

    fn is_idle(&self) -> bool {
        self.amp.is_idle()
    }

    fn loudness(&self) -> f32 {
        self.amp.level as f32
    }

    fn release(&mut self) {
        self.amp.release();
        self.envelopes.iter_mut().for_each(EnvelopeState::release);
    }

    fn start(&mut self, pitch: f32, velocity: Velocity, context: &NoteStart) {
        if self.is_idle() {
            *self = Self::idle();
        }
        // A voice taken from another note keeps its phases, its filters and its levels, and
        // every envelope starts from where it is, so the takeover does not click.
        self.amp.start();
        self.envelopes.iter_mut().for_each(EnvelopeState::start);
        self.lfos = context.lfos;
        self.lfo_rates = [1.0; 2];
        self.pitch = pitch;
        self.velocity = f32::from(velocity.value()) / 127.0;
        self.random = context.random;
    }

    fn set_pitch(&mut self, pitch: f32, _: &NoteStart) {
        self.pitch = pitch;
    }
}

impl Voice {
    pub fn idle() -> Self {
        Self {
            pitch: 60.0,
            velocity: 0.0,
            random: 0.0,
            amp: EnvelopeState::IDLE,
            envelopes: [EnvelopeState::IDLE; 2],
            lfos: [Lfo::default(); 2],
            lfo_rates: [1.0; 2],
            oscillators: [OscillatorVoice::START; 2],
            sub_phase: 0.0,
            filters: [FilterVoice::start(), FilterVoice::start()],
            last: Targets::default(),
            fresh: true,
            stereo: false,
        }
    }

    /// Clears what an oscillator at four times the rate remembers, for a new effect.
    pub fn reset_oscillator(&mut self, index: usize) {
        self.oscillators[index].oversamplers = [Oversampler::new(); 2];
    }

    /// Adds this voice to `output`, left and right, over the frames of `block`.
    pub fn render(&mut self, output: [&mut [f32]; 2], block: &Block<'_>) {
        let frames = block.frames;
        let modulation = self.modulate(block);
        let targets = self.targets(block, &modulation);
        if std::mem::take(&mut self.fresh) {
            self.last = targets;
        }
        let last = self.last;
        self.stereo |= [last.unison, targets.unison]
            .iter()
            .flatten()
            .flatten()
            .any(|[left, right]| left != right);
        let channels = if self.stereo { 2 } else { 1 };

        let mut oscillators = [[[0.0; MAX_BLOCK]; 2]; 2];
        let note_step = frequency_hz(self.pitch) * block.pitch_ratio / block.sample_rate;
        for (index, buffer) in oscillators.iter_mut().enumerate() {
            let pitch = modulation.get(PITCH[index]);
            let ramps = Ramps {
                position: Ramp::new(last.position[index], targets.position[index]),
                effect: Ramp::new(last.effect[index], targets.effect[index]),
                gains: [last.unison[index], targets.unison[index]],
            };
            let oscillator = &block.oscillators[index];
            if ramps.is_silent() {
                continue;
            }
            let steps = unison_steps(oscillator, note_step, pitch, targets.unison_amount, block);
            self.oscillators[index].render(oscillator, &steps, &ramps, block, channels, buffer);
        }
        let mut sub = [0.0; MAX_BLOCK];
        let sub_gain = Ramp::new(last.sub, targets.sub);
        if sub_gain.from > 0.0 || sub_gain.to > 0.0 {
            let step = (note_step * block.sub_ratio).min(HIGHEST_PHASE_STEP);
            for (frame, sample) in sub[..frames].iter_mut().enumerate() {
                self.sub_phase += step;
                if self.sub_phase >= 1.0 {
                    self.sub_phase -= 1.0;
                }
                *sample = sine(self.sub_phase) * sub_gain.at(frame, frames);
            }
        }

        let mixed = self.filter(block, &oscillators, &sub, [&last, &targets], channels);

        let [left, right] = output;
        let right_channel = channels - 1;
        let envelope = &block.envelopes[0];
        let [output_from, output_to] = [last.output, targets.output];
        for frame in 0..frames {
            // An idle voice ended on the frame before: nothing of it is added any more.
            if self.amp.is_idle() {
                break;
            }
            let level = self.amp.next(envelope) as f32;
            let at = (frame + 1) as f32 / frames as f32;
            let side = |side: usize| output_from[side] + (output_to[side] - output_from[side]) * at;
            left[frame] += mixed[0][frame] * level * side(0);
            right[frame] += mixed[right_channel][frame] * level * side(1);
        }
        self.last = targets;
    }

    /// Moves the modulation sources to the end of the block and works out the routes.
    fn modulate(&mut self, block: &Block<'_>) -> Modulation {
        let frames = block.frames;
        let mut levels = [0.0; 2];
        for ((state, envelope), level) in self
            .envelopes
            .iter_mut()
            .zip(&block.envelopes[1..])
            .zip(&mut levels)
        {
            for _ in 0..frames {
                state.next(envelope);
            }
            *level = state.level as f32;
        }
        let mut lfos = [0.0; 2];
        for (index, lfo) in self.lfos.iter_mut().enumerate() {
            let hz = block.lfo_hz[index] * self.lfo_rates[index];
            lfo.advance(frames, hz, block.sample_rate);
            lfos[index] = lfo.value(block.lfo_shapes[index], 0.0);
        }
        let mut sources: Sources = [0.0; Source::ALL.len()];
        for (value, source) in sources.iter_mut().zip(Source::ALL) {
            *value = match source {
                Source::Env2 => levels[0],
                Source::Env3 => levels[1],
                Source::Lfo1 => lfos[0],
                Source::Lfo2 => lfos[1],
                Source::Velocity => self.velocity,
                Source::Key => (self.pitch - 60.0) / KEY_SEMITONES,
                Source::ModWheel => block.mod_wheel,
                Source::Pressure => block.pressure,
                Source::Random => self.random,
            };
        }
        let modulation = Modulation::new(block.routes, &sources);
        self.lfo_rates = [Destination::Lfo1Rate, Destination::Lfo2Rate]
            .map(|destination| modulation.get(destination).exp2());
        modulation
    }

    /// Where the routes put everything they move, at the end of the block.
    fn targets(&self, block: &Block<'_>, modulation: &Modulation) -> Targets {
        let unit = |value: f32| value.clamp(0.0, 1.0);
        let routed = |base: f32, destinations: [Destination; 2], index: usize| {
            base + modulation.get(destinations[index])
        };
        let unison_amount = unit(block.unison_amount + modulation.get(Destination::UnisonAmount));
        let unison = [0, 1].map(|index| {
            let osc = &block.oscillators[index];
            let gain = unit(routed(osc.gain, GAIN, index)) * osc.level;
            std::array::from_fn(|copy| {
                let level = gain * block.unison_levels[copy];
                if level == 0.0 {
                    return [0.0; 2];
                }
                let spread = unison_amount * spread(copy, block.unison_voices);
                let pan = (osc.pan + spread).clamp(-1.0, 1.0);
                pan_gains(f64::from(level), pan)
            })
        });
        let oscillators = &block.oscillators;
        let filters = &block.filters;
        let pan = modulation.get(Destination::Pan).clamp(-1.0, 1.0);
        Targets {
            position: [0, 1]
                .map(|index| unit(routed(oscillators[index].position, POSITION, index))),
            effect: [0, 1]
                .map(|index| unit(routed(oscillators[index].effect_amount, EFFECT, index))),
            sub: unit(block.sub_gain + modulation.get(Destination::SubGain)),
            cutoff: [0, 1].map(|index| routed(filters[index].cutoff, CUTOFF, index)),
            resonance: [0, 1].map(|index| unit(routed(filters[index].resonance, RESONANCE, index))),
            output: pan_gains(f64::from(modulation.amp()), pan),
            unison,
            unison_amount,
        }
    }

    /// The oscillators and the sub through the filters, as the routing weighs them.
    fn filter(
        &mut self,
        block: &Block<'_>,
        oscillators: &[Buffer; 2],
        sub: &[f32; MAX_BLOCK],
        [last, targets]: [&Targets; 2],
        channels: usize,
    ) -> Buffer {
        let frames = block.frames;
        let weights = &block.routing;
        let [first, second] = &mut self.filters;
        let mut one = [[0.0; MAX_BLOCK]; 2];
        let mut two = [[0.0; MAX_BLOCK]; 2];
        for channel in 0..channels {
            let sources = [&oscillators[0][channel], &oscillators[1][channel], sub];
            for (source, weight) in sources.iter().zip(&weights[0..3]) {
                add(&mut one[channel], source, weight, frames);
            }
            for (source, weight) in sources.iter().zip(&weights[3..6]) {
                add(&mut two[channel], source, weight, frames);
            }
        }
        first.render(
            &block.filters[0],
            [last, targets],
            0,
            block,
            channels,
            &mut one,
        );
        for channel in 0..channels {
            add(&mut two[channel], &one[channel], &weights[6], frames);
        }
        second.render(
            &block.filters[1],
            [last, targets],
            1,
            block,
            channels,
            &mut two,
        );
        let mut mixed = [[0.0; MAX_BLOCK]; 2];
        for channel in 0..channels {
            add(&mut mixed[channel], &one[channel], &weights[7], frames);
            add(&mut mixed[channel], &two[channel], &weights[8], frames);
        }
        mixed
    }
}

/// Adds `source` to `sum` at a weight that moves over the block, and nothing while it is 0.
fn add(sum: &mut [f32; MAX_BLOCK], source: &[f32; MAX_BLOCK], weight: &Ramp, frames: usize) {
    if weight.is_zero() {
        return;
    }
    let step = (weight.to - weight.from) / frames as f32;
    let pairs = sum[..frames].iter_mut().zip(&source[..frames]);
    for (frame, (sum, source)) in pairs.enumerate() {
        *sum += (weight.from + step * (frame + 1) as f32) * source;
    }
}

const POSITION: [Destination; 2] = [Destination::Osc1Position, Destination::Osc2Position];
const EFFECT: [Destination; 2] = [Destination::Osc1Effect, Destination::Osc2Effect];
const PITCH: [Destination; 2] = [Destination::Osc1Pitch, Destination::Osc2Pitch];
const GAIN: [Destination; 2] = [Destination::Osc1Gain, Destination::Osc2Gain];
const CUTOFF: [Destination; 2] = [Destination::Filter1Cutoff, Destination::Filter2Cutoff];
const RESONANCE: [Destination; 2] = [Destination::Filter1Resonance, Destination::Filter2Resonance];

/// Where unison copy `copy` of `voices` sits in the spread, from -1 to 1. The only copy of one
/// sits in the middle.
fn spread(copy: usize, voices: usize) -> f32 {
    if voices < 2 {
        return 0.0;
    }
    2.0 * copy as f32 / (voices - 1) as f32 - 1.0
}

/// The phase step of each unison copy of an oscillator, in cycles per frame.
fn unison_steps(
    oscillator: &OscillatorBlock<'_>,
    note_step: f32,
    pitch: f32,
    unison_amount: f32,
    block: &Block<'_>,
) -> [f32; MAX_UNISON] {
    let semitones = oscillator.transpose + pitch;
    let step = note_step * (semitones / 12.0).exp2();
    let cents = UNISON_CENTS * unison_amount;
    std::array::from_fn(|copy| {
        let detune = cents * spread(copy, block.unison_voices);
        let step = if detune == 0.0 {
            step
        } else {
            step * (detune / 1_200.0).exp2()
        };
        step.min(HIGHEST_PHASE_STEP)
    })
}

/// The glides of one oscillator over a block.
struct Ramps {
    position: Ramp,
    effect: Ramp,
    /// The gains of each unison copy, left and right, at the start and at the end.
    gains: [[[f32; 2]; MAX_UNISON]; 2],
}

impl Ramps {
    fn copy_is_silent(&self, copy: usize) -> bool {
        self.gains.iter().all(|gains| gains[copy] == [0.0; 2])
    }

    fn is_silent(&self) -> bool {
        self.gains
            .iter()
            .flatten()
            .flatten()
            .all(|gain| *gain == 0.0)
    }
}

/// Which two frames of a level each frame of a block reads, and how much of the second.
struct Frames {
    first: [usize; MAX_BLOCK],
    second: [usize; MAX_BLOCK],
    mix: [f32; MAX_BLOCK],
}

impl Frames {
    fn new(level: &LevelView<'_>, position: &Ramp, frames: usize) -> Self {
        let last = level.frames - 1;
        let stride = level.length + 1;
        let at = |position: f32| {
            let at = position * last as f32;
            let first = (at as usize).min(last);
            (
                first * stride,
                (first + 1).min(last) * stride,
                at - first as f32,
            )
        };
        if position.from == position.to {
            // It stands still, which is most of the time: every frame reads the same.
            let (first, second, mix) = at(position.to);
            return Self {
                first: [first; MAX_BLOCK],
                second: [second; MAX_BLOCK],
                mix: [mix; MAX_BLOCK],
            };
        }
        let mut read = Self {
            first: [0; MAX_BLOCK],
            second: [0; MAX_BLOCK],
            mix: [0.0; MAX_BLOCK],
        };
        for frame in 0..frames {
            (read.first[frame], read.second[frame], read.mix[frame]) =
                at(position.at(frame, frames));
        }
        read
    }
}

/// What rounds off a jump at the start of a cycle, times half the jump, for a phase that moves
/// by `step`: from 0 far from the jump to 1 just before it and -1 just after it, so both
/// samples next to the jump meet in the middle.
#[inline]
fn step_rounding(phase: f32, step: f32) -> f32 {
    if phase < step {
        let t = phase / step;
        t + t - t * t - 1.0
    } else if phase > 1.0 - step {
        let t = (phase - 1.0) / step;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Reads a level at a phase from 0 to 1, between two of its frames: in a straight line
/// between the two samples around the phase in each, and `mix` of the second. A frame is
/// `length + 1` samples, the last a copy of the first.
#[inline]
fn read(first: &[f32], second: &[f32], length: usize, mix: f32, phase: f32) -> f32 {
    let at = phase * length as f32;
    let index = (at as usize).min(length - 1);
    let between = at - index as f32;
    let (a, b) = (first[index], first[index + 1]);
    let (c, d) = (second[index], second[index + 1]);
    let one = a + (b - a) * between;
    let two = c + (d - c) * between;
    one + (two - one) * mix
}

impl OscillatorVoice {
    /// Renders every unison copy into `output`, left and, when `channels` is 2, right.
    fn render(
        &mut self,
        oscillator: &OscillatorBlock<'_>,
        steps: &[f32; MAX_UNISON],
        ramps: &Ramps,
        block: &Block<'_>,
        channels: usize,
        output: &mut Buffer,
    ) {
        let frames = block.frames;
        let most = ramps.effect.from.max(ramps.effect.to);
        let fastest = (0..MAX_UNISON)
            .filter(|copy| !ramps.copy_is_silent(*copy))
            .fold(0.0_f32, |fastest, copy| fastest.max(steps[copy]));
        // How much faster than the phase the effect reads at its fastest, and the setting of
        // the effect at each end of the block.
        let (speed, setting) = match oscillator.effect {
            Effect::None => (1.0, Ramp::new(0.0, 0.0)),
            Effect::Fm => (
                1.0 + std::f32::consts::TAU * FM_DEPTH * most,
                ramps.effect.map(|amount| amount * FM_DEPTH),
            ),
            Effect::Warp => (
                1.0 + WARP_SQUEEZE * most,
                ramps.effect.map(|amount| amount * WARP_SQUEEZE),
            ),
            Effect::Sync => {
                let ratio = |amount: f32| (amount * SYNC_OCTAVES).exp2();
                (ratio(most), ramps.effect.map(ratio))
            }
            // A fold makes the steep parts of a table steeper by up to its drive: the table
            // it folds leaves out what would then pass half the sample rate.
            Effect::Fold => {
                let drive = |amount: f32| 1.0 + amount * FOLD_DRIVE;
                (drive(most), ramps.effect.map(drive))
            }
        };
        // A new level takes over at the start of a block. Only harmonics between a quarter and
        // a half of the sample rate come or go with it, so it is not crossfaded.
        let level = oscillator
            .table
            .level(Wavetable::level_for(fastest * speed));
        let read_frames = Frames::new(&level, &ramps.position, frames);
        let copies = Copies {
            phases: &mut self.phases,
            steps,
            ramps,
            level: &level,
            frames: &read_frames,
            setting,
            channels,
            count: frames,
        };
        let [left, right] = output;
        let plain = |sample: f32, _| sample;
        let no_restart = |_| None;
        if oscillator.effect == Effect::None {
            copies.render::<1>([left, right], |phase, _| phase, plain, no_restart);
            return;
        }
        let mut four = [[0.0; 4 * MAX_BLOCK]; 2];
        let [four_left, four_right] = &mut four;
        let target = [&mut four_left[..], &mut four_right[..]];
        match oscillator.effect {
            Effect::Fm => {
                let fm = |phase: f32, depth: f32| {
                    let read = phase + depth * sine(phase);
                    read - read.floor()
                };
                copies.render::<4>(target, fm, plain, no_restart);
            }
            Effect::Warp => {
                let warp =
                    |phase: f32, squeeze: f32| phase * (1.0 + squeeze) / (1.0 + squeeze * phase);
                copies.render::<4>(target, warp, plain, no_restart);
            }
            Effect::Sync => {
                let sync = |phase: f32, ratio: f32| {
                    let read = phase * ratio;
                    read - read.floor()
                };
                // Where the last read of a cycle is, just before it starts again.
                let restart = |ratio: f32| Some(ratio - ratio.floor());
                copies.render::<4>(target, sync, plain, restart);
            }
            Effect::Fold => {
                let shape = |sample: f32, drive: f32| fold(sample * drive);
                copies.render::<4>(target, |phase, _| phase, shape, no_restart);
            }
            Effect::None => {}
        }
        for (channel, output) in [left, right].into_iter().enumerate().take(channels) {
            let four = &four[channel][..4 * frames];
            let oversampler = &mut self.oversamplers[channel];
            oversampler.down(block.oversampling, four, &mut output[..frames]);
        }
        if channels == 1 {
            // The right side goes on from where the left is, should the voice spread.
            self.oversamplers[1] = self.oversamplers[0];
        }
    }
}

/// The unison copies of an oscillator over a block.
struct Copies<'a> {
    phases: &'a mut [f32; MAX_UNISON],
    steps: &'a [f32; MAX_UNISON],
    ramps: &'a Ramps,
    level: &'a LevelView<'a>,
    frames: &'a Frames,
    /// The setting of the effect, such as the ratio of sync, at each end of the block.
    setting: Ramp,
    channels: usize,
    count: usize,
}

impl Copies<'_> {
    /// Adds every copy that sounds to `output` at `FACTOR` samples per frame. `warp` bends the
    /// phase it reads at and `shape` bends what it reads, each with the setting of the effect.
    /// For an effect that jumps back to the start of the frame at every cycle, `restart` gives
    /// the phase read last before the jump, and the jump is rounded off over two samples
    /// (PolyBLEP), which takes away most of what it would fold back.
    #[inline]
    fn render<const FACTOR: usize>(
        self,
        [left, right]: [&mut [f32]; 2],
        warp: impl Fn(f32, f32) -> f32,
        shape: impl Fn(f32, f32) -> f32,
        restart: impl Fn(f32) -> Option<f32>,
    ) {
        let per_frame = 1.0 / self.count as f32;
        for copy in 0..MAX_UNISON {
            if self.ramps.copy_is_silent(copy) {
                continue;
            }
            let [from, to] = [self.ramps.gains[0][copy], self.ramps.gains[1][copy]];
            let step = self.steps[copy] / FACTOR as f32;
            let mut phase = self.phases[copy];
            for frame in 0..self.count {
                let at = (frame + 1) as f32 * per_frame;
                let setting = self.setting.from + (self.setting.to - self.setting.from) * at;
                let left_gain = from[0] + (to[0] - from[0]) * at;
                let right_gain = from[1] + (to[1] - from[1]) * at;
                let length = self.level.length;
                let frame_at = |start: usize| &self.level.samples[start..start + length + 1];
                let first = frame_at(self.frames.first[frame]);
                let second = frame_at(self.frames.second[frame]);
                let mix = self.frames.mix[frame];
                for sample in 0..FACTOR {
                    phase += step;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                    let read_at = |phase| read(first, second, length, mix, phase);
                    let mut value = shape(read_at(warp(phase, setting)), setting);
                    if let Some(end) = restart(setting) {
                        let rounding = step_rounding(phase, step);
                        if rounding != 0.0 {
                            value += 0.5 * (read_at(0.0) - read_at(end)) * rounding;
                        }
                    }
                    let index = frame * FACTOR + sample;
                    left[index] += value * left_gain;
                    if self.channels == 2 {
                        right[index] += value * right_gain;
                    }
                }
            }
            self.phases[copy] = phase;
        }
    }
}

impl FilterVoice {
    /// Filters `buffer` in place, left and, when `channels` is 2, right. Off, it lets the sound
    /// through as it is.
    fn render(
        &mut self,
        filter: &FilterBlock,
        [last, targets]: [&Targets; 2],
        index: usize,
        block: &Block<'_>,
        channels: usize,
        buffer: &mut Buffer,
    ) {
        let frames = block.frames;
        if filter.oversampled.is_zero() {
            if std::mem::take(&mut self.driving) {
                self.oversamplers = [Oversampler::new(); 2];
            }
        } else {
            self.driving = true;
            // Without drive of its own it only waits, as long as the drive of the other filter.
            let clean = filter.saturated.is_zero();
            let mut four = [0.0; 4 * MAX_BLOCK];
            let four = &mut four[..4 * frames];
            for channel in 0..channels {
                let oversampler = &mut self.oversamplers[channel];
                let samples = &mut buffer[channel][..frames];
                oversampler.up(block.oversampling, samples, four);
                if !clean {
                    for (frame, chunk) in four.chunks_exact_mut(4).enumerate() {
                        let drive = filter.drive.at(frame, frames);
                        let part = filter.saturated.at(frame, frames);
                        for sample in chunk.iter_mut() {
                            *sample += part * (soft_clip(*sample * drive) - *sample);
                        }
                    }
                }
                let mut late = [0.0; MAX_BLOCK];
                oversampler.down(block.oversampling, four, &mut late[..frames]);
                // Into the late sound over a fade, so a drive turned on does not click.
                for (frame, sample) in samples.iter_mut().enumerate() {
                    let part = filter.oversampled.at(frame, frames);
                    *sample += part * (late[frame] - *sample);
                }
            }
            if channels == 1 {
                self.oversamplers[1] = self.oversamplers[0];
            }
        }
        if filter.wet.is_zero() {
            self.sections = [[SvfSection::default(); 2]; 2];
            return;
        }

        // While it is all the way on, which is nearly always, it keeps nothing of the dry sound.
        let all_wet = filter.wet.from == 1.0 && filter.wet.to == 1.0;
        let mut dry = [[0.0; MAX_BLOCK]; 2];
        if !all_wet {
            dry[..channels].copy_from_slice(&buffer[..channels]);
        }
        let cutoff = Ramp::new(last.cutoff[index], targets.cutoff[index]);
        let resonance = Ramp::new(last.resonance[index], targets.resonance[index]);
        let second_runs = !filter.slope.is_zero();
        let mut start = 0;
        while start < frames {
            let end = (start + FACTOR_FRAMES).min(frames);
            let at = |ramp: &Ramp| ramp.at(end - 1, frames);
            let inputs = [at(&cutoff), at(&resonance), at(&filter.slope)];
            if inputs != self.factors_for {
                let [cutoff, resonance, slope] = inputs;
                (self.factors, self.factors_level) =
                    SvfFactors::sections(cutoff.exp2(), resonance, slope, block.sample_rate);
                self.factors_for = inputs;
            }
            let (factors, level_to, level_from) = (self.factors, self.factors_level, self.level);
            for (channel, sections) in self.sections.iter_mut().enumerate().take(channels) {
                let [first, second] = sections;
                for frame in start..end {
                    let part = (frame + 1 - start) as f32 / (end - start) as f32;
                    let level = level_from + (level_to - level_from) * part;
                    let taps = filter.taps.map(|tap| tap.at(frame, frames));
                    let input = buffer[channel][frame];
                    let one = first.next(&factors[0], taps, level, input);
                    let filtered = if second_runs {
                        let two = second.next(&factors[1], taps, 1.0, one);
                        one + filter.slope.at(frame, frames) * (two - one)
                    } else {
                        one
                    };
                    buffer[channel][frame] = filtered;
                }
            }
            self.level = level_to;
            start = end;
        }
        if !second_runs {
            self.sections
                .iter_mut()
                .for_each(|sections| sections[1] = SvfSection::default());
        }
        if channels == 1 {
            self.sections[1] = self.sections[0];
        }
        if all_wet {
            return;
        }
        for channel in 0..channels {
            for frame in 0..frames {
                let wet = filter.wet.at(frame, frames);
                let dry = dry[channel][frame];
                buffer[channel][frame] = dry + wet * (buffer[channel][frame] - dry);
            }
        }
    }
}
