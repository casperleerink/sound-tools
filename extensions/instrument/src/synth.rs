//! The synth processor: 16 voices, each an oscillator into a low-pass filter into an envelope.
//!
//! Everything is allocated with the processor. A voice that is not in use costs one comparison
//! per block, and a synth with no voice in use returns before it touches its output.

use std::f32::consts::{PI, SQRT_2};

use sound_core::{
    AudioOutput, EventInput, Ports, PrepareConfig, ProcessContext, Processor, Smoothed,
};
use sound_notes::{NoteEvent, Pedal, Pitch, Velocity};

use crate::{SynthState, Waveform};

/// Notes that sound at once. One more note takes over a voice: the quietest released one, or
/// the oldest held one when none is released.
pub const VOICES: usize = 16;

/// How long gain, cutoff and resonance take to reach a new value.
const RAMP_SECONDS: f32 = 0.05;

/// The filter peak at resonance 1, as a Q factor. Resonance 0 is Q 0.707, which has no peak.
const HIGHEST_Q: f32 = 16.0;

/// A new voice starts here in its cycle. Both waveforms are far from a step at this phase, so
/// the very first frame of a note already gives output.
const START_PHASE: f32 = 0.25;

/// Above this a cycle is about two frames and the waveform corrections overlap.
const HIGHEST_PHASE_STEP: f32 = 0.45;

/// How far past full level the attack aims. It shapes the attack curve, and lets it reach full
/// level in exactly the attack time.
const ATTACK_OVERSHOOT: f32 = 0.3;

/// -60 dB. The release aims this far below silence, and so reaches silence in exactly the
/// release time. The decay is within this of the sustain level after the decay time. A held
/// note with no sustain ends when it falls below this.
const ENVELOPE_FLOOR: f32 = 0.001;

/// The envelope of every voice, as per-frame factors. Each stage is `level * coefficient +
/// base`: a curve toward a point a little past the target.
#[derive(Default)]
struct Envelope {
    attack_coefficient: f32,
    attack_base: f32,
    decay_coefficient: f32,
    sustain: f32,
    release_coefficient: f32,
    release_base: f32,
}

impl Envelope {
    fn new(state: &SynthState, sample_rate: f32) -> Self {
        // The factor that covers a distance of 1 in `seconds`, when the curve aims `overshoot`
        // past the end of that distance.
        let coefficient = |seconds: f32, overshoot: f32| {
            let frames = (seconds * sample_rate).max(1.0);
            (-((1.0 + overshoot) / overshoot).ln() / frames).exp()
        };
        let attack_coefficient = coefficient(state.attack_seconds, ATTACK_OVERSHOOT);
        let release_coefficient = coefficient(state.release_seconds, ENVELOPE_FLOOR);
        Self {
            attack_coefficient,
            attack_base: (1.0 + ATTACK_OVERSHOOT) * (1.0 - attack_coefficient),
            decay_coefficient: coefficient(state.decay_seconds, ENVELOPE_FLOOR),
            sustain: state.sustain,
            release_coefficient,
            release_base: -ENVELOPE_FLOOR * (1.0 - release_coefficient),
        }
    }
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

#[derive(Copy, Clone, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    /// Decay, and the sustain it ends in. One stage, so a sustain edit on a held note glides.
    Decay,
    Release,
}

#[derive(Copy, Clone)]
struct Voice {
    stage: Stage,
    /// The key of this note is up and only the sustain pedal keeps it sounding. It is released
    /// when the pedal comes up.
    sustained: bool,
    /// None until the first note.
    pitch: Option<Pitch>,
    /// The count of the note on that started this voice. The lowest is the oldest.
    started: u64,
    /// In cycles, from 0 to 1.
    phase: f32,
    phase_step: f32,
    /// From the velocity.
    amplitude: f32,
    /// The envelope, from 0 to 1.
    level: f32,
    filter_state: [f32; 2],
}

