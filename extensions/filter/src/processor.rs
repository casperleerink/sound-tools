//! The filter processor: a drive stage, two state variable filter sections, and the mix.
//!
//! The two sections are the state variable filter of the SDK, `SvfSection`, which says how
//! they sound. Both always run and the output glides from the first to the second, and the type
//! is a weight for each output of a section, so a change of type or slope is a glide and not a
//! switch. Nothing that a composer or an agent changes jumps.
//!
//! A drive or the warm character bends the sound, so while one is on, the drive and the
//! sections run at four times the sample rate between the two stages of an [`Oversampler`]. A
//! clean filter without drive runs at the sample rate, as it always has. Either way the sound
//! comes out [`LATENCY`] frames late, so the latency the filter reports never moves: a drive
//! automated across 0 glides from one way to the other and stays in time.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, FilterCharacter, Lfo, LfoShape,
    Oversampler, OversamplerDelay, OversamplingFilters, Ports, PrepareConfig, ProcessContext,
    Processor, Smoothed, SvfFactors, SvfSection, Targets, all_zero, amplitude, held, soft_clip,
    svf_response,
};

use crate::{CUTOFF, DRIVE, FilterState, LFO_DEPTH, MIX, PARAMETERS, RESONANCE};

/// Every number of the filter can be automated.
type FilterTargets = Targets<FilterState, { PARAMETERS.len() }>;

/// How many frames the filter delays the sound, at every sample rate: 1.3 ms at 48 kHz.
pub const LATENCY: u32 = Oversampler::DELAY_FRAMES as u32;

/// How long a change takes to arrive. A jump would click, or step in the sound.
const RAMP_SECONDS: f32 = 0.02;

/// While something moves, the factors are worked out again this often. Four times per block
/// of the engine: a sweep has no steps anyone can hear, and a `tan` per frame is not needed.
const FACTOR_FRAMES: usize = 16;

/// Frames of silent input and silent sections after which the delays and the oversampling
/// hold only zeros: more than the delay and the memory of every stage.
const QUIET_FRAMES: usize = 2 * Oversampler::DELAY_FRAMES + 32;

/// The gain at `hz` of a filter with this record, as a factor, once every change has arrived:
/// what a quiet steady sine comes out with, at the cutoff the record says and with its mix.
/// Drive is left out, because what it does depends on the level; a quiet sound gets its gain.
///
/// This is the exact response of the processor, not a drawing of one: see [`svf_response`].
/// The card draws it and the tests hold the measured sound to it.
pub fn response(state: &FilterState, hz: f32, sample_rate: f32) -> f32 {
    let filtered = svf_response(
        state.kind,
        state.slope,
        state.cutoff_hz,
        state.resonance,
        hz,
        sample_rate,
    );
    let mix = f64::from(state.mix);
    let (real, imaginary) = (mix * filtered.0 + (1.0 - mix), mix * filtered.1);
    real.hypot(imaginary) as f32
}

/// Where every glide of one frame is.
#[derive(Clone, Copy, Default)]
struct Frame {
    taps: [f32; 4],
    level: f32,
    drive: f32,
    mix: f32,
    warmth: f32,
    /// 0 is the way at the sample rate, 1 the way at four times it.
    oversampled: f32,
}

/// Everything of one channel that remembers.
#[derive(Clone, Copy)]
struct Channel {
    /// The two sections at the sample rate.
    sections: [SvfSection; 2],
    /// The two sections at four times the sample rate.
    oversampled: [SvfSection; 2],
    oversampler: Oversampler,
    /// The sound of the way at the sample rate, and the input, on their way to the output.
    late: OversamplerDelay,
    late_dry: OversamplerDelay,
}

impl Channel {
    fn new() -> Self {
        Self {
            sections: [SvfSection::default(); 2],
            oversampled: [SvfSection::default(); 2],
            oversampler: Oversampler::new(),
            late: OversamplerDelay::new(),
            late_dry: OversamplerDelay::new(),
        }
    }

    /// Lets go of a memory too small to hear, after a block of silent input. True when every
    /// section is silent.
    fn settle(&mut self) -> bool {
        let sections = self.sections.iter_mut().chain(&mut self.oversampled);
        sections.fold(true, |silent, section| {
            section.settle();
            silent && section.is_silent()
        })
    }
}

