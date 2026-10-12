//! The synth processor: 16 voices, each an oscillator into a low-pass filter into an envelope.
//!
//! Everything is allocated with the processor. A voice that is not in use costs one comparison
//! per block, and a synth with no voice in use returns before it touches its output.
//!
//! The warm character bends the sound inside the filter, so a warm voice runs its filter at four
//! times the sample rate, between the two stages of an [`Oversampler`]. A clean voice runs it at
//! the sample rate, as it always has, and the synth delays what the clean voices make by as
//! long. So the latency it reports never moves, and a change of character glides in time.

use sound_core::{
    AudioOutput, Automated, AutomationInput, Envelope, EnvelopeState, EventInput,
    HIGHEST_PHASE_STEP, MAX_BLOCK, Oversampler, OversamplerDelay, OversamplingFilters, Ports,
    PrepareConfig, ProcessContext, Processor, Smoothed, SvfFactors, SvfSection, Targets, poly_blep,
};
use sound_notes::{NoteEvent, Velocity, Voice as _, Voices, Wheels, frequency_hz};

use crate::{CUTOFF, GAIN, PARAMETERS, RESONANCE, SynthState, Waveform};

/// Every number of the synth can be automated.
type SynthTargets = Targets<SynthState, { PARAMETERS.len() }>;

/// Notes that sound at once. One more note takes over a voice in place, as [`Voices`] picks it.
pub const VOICES: usize = 16;

/// How many frames the synth delays its sound, at every sample rate: 1.3 ms at 48 kHz.
pub const LATENCY: u32 = Oversampler::DELAY_FRAMES as u32;

/// The low pass of the first section, as [`SvfSection::next`] takes it.
const LOW_PASS: [f32; 4] = [1.0, 0.0, 0.0, 0.0];

/// How long gain, cutoff and resonance take to reach a new value.
const RAMP_SECONDS: f32 = 0.05;

/// A new voice starts here in its cycle. Both waveforms are far from a step at this phase, so
/// the very first frame of a note already gives output.
const START_PHASE: f32 = 0.25;

/// The envelope of the synth, from its state.
fn envelope(state: &SynthState, sample_rate: f32) -> Envelope {
    Envelope::new(
        state.attack_seconds,
        state.decay_seconds,
        state.sustain,
        state.release_seconds,
        sample_rate,
    )
}

/// The low pass of the shared state variable filter at 12 dB per octave, as the Filter and the
/// Wavetable play it, so a resonance sounds the same in all three. All voices share it.
#[derive(Copy, Clone, Default)]
struct Filter {
    factors: SvfFactors,
    /// The same at four times the sample rate, for a warm voice.
    oversampled: SvfFactors,
    /// What resonance leaves of the level of the low pass.
    level: f32,
}

impl Filter {
    fn new(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> Self {
        let ([factors, _], level) = SvfFactors::sections(cutoff_hz, resonance, 0.0, sample_rate);
        let four = Oversampler::FACTOR as f32 * sample_rate;
        let ([oversampled, _], _) = SvfFactors::sections(cutoff_hz, resonance, 0.0, four);
        Self {
            factors,
            oversampled,
            level,
        }
    }
}

/// What a voice reads from the synth for the frames it renders.
struct Frames<'a> {
    filter: &'a Filter,
    envelope: &'a Envelope,
    pitch_ratio: f32,
    /// From 0, clean, to 1, warm, at the start and at the end of the frames.
    warmth: [f32; 2],
    oversampling: &'a OversamplingFilters,
}

#[derive(Copy, Clone)]
struct Voice {
    /// In cycles, from 0 to 1.
    phase: f32,
    /// The phase step of the pitch of the note, before the wheels move it.
    key_step: f32,
    /// From the velocity.
    amplitude: f32,
    envelope: EnvelopeState,
    filter: SvfSection,
    /// The filter of a warm voice, at four times the sample rate.
    warm: SvfSection,
    /// Started from silence: its warm filter and oversampler still hold the last note.
    fresh: bool,
}

/// The naive sawtooth steps from 1 to -1 where the phase wraps, rounded off there.
fn sawtooth(phase: f32, phase_step: f32) -> f32 {
    2.0 * phase - 1.0 - poly_blep(phase, phase_step)
}

/// The sample rate is what a voice reads from the synth to start or move.
impl sound_notes::Voice for Voice {
    type Context = f32;

    fn is_idle(&self) -> bool {
        self.envelope.is_idle()
    }

    fn loudness(&self) -> f32 {
        self.envelope.level as f32 * self.amplitude
    }

