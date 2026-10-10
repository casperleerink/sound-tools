//! The filter processor: a drive stage, two state variable filter sections, and the mix.
//!
//! The two sections are the state variable filter of the SDK, `SvfSection`, which says how
//! they sound. Both always run and the output glides from the first to the second, and the type
//! is a weight for each output of a section, so a change of type or slope is a glide and not a
//! switch. Nothing that a composer or an agent changes jumps.

use sound_core::{
    AudioInput, AudioOutput, Automated, AutomationInput, CHANNELS, Lfo, LfoShape, Ports,
    PrepareConfig, ProcessContext, Processor, Smoothed, SvfFactors, SvfSection, Targets, all_zero,
    amplitude, held, soft_clip, svf_response,
};

use crate::{CUTOFF, DRIVE, FilterState, LFO_DEPTH, MIX, PARAMETERS, RESONANCE};

/// Every number of the filter can be automated.
type FilterTargets = Targets<FilterState, { PARAMETERS.len() }>;

/// How long a change takes to arrive. A jump would click, or step in the sound.
const RAMP_SECONDS: f32 = 0.02;

/// While something moves, the factors are worked out again this often. Four times per block
/// of the engine: a sweep has no steps anyone can hear, and a `tan` per frame is not needed.
const FACTOR_FRAMES: usize = 16;

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
    lfo_depth: Smoothed,
    lfo_rate_hz: f32,
    lfo: Lfo,
    /// Whether the factors have to be worked out again although nothing glides: after an
    /// update that snapped, and before the first block.
    stale: bool,
    factors: [SvfFactors; 2],
    /// The level of the low and high pass of the first section, at the end of the last run of
    /// frames, and where it is going in the run now. It moves with the resonance, frame by
    /// frame inside a run, so it makes no step.
    level: f32,
    level_target: f32,
    /// The two sections of each channel, left first.
    sections: [[SvfSection; 2]; CHANNELS],
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
            lfo_depth: Smoothed::new(0.0),
            lfo_rate_hz: state.lfo_rate_hz,
            lfo: Lfo::default(),
            stale: true,
            factors: [SvfFactors::default(); 2],
            level: 1.0,
            level_target: 1.0,
            sections: [[SvfSection::default(); 2]; CHANNELS],
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
            &mut self.lfo_depth,
        ]
        .into_iter()
    }

    /// Takes every target at once. For a filter nobody hears, which has nothing to glide for.
    fn snap(&mut self) {
        self.smoothers().for_each(Smoothed::snap);
        self.stale = true;
    }

    /// Moves the cutoff, the resonance, the slope and the LFO `frames` along, and works out the
    /// factors for where they are when anything of them moves.
    fn move_factors(&mut self, frames: usize) {
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
        if !changes {
            return;
        }
        let hz = (cutoff + depth * lfo).exp2();
        let (factors, level) = SvfFactors::sections(hz, resonance, slope, self.sample_rate);
        self.factors = factors;
        self.level_target = level;
        // After a snap there is nothing to glide from.
        if std::mem::take(&mut self.stale) {
            self.level = level;
        }
    }

    fn is_resting(&self) -> bool {
        self.sections.iter().flatten().all(SvfSection::is_silent)
    }
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

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        let frames = context.frames;
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let silent_input = all_zero(left_in) && all_zero(right_in);
        if silent_input && self.is_resting() {
            // Nothing sounds and nothing rings: no glide can be heard, and the output is
            // already silent. The LFO goes on, so where it is does not depend on the silence.
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
            self.move_factors(length);
            let (factors, slope) = (self.factors, self.slope.current());
            let (from, to) = (self.level, self.level_target);
            let frames = left_in
                .iter()
                .zip(right_in)
                .zip(left_out.iter_mut())
                .zip(right_out.iter_mut());
            for (index, (((left_in, right_in), left_out), right_out)) in frames.enumerate() {
                let level = from + (to - from) * (index + 1) as f32 / length as f32;
                let taps = self.taps.each_mut().map(|tap| tap.advance(1));
                let drive = self.drive.advance(1);
                let mix = self.mix.advance(1);
                let [left, right] = &mut self.sections;
                for (sections, input, output) in
                    [(left, left_in, left_out), (right, right_in, right_out)]
                {
                    let dry = held(*input);
                    let driven = soft_clip(dry * drive);
                    let [first, second] = sections;
                    let one = first.next(&factors[0], taps, level, driven);
                    let two = second.next(&factors[1], taps, 1.0, one);
                    let wet = one + slope * (two - one);
                    *output = dry + mix * (wet - dry);
                }
            }
        }
        if silent_input {
            self.sections
                .iter_mut()
                .flatten()
                .for_each(SvfSection::settle);
        }
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