pub struct Filter {
    /// The record, with the values of the lanes that automate it.
    state: Automated<FilterState, { PARAMETERS.len() }>,
    sample_rate: f32,
    /// The frames a change takes.
    ramp_frames: f32,
    /// The cutoff as `log2` of hertz, so a glide and the LFO move it in octaves.
    octaves: Smoothed,
    resonance: Smoothed,
    /// 0 is 12 dB per octave, 1 is 24 dB.
    slope: Smoothed,
    /// The weights of low, band and high pass and of the notch.
    taps: [Smoothed; 4],
    /// The gain into the saturation, as a factor.
    drive: Smoothed,
    mix: Smoothed,
    /// 0 is clean, 1 is warm.
    warmth: Smoothed,
    /// 1 while drive or warmth bends the sound, else 0.
    oversampled: Smoothed,
    lfo_depth: Smoothed,
    lfo_rate_hz: f32,
    lfo: Lfo,
    /// Whether the factors have to be worked out again although nothing glides: after an
    /// update that snapped, and before the first block.
    stale: bool,
    /// The factors of the sections at the sample rate and at four times it, and whether each
    /// is older than the cutoff, the resonance and the slope. Each is worked out only while
    /// its way runs.
    factors: [[SvfFactors; 2]; 2],
    outdated: [bool; 2],
    /// The level of the low and high pass of the first section, at the end of the last run of
    /// frames, and where it is going in the run now. It moves with the resonance, frame by
    /// frame inside a run, so it makes no step.
    level: f32,
    level_target: f32,
    filters: OversamplingFilters,
    /// Left first.
    channels: [Channel; CHANNELS],
    /// Whether each way ran in the last run of frames, so its memory is cleared once when it
    /// stops, and not in every block.
    running: [bool; 2],
    /// Frames in a row of silent input and silent sections, up to [`QUIET_FRAMES`].
    quiet: usize,
}

impl Filter {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<FilterState, { PARAMETERS.len() }> =
        AutomationInput::new(0, PARAMETERS);

    /// Starts at these values, so a filter that is added or opened does not glide in.
    pub fn new(state: FilterState) -> Self {
        let mut filter = Self {
            state: Automated::new(Self::AUTOMATION, state),
            sample_rate: 48_000.0,
            ramp_frames: 1.0,
            octaves: Smoothed::new(0.0),
            resonance: Smoothed::new(0.0),
            slope: Smoothed::new(0.0),
            taps: [0.0; 4].map(Smoothed::new),
            drive: Smoothed::new(1.0),
            mix: Smoothed::new(1.0),
            warmth: Smoothed::new(0.0),
            oversampled: Smoothed::new(0.0),
            lfo_depth: Smoothed::new(0.0),
            lfo_rate_hz: state.lfo_rate_hz,
            lfo: Lfo::default(),
            stale: true,
            factors: [[SvfFactors::default(); 2]; 2],
            outdated: [true; 2],
            level: 1.0,
            level_target: 1.0,
            filters: OversamplingFilters::new(),
            channels: [Channel::new(); CHANNELS],
            running: [false; 2],
            quiet: QUIET_FRAMES,
        };
        filter.aim(&filter.state.targets(filter.ramp_frames));
        filter.snap();
        filter
    }

    /// Sets every target from the record and its lanes, each reached in its own ramp.
    fn aim(&mut self, targets: &FilterTargets) {
        let (state, edit) = (*self.state, targets.edit());
        self.octaves
            .set_target(state.cutoff_hz.log2(), targets.ramp(&CUTOFF));
        self.resonance
            .set_target(state.resonance, targets.ramp(&RESONANCE));
        self.slope.set_target(state.slope.weight(), edit);
        for (tap, target) in self.taps.iter_mut().zip(state.kind.taps()) {
            tap.set_target(target, edit);
        }
        let drive = amplitude(state.drive_db);
        self.drive.set_target(drive, targets.ramp(&DRIVE));
        self.mix.set_target(state.mix, targets.ramp(&MIX));
        self.warmth.set_target(state.character.warmth(), edit);
        let bends = state.drive_db > 0.0 || state.character == FilterCharacter::Warm;
        self.oversampled
            .set_target(if bends { 1.0 } else { 0.0 }, edit);
        self.lfo_depth
            .set_target(state.lfo_depth_octaves, targets.ramp(&LFO_DEPTH));
        self.lfo_rate_hz = state.lfo_rate_hz;
        // A number that took its value at once does not move, so nothing else says the
        // factors are old.
        self.stale |= targets.snaps();
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        let [low, band, high, notch] = &mut self.taps;
        [
            &mut self.octaves,
            &mut self.resonance,
            &mut self.slope,
            low,
            band,
            high,
            notch,
            &mut self.drive,
            &mut self.mix,
            &mut self.warmth,
            &mut self.oversampled,
            &mut self.lfo_depth,
        ]
        .into_iter()
    }

