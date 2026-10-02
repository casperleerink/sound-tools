//! The Wavetable processor: 16 voices, the glides of every setting, and the tables.
//!
//! Everything is allocated with the processor. A voice that is not in use costs one comparison
//! per block, and a synth with no voice in use returns before it touches its output.
//!
//! Every setting glides over 20 ms, choices too: a filter type or slope, a filter turned on or
//! off, the routing and the unison count are weights that glide. A new table or oscillator
//! effect cannot be a weight, so the oscillator fades out over 10 ms, changes, and fades in.

use std::sync::Arc;

use sound_core::{
    AudioOutput, Automated, AutomationInput, Envelope, EnvelopeCurves, EventInput, Lfo, LfoShape,
    OversamplingFilters, Parameter, Ports, PrepareConfig, ProcessContext, Processor, Smoothed,
    Targets, Transport, amplitude,
};
use sound_notes::{NoteEvent, Voice as _, Voices, Wheels};

use crate::matrix::Route;
use crate::state::{
    AUTOMATED, Adsr, Effect, FILTER_LANES, Filter, GAIN, OSCILLATOR_LANES, Oscillator, Routing,
    VOICE_LANES, VoiceMode, WavetableState,
};
use crate::tables::Wavetable;
use crate::voice::{NoteStart, Voice};

type WavetableTargets = Targets<WavetableState, { AUTOMATED.len() }>;

/// The lanes of the numbers of one object, in the order of their list in [`crate::state`].
type Lanes<const N: usize> = [Parameter<WavetableState>; N];

/// Notes that sound at once, at most. The polyphony of a record picks how many of them play.
pub(crate) const VOICES: usize = 16;

/// Unison copies of each oscillator, at most.
pub(crate) const MAX_UNISON: usize = 8;

/// While a cutoff or a resonance moves, a voice works its filter out again this often. Four
/// times per block of the engine: a sweep has no steps anyone can hear.
pub(crate) const FACTOR_FRAMES: usize = 16;

/// How long a change of a setting takes.
const RAMP_SECONDS: f32 = 0.02;

/// The frames a change of a setting takes at a sample rate.
fn ramp_frames(sample_rate: f32) -> f32 {
    (RAMP_SECONDS * sample_rate).max(1.0)
}

/// How long an oscillator takes to fade out, and in again, around a new table or effect.
const DIP_SECONDS: f32 = 0.01;

/// A value that moves in a straight line over a block, from where it was to where it goes.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct Ramp {
    pub from: f32,
    pub to: f32,
}

impl Ramp {
    pub(crate) fn new(from: f32, to: f32) -> Self {
        Self { from, to }
    }

    /// The value at the end of frame `frame` of `frames`.
    #[inline]
    pub(crate) fn at(&self, frame: usize, frames: usize) -> f32 {
        // A product with the reciprocal, which a loop works out once, and not a division.
        self.from + (self.to - self.from) * ((frame + 1) as f32 * (1.0 / frames as f32))
    }

    pub(crate) fn map(self, change: impl Fn(f32) -> f32) -> Self {
        Self::new(change(self.from), change(self.to))
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.from == 0.0 && self.to == 0.0
    }

    /// Moves `smoothed` `frames` along, from where it is.
    fn advance(smoothed: &mut Smoothed, frames: usize) -> Self {
        let from = smoothed.current();
        Self::new(from, smoothed.advance(frames))
    }
}

/// What every voice reads for one block: the settings where their glides are at its end, and
/// the tables.
pub(crate) struct Block<'a> {
    pub frames: usize,
    pub sample_rate: f32,
    pub oscillators: [OscillatorBlock<'a>; 2],
    pub sub_gain: f32,
    /// The frequency of the sub as a part of the note's.
    pub sub_ratio: f32,
    pub unison_voices: usize,
    pub unison_amount: f32,
    /// The part of the level of each unison copy: 1 over the square root of the count, so more
    /// copies are about as loud as one, and 0 for the copies that do not play.
    pub unison_levels: [f32; MAX_UNISON],
    pub filters: [FilterBlock; 2],
    /// How much goes where, as [`routing_weights`] lists it.
    pub routing: [Ramp; ROUTING_WEIGHTS],
    /// Amp, env 2, env 3.
    pub envelopes: &'a [Envelope; 3],
    pub lfo_hz: [f32; 2],
    pub lfo_shapes: [LfoShape; 2],
    /// The bend and the vibrato of the wheels.
    pub pitch_ratio: f32,
    pub mod_wheel: f32,
    pub pressure: f32,
    pub routes: &'a [Route],
    pub oversampling: &'a OversamplingFilters,
}