    fn release(&mut self) {
        self.envelope.release();
    }

    fn start(&mut self, pitch: f32, velocity: Velocity, sample_rate: &f32) {
        let amplitude = (f32::from(velocity.value()) / 127.0).powi(2);
        if self.is_idle() {
            self.phase = START_PHASE;
            self.filter = SvfSection::default();
            self.fresh = true;
            self.envelope.level = 0.0;
        } else {
            // A voice taken from another note keeps its phase and filter state, and its
            // loudness (level times amplitude), so the takeover is not a click. A quiet note
            // that takes over a loud voice starts above full level, and decays from there.
            let loudness = f64::from(self.amplitude / amplitude);
            self.envelope.level *= loudness;
        }
        self.envelope.start();
        self.amplitude = amplitude;
        self.set_pitch(pitch, sample_rate);
    }

    fn set_pitch(&mut self, pitch: f32, sample_rate: &f32) {
        self.key_step = frequency_hz(pitch) / sample_rate;
    }
}

impl Voice {
    fn idle() -> Self {
        Self {
            phase: 0.0,
            key_step: 0.0,
            amplitude: 0.0,
            envelope: EnvelopeState::IDLE,
            filter: SvfSection::default(),
            warm: SvfSection::default(),
            fresh: false,
        }
    }

    /// Adds this voice to `clean`, or to `warm` at four times the rate while the synth is
    /// warm, and to both, weighted, while it glides from one to the other. `SQUARE` picks the
    /// waveform at compile time, so the frame loop has no waveform branch.
    fn render<const SQUARE: bool>(
        &mut self,
        clean: &mut [f32],
        warm: &mut [f32],
        oversampler: &mut Oversampler,
        frames: &Frames<'_>,
    ) {
        if frames.warmth == [0.0; 2] {
            let (filter, envelope) = (frames.filter, frames.envelope);
            self.render_clean::<SQUARE>(clean, filter, envelope, frames.pitch_ratio);
            return;
        }
        let length = clean.len();
        let mut oscillator = [0.0; MAX_BLOCK];
        let oscillator = &mut oscillator[..length];
        let phase_step = (self.key_step * frames.pitch_ratio).min(HIGHEST_PHASE_STEP);
        for sample in oscillator.iter_mut() {
            *sample = self.oscillator::<SQUARE>(phase_step);
        }
        let filter = frames.filter;
        let [from, to] = frames.warmth;
        // From silence, or from clean: what the warm filter held is old.
        if std::mem::take(&mut self.fresh) || from == 0.0 {
            self.warm = SvfSection::default();
            *oversampler = Oversampler::new();
        }
        let warmth = |frame: usize| from + (to - from) * (frame + 1) as f32 / length as f32;
        if from == 1.0 && to < 1.0 {
            // Back from warm: what the clean filter held is old.
            self.filter = SvfSection::default();
        }
        let mut plain = [0.0; MAX_BLOCK];
        if from < 1.0 || to < 1.0 {
            for (plain, sample) in plain.iter_mut().zip(&*oscillator) {
                *plain = self.filter.band_and_low(&filter.factors, *sample).1;
            }
        }
        let mut four = [0.0; Oversampler::FACTOR * MAX_BLOCK];
        let four = &mut four[..Oversampler::FACTOR * length];
        oversampler.up(frames.oversampling, oscillator, four);
        for (frame, samples) in four.chunks_exact_mut(Oversampler::FACTOR).enumerate() {
            let warmth = warmth(frame);
            for sample in samples {
                *sample = self
                    .warm
                    .next_warm(&filter.oversampled, LOW_PASS, 1.0, *sample, warmth);
            }
        }
        let still = self.envelope.is_still(frames.envelope);
        let gain = filter.level * self.amplitude;
        let warm = warm.chunks_exact_mut(Oversampler::FACTOR);
        let bent = four.chunks_exact(Oversampler::FACTOR);
        let outputs = clean.iter_mut().zip(warm).zip(plain.iter().zip(bent));
        for (frame, ((clean, warm), (plain, bent))) in outputs.enumerate() {
            if self.envelope.is_idle() {
                break;
            }
            let level = match still {
                true => self.envelope.level,
                false => self.envelope.next(frames.envelope),
            } as f32;
            let warmth = warmth(frame);
            *clean += (1.0 - warmth) * plain * level * gain;
            for (warm, bent) in warm.iter_mut().zip(bent) {
                *warm += warmth * bent * level * gain;
            }
        }
    }