    /// Takes every target at once. For a filter nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        self.smoothers().for_each(Smoothed::snap);
        self.stale = true;
    }

    /// Which way runs over the next frames: at the sample rate, at four times it, or both
    /// while one glides into the other.
    fn ways(&self) -> [bool; 2] {
        let (now, moving) = (self.oversampled.current(), self.oversampled.is_moving());
        [now < 1.0 || moving, now > 0.0 || moving]
    }

    /// Moves the cutoff, the resonance, the slope and the LFO `frames` along, and works out the
    /// factors for where they are when anything of them moves, for each way that runs.
    fn move_factors(&mut self, frames: usize, ways: [bool; 2]) {
        let changes = self.stale
            || self.octaves.is_moving()
            || self.resonance.is_moving()
            || self.slope.is_moving()
            || self.lfo_depth.is_moving()
            || self.lfo_depth.current() != 0.0;
        let cutoff = self.octaves.advance(frames);
        let resonance = self.resonance.advance(frames);
        let slope = self.slope.advance(frames);
        let depth = self.lfo_depth.advance(frames);
        let lfo = self.lfo.value(LfoShape::Sine, 0.0);
        self.lfo.advance(frames, self.lfo_rate_hz, self.sample_rate);
        self.level = self.level_target;
        if changes {
            self.outdated = [true; 2];
        }
        let hz = (cutoff + depth * lfo).exp2();
        let rates = [1.0, Oversampler::FACTOR as f32];
        for (index, rate) in rates.into_iter().enumerate() {
            if !ways[index] || !self.outdated[index] {
                continue;
            }
            let sample_rate = rate * self.sample_rate;
            let (factors, level) = SvfFactors::sections(hz, resonance, slope, sample_rate);
            self.factors[index] = factors;
            self.level_target = level;
            self.outdated[index] = false;
        }
        // After a snap there is nothing to glide from.
        if changes && std::mem::take(&mut self.stale) {
            self.level = self.level_target;
        }
    }

    /// Moves every glide of the frames one frame along.
    fn move_frames(&mut self, frames: &mut [Frame]) {
        let (from, to, length) = (self.level, self.level_target, frames.len());
        for (index, frame) in frames.iter_mut().enumerate() {
            *frame = Frame {
                taps: self.taps.each_mut().map(|tap| tap.advance(1)),
                level: from + (to - from) * (index + 1) as f32 / length as f32,
                drive: self.drive.advance(1),
                mix: self.mix.advance(1),
                warmth: self.warmth.advance(1),
                oversampled: self.oversampled.advance(1),
            };
        }
    }

    fn is_resting(&self) -> bool {
        self.quiet >= QUIET_FRAMES
    }
}

/// One frame of the drive and both sections, at whatever rate they run.
#[inline]
fn bend<const WARM: bool>(
    [first, second]: &mut [SvfSection; 2],
    factors: &[SvfFactors; 2],
    slope: f32,
    frame: &Frame,
    input: f32,
) -> f32 {
    let driven = soft_clip(input * frame.drive);
    let one = match WARM {
        true => first.next_warm(&factors[0], frame.taps, frame.level, driven, frame.warmth),
        false => first.next(&factors[0], frame.taps, frame.level, driven),
    };
    let two = second.next(&factors[1], frame.taps, 1.0, one);
    one + slope * (two - one)
}