pub(crate) struct OscillatorBlock<'a> {
    pub table: &'a Wavetable,
    pub effect: Effect,
    pub position: f32,
    pub effect_amount: f32,
    pub gain: f32,
    pub pan: f32,
    /// Octave, semitone and detune, in semitones.
    pub transpose: f32,
    /// 1 while it sounds and 0 while it is off, and between while it fades.
    pub level: f32,
}

pub(crate) struct FilterBlock {
    /// In octaves, the base 2 logarithm of the frequency.
    pub cutoff: f32,
    pub resonance: f32,
    /// The gain into the saturation, as a factor.
    pub drive: Ramp,
    /// From 0 without drive to 1 with it: how much of the saturated sound it takes.
    pub saturated: Ramp,
    /// The weights of low, band and high pass and notch.
    pub taps: [Ramp; 4],
    /// From 0 at 12 dB per octave to 1 at 24.
    pub slope: Ramp,
    /// From 0 while the filter is off to 1 while it is on.
    pub wet: Ramp,
    /// From 0 to 1 while any filter of the patch has drive: then both filters run at four
    /// times the rate, the one without drive clean, so the two come out equally late and a
    /// sum of them has no comb. Off, a filter still waits.
    pub oversampled: Ramp,
}

const ROUTING_WEIGHTS: usize = 9;

/// A routing as weights, so a change of routing glides. In this order: oscillator 1,
/// oscillator 2 and the sub into filter 1; the same into filter 2; filter 1 into filter 2;
/// filter 1 and filter 2 out. A filter that is off lets its input through.
fn routing_weights(routing: Routing) -> [f32; ROUTING_WEIGHTS] {
    match routing {
        Routing::Serial => [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0],
        Routing::Parallel => [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.5, 0.5],
        Routing::Split => [1.0, 0.0, 0.5, 0.0, 1.0, 0.5, 0.0, 1.0, 1.0],
    }
}

/// What the processor gets from the behaviour: the record and the tables it names.
pub(crate) struct Update {
    pub(crate) state: WavetableState,
    pub(crate) tables: [Option<Arc<Wavetable>>; 2],
}

impl Update {
    pub(crate) fn new(state: WavetableState, tables: [Arc<Wavetable>; 2]) -> Self {
        Self {
            state,
            tables: tables.map(Some),
        }
    }
}

/// The glides of one oscillator, and its table and effect.
struct OscillatorGlide {
    position: Smoothed,
    effect_amount: Smoothed,
    gain: Smoothed,
    pan: Smoothed,
    detune: Smoothed,
    /// 1 while it sounds. 0 while it is off, and on the way down and up around a new table or
    /// effect.
    level: Smoothed,
    /// The effect the voices play. Another one waits for `level` to reach 0.
    effect: Effect,
    table: Arc<Wavetable>,
    /// A table that waits for `level` to reach 0 while `other_is_new`, or else the one played
    /// before, which the next update takes back to the control thread: nothing is freed here.
    other: Option<Arc<Wavetable>>,
    other_is_new: bool,
}

impl OscillatorGlide {
    fn new(table: Arc<Wavetable>, effect: Effect) -> Self {
        Self {
            position: Smoothed::new(0.0),
            effect_amount: Smoothed::new(0.0),
            gain: Smoothed::new(0.0),
            pan: Smoothed::new(0.0),
            detune: Smoothed::new(0.0),
            level: Smoothed::new(0.0),
            effect,
            table,
            other: None,
            other_is_new: false,
        }
    }

    /// Takes the table of an update. The update carries back whatever it swaps out.
    fn offer(&mut self, offered: &mut Option<Arc<Wavetable>>) {
        let Some(table) = offered else {
            return;
        };
        if Arc::ptr_eq(table, &self.table) {
            // Back to the table that plays: what waited stays until the next update takes it.
            self.other_is_new = false;
        } else if self
            .other
            .as_ref()
            .is_some_and(|other| Arc::ptr_eq(other, table))
        {
            self.other_is_new = true;
        } else {
            std::mem::swap(offered, &mut self.other);
            self.other_is_new = true;
        }
    }

    fn is_changing(&self, oscillator: &Oscillator) -> bool {
        self.other_is_new || self.effect != oscillator.effect
    }

