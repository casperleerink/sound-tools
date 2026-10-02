//! The synth processor: 16 voices, each an oscillator into a low-pass filter into an envelope.
//!
//! Everything is allocated with the processor. A voice that is not in use costs one comparison
//! per block, and a synth with no voice in use returns before it touches its output.

use std::f32::consts::{PI, SQRT_2};

use sound_core::{
    AudioOutput, Automated, AutomationInput, Envelope, EnvelopeState, EventInput,
    HIGHEST_PHASE_STEP, Ports, PrepareConfig, ProcessContext, Processor, Smoothed, Targets,
    poly_blep,
};
use sound_notes::{NoteEvent, Velocity, Voice as _, Voices, Wheels, frequency_hz};

use crate::{CUTOFF, GAIN, PARAMETERS, RESONANCE, SynthState, Waveform};

/// Every number of the synth can be automated.
type SynthTargets = Targets<SynthState, { PARAMETERS.len() }>;

/// Notes that sound at once. One more note takes over a voice in place, as [`Voices`] picks it.
pub const VOICES: usize = 16;

/// How long gain, cutoff and resonance take to reach a new value.
const RAMP_SECONDS: f32 = 0.05;

/// The filter peak at resonance 1, as a Q factor. Resonance 0 is Q 0.707, which has no peak.
const HIGHEST_Q: f32 = 16.0;

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

/// A state variable low-pass filter in the trapezoidal form (Simper, "Solving the continuous
/// SVF equations using trapezoidal integration"). It stays stable when the cutoff moves fast,
/// and all voices share one set of factors.
///
/// Simper's steps are multiplied out here, so the next state is one sum of products of the
/// old state and the input. The result is the same. The chain of operations from one frame to
/// the next is half as long, and that chain is what limits the speed of a voice.
#[derive(Default)]
struct FilterFactors {
    /// The next `[ic1, ic2]` from `[ic1, ic2, input]`.
    next_state: [[f32; 3]; 2],
    /// The low-pass output from `[ic1, ic2, input]`.
    output: [f32; 3],
}

impl FilterFactors {
    fn new(cutoff_hz: f32, resonance: f32, sample_rate: f32) -> Self {
        // Below Nyquist with a margin, also at low sample rates.
        let cutoff_hz = cutoff_hz.min(0.45 * sample_rate);
        let g = (PI * cutoff_hz / sample_rate).tan();
        // 1/Q, from 1/0.707 at resonance 0 to 1/HIGHEST_Q at resonance 1, in even ratios.
        let k = SQRT_2 * (1.0 / (SQRT_2 * HIGHEST_Q)).powf(resonance);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        // The peak adds level: a partial on the cutoff comes out Q times louder. The input is
        // turned down by the square root of that, so resonance does not overload the output.
        let input = (k / SQRT_2).sqrt();
        Self {
            next_state: [
                [2.0 * a1 - 1.0, -2.0 * a2, 2.0 * a2 * input],
                [2.0 * a2, 1.0 - 2.0 * a3, 2.0 * a3 * input],
            ],
            output: [a2, 1.0 - a3, a3 * input],
        }
    }
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
    filter_state: [f32; 2],
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
            self.filter_state = [0.0; 2];
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
    const IDLE: Self = Self {
        phase: 0.0,
        key_step: 0.0,
        amplitude: 0.0,
        envelope: EnvelopeState::IDLE,
        filter_state: [0.0; 2],
    };

    /// Adds this voice to `output`, at `pitch_ratio` times the pitch of its key. `SQUARE` picks
    /// the waveform at compile time, so the frame loop has no waveform branch.
    fn render<const SQUARE: bool>(
        &mut self,
        output: &mut [f32],
        filter: &FilterFactors,
        envelope: &Envelope,
        pitch_ratio: f32,
    ) {
        let phase_step = (self.key_step * pitch_ratio).min(HIGHEST_PHASE_STEP);
        let [mut ic1, mut ic2] = self.filter_state;
        for sample in output {
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

            let mix = |[a, b, c]: [f32; 3]| (a * ic1 + b * ic2) + c * oscillator;
            let filtered = mix(filter.output);
            [ic1, ic2] = filter.next_state.map(mix);

            // An idle voice ended on the frame before: nothing of it is added any more.
            if self.envelope.is_idle() {
                break;
            }
            let level = self.envelope.next(envelope) as f32;
            *sample += filtered * level * self.amplitude;
        }
        self.filter_state = [ic1, ic2];
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
    filter: FilterFactors,
    gain: Smoothed,
    voices: Voices<Voice, VOICES>,
    /// The bend and the vibrato of every voice. At rest after every `AllOff`, like the pedal.
    wheels: Wheels,
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
            filter: FilterFactors::default(),
            gain: Smoothed::new(state.gain),
            voices: Voices::new(Voice::IDLE, VOICES),
            wheels: Wheels::default(),
        }
    }

    fn handle(&mut self, event: NoteEvent) {
        self.wheels.follow(event);
        self.voices.handle(event, &self.sample_rate);
    }

    /// Moves cutoff and resonance along their ramps and works out the filter for where they are.
    fn move_filter(&mut self, frames: usize) {
        self.filter = FilterFactors::new(
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
        let idle = self.voices.is_idle();
        if idle {
            // Nothing sounds, so there is nothing to smooth. The next note starts on the new values.
            self.cutoff_octaves.snap();
            self.resonance.snap();
            self.gain.snap();
        }
        // A filter that took its value at once does not move, so nothing else works it out.
        if idle || targets.snaps() {
            self.move_filter(0);
        }
    }

    /// Renders the frames between two events.
    fn render(&mut self, output: &mut [f32]) {
        let frames = output.len();
        if frames == 0 {
            return;
        }
        if self.cutoff_octaves.is_moving() || self.resonance.is_moving() {
            self.move_filter(frames);
        }
        let filter = &self.filter;
        let pitch_ratio = self.wheels.pitch_ratio(frames, self.sample_rate);
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            let envelope = &self.envelope;
            match self.state.waveform {
                Waveform::Saw => voice.render::<false>(output, filter, envelope, pitch_ratio),
                Waveform::Square => voice.render::<true>(output, filter, envelope, pitch_ratio),
            }
        }
        self.voices.glide(frames, &self.sample_rate);
        let gain_before = self.gain.current();
        let gain_step = (self.gain.advance(frames) - gain_before) / frames as f32;
        for (frame, sample) in output.iter_mut().enumerate() {
            *sample *= gain_before + gain_step * (frame + 1) as f32;
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

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames()) {
            self.aim(&targets);
        }
        let events = context.event_inputs.get(Self::NOTES);
        if events.is_empty() && self.voices.is_idle() {
            return;
        }
        // The synth is one voice bank in the middle. It renders once, into the left channel,
        // and copies that to the right. Where the track puts it is the track's business.
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let mut rendered = 0;
        for timed in events {
            let offset = timed.offset.clamp(rendered, left.len());
            self.render(&mut left[rendered..offset]);
            rendered = offset;
            self.handle(timed.event);
        }
        self.render(&mut left[rendered..]);
        right.copy_from_slice(left);
    }
}