/// The naive sawtooth steps from 1 to -1 where the phase wraps. This is what to subtract near
/// that step to round it off over two frames (PolyBLEP), which removes most of the aliasing.
fn step_correction(phase: f32, phase_step: f32) -> f32 {
    if phase < phase_step {
        let t = phase / phase_step;
        t + t - t * t - 1.0
    } else if phase > 1.0 - phase_step {
        let t = (phase - 1.0) / phase_step;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

fn sawtooth(phase: f32, phase_step: f32) -> f32 {
    2.0 * phase - 1.0 - step_correction(phase, phase_step)
}

impl Voice {
    const IDLE: Self = Self {
        stage: Stage::Idle,
        sustained: false,
        pitch: None,
        started: 0,
        phase: 0.0,
        phase_step: 0.0,
        amplitude: 0.0,
        level: 0.0,
        filter_state: [0.0; 2],
    };

    fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    fn loudness(&self) -> f32 {
        self.level * self.amplitude
    }

    fn is_held(&self) -> bool {
        matches!(self.stage, Stage::Attack | Stage::Decay)
    }

    fn release(&mut self) {
        self.sustained = false;
        if self.is_held() {
            self.stage = Stage::Release;
        }
    }

    /// The key came up. With the pedal down the note sounds on until the pedal comes up.
    fn key_up(&mut self, pedal_is_down: bool) {
        if pedal_is_down && self.is_held() {
            self.sustained = true;
        } else {
            self.release();
        }
    }

    fn start(&mut self, pitch: Pitch, velocity: Velocity, started: u64, sample_rate: f32) {
        self.sustained = false;
        let amplitude = (f32::from(velocity.value()) / 127.0).powi(2);
        if self.is_idle() {
            self.phase = START_PHASE;
            self.filter_state = [0.0; 2];
            self.level = 0.0;
        } else {
            // A voice taken from another note keeps its phase and filter state, and its
            // loudness (level times amplitude), so the takeover is not a click. A quiet note
            // that takes over a loud voice starts above full level, and decays from there.
            self.level = self.level * self.amplitude / amplitude;
        }
        self.stage = if self.level < 1.0 {
            Stage::Attack
        } else {
            Stage::Decay
        };
        self.pitch = Some(pitch);
        self.started = started;
        self.amplitude = amplitude;
        self.phase_step = (pitch.frequency_hz() / sample_rate).min(HIGHEST_PHASE_STEP);
    }

    /// Adds this voice to `output`. `SQUARE` picks the waveform at compile time, so the frame
    /// loop has no waveform branch.
    fn render<const SQUARE: bool>(
        &mut self,
        output: &mut [f32],
        filter: &FilterFactors,
        envelope: &Envelope,
    ) {
        let [mut ic1, mut ic2] = self.filter_state;
        for sample in output {
            let mut oscillator = sawtooth(self.phase, self.phase_step);
            if SQUARE {
                // A square is a sawtooth minus the same sawtooth half a cycle later.
                let half_later = self.phase + 0.5;
                let half_later = half_later - half_later.floor();
                oscillator -= sawtooth(half_later, self.phase_step);
            }
            self.phase += self.phase_step;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }

            let mix = |[a, b, c]: [f32; 3]| (a * ic1 + b * ic2) + c * oscillator;
            let filtered = mix(filter.output);
            [ic1, ic2] = filter.next_state.map(mix);

            match self.stage {
                Stage::Attack => {
                    self.level = self.level * envelope.attack_coefficient + envelope.attack_base;
                    if self.level >= 1.0 {
                        self.level = 1.0;
                        self.stage = Stage::Decay;
                    }
                }
                Stage::Decay => {
                    let above = self.level - envelope.sustain;
                    self.level = envelope.sustain + above * envelope.decay_coefficient;
                    // A pluck: with no sustain a held note ends here, and not at its note off.
                    if self.level < ENVELOPE_FLOOR && envelope.sustain < ENVELOPE_FLOOR {
                        self.level = 0.0;
                        self.stage = Stage::Idle;
                    }
                }
                Stage::Release => {
                    self.level = self.level * envelope.release_coefficient + envelope.release_base;
                    if self.level <= 0.0 {
                        self.level = 0.0;
                        self.stage = Stage::Idle;
                    }
                }
                Stage::Idle => break,
            }
            *sample += filtered * self.level * self.amplitude;
        }
        self.filter_state = [ic1, ic2];
    }
}

pub struct Synth {
    state: SynthState,
    /// Zero until `prepare` runs.
    sample_rate: f32,
    envelope: Envelope,
    /// In octaves, the base 2 logarithm of the frequency, so a ramp is even to the ear.
    cutoff_octaves: Smoothed,
    resonance: Smoothed,
    /// For the current cutoff and resonance. The tangent in it is worth keeping between blocks.
    filter: FilterFactors,
    gain: Smoothed,
    voices: [Voice; VOICES],
    notes_started: u64,
    /// Where the sustain pedal stands. Up after every `AllOff`, so a stop, a seek or an edit
    /// can leave no note hanging under a pedal nobody will lift.
    pedal: Pedal,
}

impl Synth {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub fn new(state: SynthState) -> Self {
        Self {
            state,
            sample_rate: 0.0,
            envelope: Envelope::default(),
            cutoff_octaves: Smoothed::new(state.cutoff_hz.log2()),
            resonance: Smoothed::new(state.resonance),
            filter: FilterFactors::default(),
            gain: Smoothed::new(state.gain),
            voices: [Voice::IDLE; VOICES],
            notes_started: 0,
            pedal: Pedal::UP,
        }
    }

    /// Voices that sound, held or in their release.
    fn active_voices(&self) -> usize {
        VOICES - self.voices.iter().filter(|voice| voice.is_idle()).count()
    }

    fn handle(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::On { pitch, velocity } => {
                self.notes_started += 1;
                let (started, sample_rate) = (self.notes_started, self.sample_rate);
                self.voice_for_a_new_note()
                    .start(pitch, velocity, started, sample_rate);
            }
            NoteEvent::Off { pitch } => {
                let of_this_pitch = |voice: &&mut Voice| voice.pitch == Some(pitch);
                let pedal_is_down = self.pedal.is_down();
                self.voices
                    .iter_mut()
                    .filter(of_this_pitch)
                    .for_each(|voice| voice.key_up(pedal_is_down));
            }
            NoteEvent::Pedal(value) => {
                // Half pedal is kept as it was played but not acted on: this synth has one
                // damper. A piano plugin that knows more gets the value it was given.
                if self.pedal.is_down() && !value.is_down() {
                    let sustained = self.voices.iter_mut().filter(|voice| voice.sustained);
                    sustained.for_each(Voice::release);
                }
                self.pedal = value;
            }
            NoteEvent::AllOff => {
                self.pedal = Pedal::UP;
                self.voices.iter_mut().for_each(Voice::release);
            }
        }
    }

    /// Where the pedal stands. For tests and for an interface that shows it.
    pub fn pedal(&self) -> Pedal {
        self.pedal
    }

    fn voice_for_a_new_note(&mut self) -> &mut Voice {
        let voices = &self.voices;
        let idle = voices.iter().position(Voice::is_idle);
        let quietest_released = || {
            let released = voices
                .iter()
                .enumerate()
                .filter(|(_, voice)| !voice.is_held());
            released
                .min_by(|(_, a), (_, b)| a.loudness().total_cmp(&b.loudness()))
                .map(|(index, _)| index)
        };
        let oldest = || {
            let oldest = voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| voice.started);
            oldest.map_or(0, |(index, _)| index)
        };
        let index = idle.or_else(quietest_released).unwrap_or_else(oldest);
        &mut self.voices[index]
    }

    /// Moves cutoff and resonance along their ramps and works out the filter for where they are.
    fn move_filter(&mut self, frames: usize) {
        self.filter = FilterFactors::new(
            self.cutoff_octaves.advance(frames).exp2(),
            self.resonance.advance(frames),
            self.sample_rate,
        );
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
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            match self.state.waveform {
                Waveform::Saw => voice.render::<false>(output, filter, &self.envelope),
                Waveform::Square => voice.render::<true>(output, filter, &self.envelope),
            }
        }
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
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.envelope = Envelope::new(&self.state, self.sample_rate);
        self.move_filter(0);
    }

    fn update(&mut self, update: &mut SynthState) {
        self.state = *update;
        self.envelope = Envelope::new(&self.state, self.sample_rate);
        let ramp_frames = RAMP_SECONDS * self.sample_rate;
        self.cutoff_octaves
            .set_target(self.state.cutoff_hz.log2(), ramp_frames);
        self.resonance.set_target(self.state.resonance, ramp_frames);
        self.gain.set_target(self.state.gain, ramp_frames);
        if self.active_voices() == 0 {
            // Nothing sounds, so there is nothing to smooth. The next note starts on the new values.
            self.cutoff_octaves.snap();
            self.resonance.snap();
            self.gain.snap();
            self.move_filter(0);
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let events = context.event_inputs.get(Self::NOTES);
        if events.is_empty() && self.active_voices() == 0 {
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