    /// Aims at `oscillator`, whose numbers are `lanes` of the record.
    fn aim(
        &mut self,
        oscillator: &Oscillator,
        lanes: &Lanes<5>,
        targets: &WavetableTargets,
        dip: f32,
    ) {
        let [position, effect_amount, detune, gain, pan] =
            lanes.each_ref().map(|lane| targets.ramp(lane));
        self.position.set_target(oscillator.position, position);
        self.effect_amount
            .set_target(oscillator.effect_amount, effect_amount);
        self.gain.set_target(oscillator.gain, gain);
        self.pan.set_target(oscillator.pan, pan);
        self.detune.set_target(oscillator.detune_cents, detune);
        if self.is_changing(oscillator) {
            self.level.set_target(0.0, dip);
        } else {
            self.level
                .set_target(if oscillator.on { 1.0 } else { 0.0 }, targets.edit());
        }
    }

    /// Takes the new table and effect, once the oscillator is silent. True when it did.
    fn change(&mut self, oscillator: &Oscillator, dip: f32) -> bool {
        if !self.is_changing(oscillator) || self.level.current() != 0.0 {
            return false;
        }
        self.effect = oscillator.effect;
        if std::mem::take(&mut self.other_is_new)
            && let Some(other) = self.other.as_mut()
        {
            std::mem::swap(&mut self.table, other);
        }
        self.level
            .set_target(if oscillator.on { 1.0 } else { 0.0 }, dip);
        true
    }

    fn smoothers(&mut self) -> [&mut Smoothed; 6] {
        [
            &mut self.position,
            &mut self.effect_amount,
            &mut self.gain,
            &mut self.pan,
            &mut self.detune,
            &mut self.level,
        ]
    }
}

/// The glides of one filter.
struct FilterGlide {
    /// In octaves.
    cutoff: Smoothed,
    resonance: Smoothed,
    drive: Smoothed,
    taps: [Smoothed; 4],
    slope: Smoothed,
    wet: Smoothed,
    saturated: Smoothed,
    oversampled: Smoothed,
}

impl FilterGlide {
    fn new() -> Self {
        Self {
            cutoff: Smoothed::new(0.0),
            resonance: Smoothed::new(0.0),
            drive: Smoothed::new(1.0),
            taps: [0.0; 4].map(Smoothed::new),
            slope: Smoothed::new(0.0),
            wet: Smoothed::new(0.0),
            saturated: Smoothed::new(0.0),
            oversampled: Smoothed::new(0.0),
        }
    }

    /// Aims at `filter`, whose numbers are `lanes` of the record. What a choice or the drive
    /// turns on or off glides as an edit.
    fn aim(
        &mut self,
        filter: &Filter,
        lanes: &Lanes<3>,
        targets: &WavetableTargets,
        any_driven: bool,
    ) {
        let [cutoff, resonance, drive] = lanes.each_ref().map(|lane| targets.ramp(lane));
        self.cutoff.set_target(filter.cutoff_hz.log2(), cutoff);
        self.resonance.set_target(filter.resonance, resonance);
        let driven = filter.on && filter.drive_db > 0.0;
        self.drive.set_target(amplitude(filter.drive_db), drive);
        let ramp = targets.edit();
        self.saturated
            .set_target(if driven { 1.0 } else { 0.0 }, ramp);
        for (tap, target) in self.taps.iter_mut().zip(filter.kind.taps()) {
            tap.set_target(target, ramp);
        }
        self.slope.set_target(filter.slope.weight(), ramp);
        self.wet.set_target(if filter.on { 1.0 } else { 0.0 }, ramp);
        let oversampled = if any_driven { 1.0 } else { 0.0 };
        self.oversampled.set_target(oversampled, ramp);
    }

    fn advance(&mut self, frames: usize) -> FilterBlock {
        FilterBlock {
            cutoff: self.cutoff.advance(frames),
            resonance: self.resonance.advance(frames),
            drive: Ramp::advance(&mut self.drive, frames),
            saturated: Ramp::advance(&mut self.saturated, frames),
            taps: self.taps.each_mut().map(|tap| Ramp::advance(tap, frames)),
            slope: Ramp::advance(&mut self.slope, frames),
            wet: Ramp::advance(&mut self.wet, frames),
            oversampled: Ramp::advance(&mut self.oversampled, frames),
        }
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        [
            &mut self.cutoff,
            &mut self.resonance,
            &mut self.drive,
            &mut self.slope,
            &mut self.wet,
            &mut self.saturated,
            &mut self.oversampled,
        ]
        .into_iter()
        .chain(&mut self.taps)
    }
}