    /// The oscillator at the phase, and the phase one step on.
    #[inline(always)]
    fn oscillator<const SQUARE: bool>(&mut self, phase_step: f32) -> f32 {
        let mut oscillator = sawtooth(self.phase, phase_step);
        if SQUARE {
            // A square is a sawtooth minus the same sawtooth half a cycle later.
            let half_later = self.phase + 0.5;
            let half_later = half_later - half_later.floor();
            oscillator -= sawtooth(half_later, phase_step);
        }
        self.phase += phase_step;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        oscillator
    }

    /// The clean voice at the sample rate.
    fn render_clean<const SQUARE: bool>(
        &mut self,
        output: &mut [f32],
        filter: &Filter,
        envelope: &Envelope,
        pitch_ratio: f32,
    ) {
        let phase_step = (self.key_step * pitch_ratio).min(HIGHEST_PHASE_STEP);
        // Only an event changes the envelope between frames, and events end the frames.
        let still = self.envelope.is_still(envelope);
        let gain = filter.level * self.amplitude;
        for sample in output {
            let oscillator = self.oscillator::<SQUARE>(phase_step);
            let (_, low_pass) = self.filter.band_and_low(&filter.factors, oscillator);

            // An idle voice ended on the frame before: nothing of it is added any more.
            if self.envelope.is_idle() {
                break;
            }
            let level = match still {
                true => self.envelope.level,
                false => self.envelope.next(envelope),
            } as f32;
            *sample += low_pass * level * gain;
        }
    }
}

pub struct Synth {
    /// The record, with the values of the lanes that automate it.
    state: Automated<SynthState, { PARAMETERS.len() }>,
    /// Zero until `prepare` runs.
    sample_rate: f32,
    envelope: Envelope,
    /// In octaves, the base 2 logarithm of the frequency, so a ramp is even to the ear.
    cutoff_octaves: Smoothed,
    resonance: Smoothed,
    /// For the current cutoff and resonance. The tangent in it is worth keeping between blocks.
    filter: Filter,
    gain: Smoothed,
    /// 0 is clean, 1 is warm.
    warmth: Smoothed,
    voices: Voices<Voice, VOICES>,
    /// The bend and the vibrato of every voice. At rest after every `AllOff`, like the pedal.
    wheels: Wheels,
    oversampling: OversamplingFilters,
    /// The way up of each voice, in the order of the voices. Not in the voice: it is large, and
    /// only a warm voice uses it.
    oversamplers: [Oversampler; VOICES],
    /// The way down of the warm voices together, and what they add up to before it.
    down: Oversampler,
    four: [f32; Oversampler::FACTOR * MAX_BLOCK],
    /// What the clean voices make, on its way to the output.
    late: OversamplerDelay,
    /// Frames since the last voice ended, up to the latency: then the delay is empty.
    quiet: usize,
}

impl Synth {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<SynthState, { PARAMETERS.len() }> =
        AutomationInput::new(1, PARAMETERS);

    pub fn new(state: SynthState) -> Self {
        Self {
            state: Automated::new(Self::AUTOMATION, state),
            sample_rate: 0.0,
            envelope: Envelope::default(),
            cutoff_octaves: Smoothed::new(state.cutoff_hz.log2()),
            resonance: Smoothed::new(state.resonance),
            filter: Filter::default(),
            gain: Smoothed::new(state.gain),
            warmth: Smoothed::new(state.character.warmth()),
            voices: Voices::new(Voice::idle(), VOICES),
            wheels: Wheels::default(),
            oversampling: OversamplingFilters::new(),
            oversamplers: [Oversampler::new(); VOICES],
            down: Oversampler::new(),
            four: [0.0; Oversampler::FACTOR * MAX_BLOCK],
            late: OversamplerDelay::new(),
            quiet: Oversampler::DELAY_FRAMES,
        }
    }

    fn handle(&mut self, event: NoteEvent) {
        self.wheels.follow(event);
        self.voices.handle(event, &self.sample_rate);
    }

    /// Moves cutoff and resonance along their ramps and works out the filter for where they are.
    fn move_filter(&mut self, frames: usize) {
        self.filter = Filter::new(
            self.cutoff_octaves.advance(frames).exp2(),
            self.resonance.advance(frames),
            self.sample_rate,
        );
    }

    /// The frames a change takes. Zero until `prepare` runs: then a change is at once.
    fn ramp_frames(&self) -> f32 {
        RAMP_SECONDS * self.sample_rate
    }