/// The frames of one channel through both sections at four times the rate, in place.
fn oversample<const WARM: bool>(
    channel: &mut Channel,
    filters: &OversamplingFilters,
    factors: &[SvfFactors; 2],
    slope: f32,
    moves: &[Frame],
    frames: &mut [f32],
) {
    let mut four = [0.0; Oversampler::FACTOR * FACTOR_FRAMES];
    let four = &mut four[..Oversampler::FACTOR * frames.len()];
    channel.oversampler.up(filters, frames, four);
    for (samples, frame) in four.chunks_exact_mut(Oversampler::FACTOR).zip(moves) {
        for sample in samples {
            *sample = bend::<WARM>(&mut channel.oversampled, factors, slope, frame, *sample);
        }
    }
    channel.oversampler.down(filters, four, frames);
}

impl Processor for Filter {
    type Update = FilterState;

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

    fn update(&mut self, update: &mut FilterState) {
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
        let frames = context.frames;
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = all_zero(left_in) && all_zero(right_in);
        if silent_input && self.is_resting() {
            // Nothing sounds, nothing rings and the delays are empty: no glide can be heard,
            // and the output is already silent. The LFO goes on, so where it is does not
            // depend on the silence.
            self.snap();
            self.lfo.advance(frames, self.lfo_rate_hz, self.sample_rate);
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
            let ways = self.ways();
            for (way, (ran, runs)) in self.running.iter_mut().zip(ways).enumerate() {
                if std::mem::replace(ran, runs) && !runs {
                    // Cleared once when a way stops, so it starts again from silence.
                    for channel in &mut self.channels {
                        match way {
                            0 => channel.sections = [SvfSection::default(); 2],
                            _ => {
                                channel.oversampled = [SvfSection::default(); 2];
                                channel.oversampler = Oversampler::new();
                            }
                        }
                    }
                }
            }
            self.move_factors(length, ways);
            let mut moves = [Frame::default(); FACTOR_FRAMES];
            let moves = &mut moves[..length];
            self.move_frames(moves);
            let slope = self.slope.current();
            let warm = moves.iter().any(|frame| frame.warmth > 0.0);
            let [plain, four] = self.factors;
            let [left, right] = &mut self.channels;
            for (channel, input, output) in
                [(left, left_in, left_out), (right, right_in, right_out)]
            {
                let mut bent = [0.0; FACTOR_FRAMES];
                let bent = &mut bent[..length];
                for (bent, input) in bent.iter_mut().zip(input) {
                    *bent = held(*input);
                }
                // The way at the sample rate, as the filter always was, then the delay.
                let mut late = [0.0; FACTOR_FRAMES];
                for ((late, dry), frame) in late.iter_mut().zip(&*bent).zip(&*moves) {
                    let mixed = match ways[0] {
                        true => {
                            let wet =
                                bend::<false>(&mut channel.sections, &plain, slope, frame, *dry);
                            dry + frame.mix * (wet - dry)
                        }
                        false => 0.0,
                    };
                    *late = channel.late.next(mixed);
                }
                let mut late_dry = [0.0; FACTOR_FRAMES];
                for (late_dry, dry) in late_dry.iter_mut().zip(&*bent) {
                    *late_dry = channel.late_dry.next(*dry);
                }
                if !ways[1] {
                    output.copy_from_slice(&late[..length]);
                    continue;
                }
                let filters = &self.filters;
                match warm {
                    true => oversample::<true>(channel, filters, &four, slope, moves, bent),
                    false => oversample::<false>(channel, filters, &four, slope, moves, bent),
                }
                let frames = output.iter_mut().zip(&*bent).zip(late.iter().zip(late_dry));
                for (((output, wet), (late, dry)), frame) in frames.zip(&*moves) {
                    let oversampled = dry + frame.mix * (wet - dry);
                    *output = late + frame.oversampled * (oversampled - late);
                }
            }
        }
        let silent = silent_input
            && self
                .channels
                .iter_mut()
                .fold(true, |silent, channel| channel.settle() && silent);
        self.quiet = match silent {
            true => (self.quiet + frames).min(QUIET_FRAMES),
            false => 0,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Slope;

    #[test]
    fn the_cutoff_is_at_minus_three_db_for_both_slopes_at_resonance_zero() {
        for slope in Slope::ALL {
            let state = FilterState {
                resonance: 0.0,
                slope,
                ..FilterState::default()
            };
            let db = 20.0 * response(&state, state.cutoff_hz, 48_000.0).log10();
            assert!((db + 3.0103).abs() < 0.001, "{slope:?}: {db}");
        }
    }
}