/// Every setting that glides.
struct Glides {
    oscillators: [OscillatorGlide; 2],
    sub_gain: Smoothed,
    unison_amount: Smoothed,
    unison_levels: [Smoothed; MAX_UNISON],
    filters: [FilterGlide; 2],
    routing: [Smoothed; ROUTING_WEIGHTS],
    gain: Smoothed,
}

impl Glides {
    /// Aims every glide at the record and its lanes, each number in its own ramp, and a choice
    /// in the glide of an edit.
    fn aim(&mut self, state: &WavetableState, targets: &WavetableTargets, dip: f32) {
        let oscillators = self.oscillators.iter_mut().zip(state.oscillators());
        for ((glide, oscillator), lanes) in oscillators.zip(&OSCILLATOR_LANES) {
            glide.aim(oscillator, lanes, targets, dip);
        }
        let [sub, unison] = VOICE_LANES.each_ref().map(|lane| targets.ramp(lane));
        self.sub_gain.set_target(state.sub.gain, sub);
        self.unison_amount.set_target(state.unison.amount, unison);
        let ramp = targets.edit();
        let voices = usize::from(state.unison.voices).clamp(1, MAX_UNISON);
        for (copy, level) in self.unison_levels.iter_mut().enumerate() {
            let target = if copy < voices {
                1.0 / (voices as f32).sqrt()
            } else {
                0.0
            };
            level.set_target(target, ramp);
        }
        let driven = |filter: &&Filter| filter.on && filter.drive_db > 0.0;
        let any_driven = state.filters().iter().any(driven);
        let filters = self.filters.iter_mut().zip(state.filters());
        for ((glide, filter), lanes) in filters.zip(&FILTER_LANES) {
            glide.aim(filter, lanes, targets, any_driven);
        }
        for (weight, target) in self.routing.iter_mut().zip(routing_weights(state.routing)) {
            weight.set_target(target, ramp);
        }
        self.gain.set_target(state.gain, targets.ramp(&GAIN));
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoothed> {
        let [one, two] = &mut self.oscillators;
        let [filter_1, filter_2] = &mut self.filters;
        one.smoothers()
            .into_iter()
            .chain(two.smoothers())
            .chain([&mut self.sub_gain, &mut self.unison_amount, &mut self.gain])
            .chain(&mut self.unison_levels)
            .chain(filter_1.smoothers())
            .chain(filter_2.smoothers())
            .chain(&mut self.routing)
    }
}

/// The envelopes of a record at a sample rate: amp, env 2, env 3.
fn envelopes(state: &WavetableState, sample_rate: f32) -> [Envelope; 3] {
    [&state.amp_env, &state.env_2, &state.env_3].map(|adsr: &Adsr| {
        let curves = EnvelopeCurves {
            attack: adsr.attack_curve,
            decay: adsr.decay_curve,
            release: adsr.release_curve,
        };
        Envelope::curved(
            adsr.attack_seconds,
            adsr.decay_seconds,
            adsr.sustain,
            adsr.release_seconds,
            curves,
            sample_rate,
        )
    })
}

/// The seeds of the free-running LFOs, far from those of the notes, which count up from 0.
const FREE_SEEDS: [u32; 2] = [u32::MAX, u32::MAX - 1];

pub(crate) struct WavetableSynth {
    /// The record, with the values of the lanes that automate it.
    state: Automated<WavetableState, { AUTOMATED.len() }>,
    /// Zero until `prepare` runs.
    sample_rate: f32,
    envelopes: [Envelope; 3],
    glides: Glides,
    /// The LFOs a note picks up when it does not start its own. They run while nothing plays,
    /// and follow the bars while the project plays.
    free_lfos: [Lfo; 2],
    voices: Voices<Voice, VOICES>,
    /// The bend and the vibrato of every voice. At rest after every `AllOff`, like the pedal.
    wheels: Wheels,
    oversampling: OversamplingFilters,
    /// Counts the notes, for the seeds of their LFOs and their random value.
    notes_started: u32,
}

impl WavetableSynth {
    pub(crate) const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub(crate) const AUTOMATION: AutomationInput<WavetableState, { AUTOMATED.len() }> =
        AutomationInput::new(1, AUTOMATED);