    /// Sets every target from the record and its lanes, each reached in its own ramp. The
    /// envelope applies at once, to every voice.
    fn aim(&mut self, targets: &SynthTargets) {
        self.envelope = envelope(&self.state, self.sample_rate);
        self.cutoff_octaves
            .set_target(self.state.cutoff_hz.log2(), targets.ramp(&CUTOFF));
        self.resonance
            .set_target(self.state.resonance, targets.ramp(&RESONANCE));
        self.gain.set_target(self.state.gain, targets.ramp(&GAIN));
        let warmth = self.state.character.warmth();
        self.warmth.set_target(warmth, targets.edit());
        let idle = self.voices.is_idle();
        if idle {
            // Nothing sounds, so there is nothing to smooth. The next note starts on the new values.
            self.cutoff_octaves.snap();
            self.resonance.snap();
            self.gain.snap();
            self.warmth.snap();
        }
        // A filter that took its value at once does not move, so nothing else works it out.
        if idle || targets.snaps() {
            self.move_filter(0);
        }
    }

    /// Renders the frames between two events: the clean voices into `clean`, the warm ones into
    /// `warm`, both of them weighted while the character glides.
    fn render(&mut self, [clean, warm]: [&mut [f32]; 2]) {
        let frames = clean.len();
        if frames == 0 {
            return;
        }
        if self.cutoff_octaves.is_moving() || self.resonance.is_moving() {
            self.move_filter(frames);
        }
        let from = self.warmth.current();
        let to = self.warmth.advance(frames);
        let frames_of = Frames {
            filter: &self.filter,
            envelope: &self.envelope,
            pitch_ratio: self.wheels.pitch_ratio(frames, self.sample_rate),
            warmth: [from, to],
            oversampling: &self.oversampling,
        };
        // The warm voices add up at four times the rate and go down together.
        let warms = from != 0.0 || to != 0.0;
        let four = match warms {
            true => &mut self.four[..Oversampler::FACTOR * frames],
            false => &mut [][..],
        };
        four.fill(0.0);
        let voices = self.voices.iter_mut().zip(&mut self.oversamplers);
        for (voice, oversampler) in voices.filter(|(voice, _)| !voice.is_idle()) {
            match self.state.waveform {
                Waveform::Saw => voice.render::<false>(clean, four, oversampler, &frames_of),
                Waveform::Square => voice.render::<true>(clean, four, oversampler, &frames_of),
            }
        }
        if warms {
            self.down.down(&self.oversampling, four, warm);
            if to == 0.0 {
                self.down = Oversampler::new();
            }
        }
        self.voices.glide(frames, &self.sample_rate);
        let gain_before = self.gain.current();
        let gain_step = (self.gain.advance(frames) - gain_before) / frames as f32;
        let gain = |frame: usize| gain_before + gain_step * (frame + 1) as f32;
        for (frame, sample) in clean.iter_mut().enumerate() {
            *sample *= gain(frame);
        }
        if warms {
            for (frame, sample) in warm.iter_mut().enumerate() {
                *sample *= gain(frame);
            }
        }
    }
}

impl Processor for Synth {
    type Update = SynthState;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION.port())
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.envelope = envelope(&self.state, self.sample_rate);
        self.move_filter(0);
    }

    fn update(&mut self, update: &mut SynthState) {
        let targets = self.state.set_record(update, self.ramp_frames());
        self.aim(&targets);
    }

    fn latency(&self) -> u32 {
        LATENCY
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames()) {
            self.aim(&targets);
        }
        let events = context.event_inputs.get(Self::NOTES);
        let idle = events.is_empty() && self.voices.is_idle();
        if idle && self.quiet >= Oversampler::DELAY_FRAMES {
            return;
        }
        // The synth is one voice bank in the middle. It renders once, the clean voices into the
        // left channel and the warm ones into the right, then delays the clean ones, adds the
        // two and copies that to the right. Where the track puts it is the track's business.
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let warm = self.warmth.current() != 0.0 || self.warmth.is_moving();
        if idle {
            self.quiet += left.len();
        } else {
            self.quiet = 0;
            let mut rendered = 0;
            for timed in events {
                let offset = timed.offset.clamp(rendered, left.len());
                self.render([&mut left[rendered..offset], &mut right[rendered..offset]]);
                rendered = offset;
                self.handle(timed.event);
            }
            self.render([&mut left[rendered..], &mut right[rendered..]]);
        }
        self.late.delay(left);
        if warm {
            left.iter_mut()
                .zip(&*right)
                .for_each(|(clean, warm)| *clean += warm);
        }
        right.copy_from_slice(left);
    }
}