    /// Starts at the settings of `state`, with `tables` for its oscillators, so a synth that
    /// is added or opened does not glide in.
    pub(crate) fn new(state: WavetableState, [one, two]: [Arc<Wavetable>; 2]) -> Self {
        let effects = state.oscillators().map(|oscillator| oscillator.effect);
        let mut synth = Self {
            envelopes: [Envelope::default(); 3],
            glides: Glides {
                oscillators: [
                    OscillatorGlide::new(one, effects[0]),
                    OscillatorGlide::new(two, effects[1]),
                ],
                sub_gain: Smoothed::new(0.0),
                unison_amount: Smoothed::new(0.0),
                unison_levels: [0.0; MAX_UNISON].map(Smoothed::new),
                filters: [FilterGlide::new(), FilterGlide::new()],
                routing: [0.0; ROUTING_WEIGHTS].map(Smoothed::new),
                gain: Smoothed::new(0.0),
            },
            free_lfos: FREE_SEEDS.map(Lfo::seeded),
            voices: Voices::new(Voice::idle(), VOICES),
            wheels: Wheels::default(),
            oversampling: OversamplingFilters::new(),
            notes_started: 0,
            sample_rate: 0.0,
            state: Automated::new(Self::AUTOMATION, state),
        };
        synth.apply_state(&synth.state.targets(ramp_frames(1.0)), 1.0);
        synth.rest();
        synth
    }

    /// Aims every glide at the record and its lanes, and follows its voicing and envelopes.
    fn apply_state(&mut self, targets: &WavetableTargets, sample_rate: f32) {
        let dip = (DIP_SECONDS * sample_rate).max(1.0);
        self.glides.aim(&self.state, targets, dip);
        self.envelopes = envelopes(&self.state, sample_rate);
        let voicing = &self.state.voicing;
        self.voices.set_mono(voicing.mode == VoiceMode::Mono);
        self.voices.set_polyphony(usize::from(voicing.polyphony));
        self.voices.set_glide(voicing.glide_seconds * sample_rate);
    }

    /// Nothing sounds, so there is nothing to glide or fade: every setting takes its value.
    fn rest(&mut self) {
        let [one, two] = self.state.oscillators();
        for (glide, oscillator) in self.glides.oscillators.iter_mut().zip([one, two]) {
            glide.level.snap();
            glide.change(oscillator, 1.0);
        }
        self.glides.smoothers().for_each(Smoothed::snap);
    }

    /// The rate of each LFO at the start of this block, and the free-running ones in time
    /// with the bars while the project plays.
    fn lfo_hz(&mut self, transport: &Transport<'_>) -> [f32; 2] {
        let [one, two] = self.state.lfos();
        let mut hz = [0.0; 2];
        for ((rate, lfo), settings) in hz.iter_mut().zip(&mut self.free_lfos).zip([one, two]) {
            *rate = if settings.sync {
                lfo.sync(transport, settings.division.quarters_with(settings.feel))
            } else {
                settings.rate_hz
            };
        }
        hz
    }

    fn handle(&mut self, event: NoteEvent) {
        self.wheels.follow(event);
        if matches!(event, NoteEvent::On { .. }) {
            self.notes_started = self.notes_started.wrapping_add(1);
        }
        let seed = self.notes_started;
        let settings = self.state.lfos();
        let start = NoteStart {
            lfos: std::array::from_fn(|index| {
                if settings[index].retrigger {
                    Lfo::seeded(seed.wrapping_mul(2).wrapping_add(index as u32))
                } else {
                    self.free_lfos[index]
                }
            }),
            // The first level of a sample and hold is a value from -1 to 1 for the seed.
            random: Lfo::seeded(seed ^ 0x5EED_0000).value(LfoShape::SampleAndHold, 0.0),
        };
        self.voices.handle(event, &start);
    }

    /// Renders the frames between two events.
    fn render(&mut self, [left, right]: [&mut [f32]; 2], lfo_hz: [f32; 2]) {
        let frames = left.len();
        if frames == 0 {
            return;
        }
        let dip = (DIP_SECONDS * self.sample_rate).max(1.0);
        let [one, two] = self.state.oscillators();
        for (index, (glide, oscillator)) in self
            .glides
            .oscillators
            .iter_mut()
            .zip([one, two])
            .enumerate()
        {
            if glide.change(oscillator, dip) {
                self.voices
                    .iter_mut()
                    .for_each(|voice| voice.reset_oscillator(index));
            }
        }
        let glides = &mut self.glides;
        let oscillator = |glide: &mut OscillatorGlide, state: &Oscillator| {
            let detune = glide.detune.advance(frames);
            (
                glide.position.advance(frames),
                glide.effect_amount.advance(frames),
                glide.gain.advance(frames),
                glide.pan.advance(frames),
                glide.level.advance(frames),
                f32::from(state.octave) * 12.0 + f32::from(state.semitone) + detune / 100.0,
            )
        };
        let [glide_1, glide_2] = &mut glides.oscillators;
        let values = [oscillator(glide_1, one), oscillator(glide_2, two)];
        let expression = self.wheels.expression();
        let [lfo_1, lfo_2] = self.state.lfos();
        let [filter_1, filter_2] = &mut glides.filters;
        let block = Block {
            frames,
            sample_rate: self.sample_rate,
            oscillators: [0, 1].map(|index| {
                let glide = &glides.oscillators[index];
                let (position, effect_amount, gain, pan, level, transpose) = values[index];
                OscillatorBlock {
                    table: &glide.table,
                    effect: glide.effect,
                    position,
                    effect_amount,
                    gain,
                    pan,
                    transpose,
                    level,
                }
            }),
            sub_gain: glides.sub_gain.advance(frames),
            sub_ratio: (f32::from(i8::from(self.state.sub.octave))).exp2(),
            unison_voices: usize::from(self.state.unison.voices).clamp(1, MAX_UNISON),
            unison_amount: glides.unison_amount.advance(frames),
            unison_levels: glides
                .unison_levels
                .each_mut()
                .map(|level| level.advance(frames)),
            filters: [filter_1.advance(frames), filter_2.advance(frames)],
            routing: glides
                .routing
                .each_mut()
                .map(|weight| Ramp::advance(weight, frames)),
            envelopes: &self.envelopes,
            lfo_hz,
            lfo_shapes: [lfo_1.shape, lfo_2.shape],
            pitch_ratio: self.wheels.pitch_ratio(frames, self.sample_rate),
            mod_wheel: expression.mod_wheel.fraction(),
            pressure: expression.pressure.fraction(),
            routes: &self.state.matrix,
            oversampling: &self.oversampling,
        };
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            voice.render([&mut *left, &mut *right], &block);
        }
        let start = NoteStart {
            lfos: [Lfo::default(); 2],
            random: 0.0,
        };
        self.voices.glide(frames, &start);
        // A note that starts after these frames picks the free LFOs up where they are then.
        for (lfo, hz) in self.free_lfos.iter_mut().zip(lfo_hz) {
            lfo.advance(frames, hz, self.sample_rate);
        }
        let gain = Ramp::advance(&mut self.glides.gain, frames);
        for (frame, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let gain = gain.at(frame, frames);
            *left *= gain;
            *right *= gain;
        }
    }
}

impl Processor for WavetableSynth {
    type Update = Update;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION.port())
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        let targets = self.state.targets(ramp_frames(self.sample_rate));
        self.apply_state(&targets, self.sample_rate);
        self.rest();
    }

    fn update(&mut self, update: &mut Update) {
        // The record this replaces rides back to the control thread in the update.
        let ramp = ramp_frames(self.sample_rate);
        let targets = self.state.set_record(&mut update.state, ramp);
        for (glide, table) in self.glides.oscillators.iter_mut().zip(&mut update.tables) {
            glide.offer(table);
        }
        self.apply_state(&targets, self.sample_rate);
        if self.voices.is_idle() {
            self.rest();
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, ramp_frames(self.sample_rate)) {
            self.apply_state(&targets, self.sample_rate);
        }
        let lfo_hz = self.lfo_hz(&context.transport);
        let frames = context.frames;
        let events = context.event_inputs.get(Self::NOTES);
        if events.is_empty() && self.voices.is_idle() {
            self.rest();
            for (lfo, hz) in self.free_lfos.iter_mut().zip(lfo_hz) {
                lfo.advance(frames, hz, self.sample_rate);
            }
            return;
        }
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let mut rendered = 0;
        for timed in events {
            let offset = timed.offset.clamp(rendered, left.len());
            self.render(
                [&mut left[rendered..offset], &mut right[rendered..offset]],
                lfo_hz,
            );
            rendered = offset;
            self.handle(timed.event);
        }
        self.render([&mut left[rendered..], &mut right[rendered..]], lfo_hz);
    }
}
