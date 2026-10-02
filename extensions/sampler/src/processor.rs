//! The sampler processor: 16 voices, each the sample read at the speed of its key through an
//! envelope.
//!
//! The sample arrives in an update as an `Arc<Audio>`, read on the control side. When another
//! one arrives, the voices of the old one fade out over 5 ms while it is kept as the previous
//! sample, and the one before that rides back to the control side in the update. So the audio
//! thread never reads a file and never frees a sample.
//!
//! Everything is allocated with the processor. A sampler with no voice in use returns before it
//! touches its output.

use std::sync::Arc;

use sound_core::{
    AudioOutput, Automated, AutomationInput, Envelope, EnvelopeState, EventInput, MAX_BLOCK, Peaks,
    Ports, PrepareConfig, ProcessContext, Processor, Smoothed, Targets, amplitude,
};
use sound_media::{Audio, SCRATCH_FRAMES, Varispeed, varispeed};
use sound_notes::{NoteEvent, Pitch, Velocity, Voice as _, Voices, Wheels};

use crate::{ATTACK, DECAY, GAIN, RELEASE, SUSTAIN, SamplerState, VELOCITY};

/// The numbers an automation lane can move: every number but the root, a key.
const AUTOMATED: [&crate::Parameter; 6] = [&ATTACK, &DECAY, &SUSTAIN, &RELEASE, &VELOCITY, &GAIN];

type SamplerTargets = Targets<SamplerState, { AUTOMATED.len() }>;

/// Notes that sound at once. One more note takes over a voice, as [`Voices`] picks it. A sample
/// cannot take over in place as the synth's oscillator does: the new note starts at the start
/// of the file, a step from where the old one was. So the voice it takes fades out over 5 ms
/// next to the new note, in one of the slots kept for that.
pub const VOICES: usize = 16;

/// Voices that fade out after they were taken over or their sample was replaced. More than
/// this many at once, within 5 ms, and the new note takes the one closest to silence.
const FADING_VOICES: usize = 8;

const SLOTS: usize = VOICES + FADING_VOICES;

/// How long a voice that was taken over, or whose sample went, takes to fade out.
const FADE_SECONDS: f32 = 0.005;

/// The level before the end of the part of the file that plays ramps to silence over this long,
/// so a sample cut in the middle of its sound does not click. The ramp of an audio clip's edge.
const EDGE_SECONDS: f64 = 0.002;

/// How long the gain takes to reach a new value: the glide of every built-in device.
const GLIDE_SECONDS: f32 = 0.02;

/// What the processor plays from: the record and the sample, read on the control side. Both
/// are swapped in, and what they replace goes back to the control side.
pub struct SamplerUpdate {
    state: SamplerState,
    sample: Option<Arc<Audio>>,
}

impl SamplerUpdate {
    /// The update for a record and the file it names, or no file.
    pub fn new(state: &SamplerState, sample: Option<Arc<Audio>>) -> Self {
        let state = state.clone();
        Self { state, sample }
    }
}

impl Settings {
    /// The numbers of a record for the file it plays, or no file.
    fn new(state: &SamplerState, sample: Option<&Audio>) -> Self {
        let (rate, frames) = sample.map_or((1.0, 0.0), |audio| {
            (f64::from(audio.sample_rate()), audio.frames() as f64)
        });
        let end = state
            .end_seconds
            .map_or(frames, |end| (end * rate).min(frames));
        Self {
            root: state.root,
            start: state.start_seconds * rate,
            end,
            file_rate: rate,
            attack_seconds: state.attack_seconds,
            decay_seconds: state.decay_seconds,
            sustain: state.sustain,
            release_seconds: state.release_seconds,
            velocity_to_volume: state.velocity_to_volume,
            gain: amplitude(state.gain_db),
        }
    }
}

/// The record in the units the audio thread works in: places are frames of the file.
#[derive(Copy, Clone)]
struct Settings {
    root: Pitch,
    start: f64,
    end: f64,
    file_rate: f64,
    attack_seconds: f32,
    decay_seconds: f32,
    sustain: f32,
    release_seconds: f32,
    velocity_to_volume: f32,
    /// Linear.
    gain: f32,
}

/// The envelope of the record: the synth's, from `sound_core`.
fn envelope(settings: &Settings, sample_rate: f32) -> Envelope {
    Envelope::new(
        settings.attack_seconds,
        settings.decay_seconds,
        settings.sustain,
        settings.release_seconds,
        sample_rate,
    )
}

/// Which sample a voice plays.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Source {
    Current,
    /// The one before the current, kept until its voices have faded out.
    Previous,
}

#[derive(Copy, Clone)]
struct Voice {
    source: Source,
    /// Where the voice is in the file, in frames of the file.
    position: f64,
    /// Frames of the file per frame of the engine: the pitch of the note and the rates, before
    /// the wheels move it.
    key_step: f64,
    /// Where the part of the file that plays ends, in frames of the file.
    end: f64,
    /// From the velocity.
    amplitude: f32,
    envelope: EnvelopeState,
    /// From 1 down to 0 while the voice fades out, 1 otherwise.
    fade: f32,
    fading: bool,
}

/// Frames of the file per frame of the engine for a note at `pitch`.
fn key_step(pitch: f32, settings: &Settings, sample_rate: f32) -> f64 {
    let semitones = f64::from(pitch) - f64::from(settings.root.number());
    let rates = settings.file_rate / f64::from(sample_rate.max(1.0));
    (semitones / 12.0).exp2() * rates
}

/// A voice reads the record and the sample rate to start or move.
impl sound_notes::Voice for Voice {
    type Context = (Settings, f32);

    fn is_idle(&self) -> bool {
        self.envelope.is_idle()
    }

    fn loudness(&self) -> f32 {
        self.envelope.level as f32 * self.amplitude * self.fade
    }

    fn start(&mut self, pitch: f32, velocity: Velocity, (settings, sample_rate): &(Settings, f32)) {
        let played = f64::from(velocity.value()) / 127.0;
        let amount = settings.velocity_to_volume;
        *self = Voice {
            source: Source::Current,
            position: settings.start,
            key_step: key_step(pitch, settings, *sample_rate),
            end: settings.end,
            amplitude: 1.0 - amount + amount * (played * played) as f32,
            envelope: EnvelopeState {
                stage: sound_core::EnvelopeStage::Attack,
                level: 0.0,
            },
            fade: 1.0,
            fading: false,
        };
    }

    fn set_pitch(&mut self, pitch: f32, (settings, sample_rate): &(Settings, f32)) {
        self.key_step = key_step(pitch, settings, *sample_rate);
    }

    fn release(&mut self) {
        self.envelope.release();
    }

    /// Fades out over 5 ms.
    fn cut(&mut self) {
        if !self.is_idle() {
            self.fading = true;
        }
    }
}

impl Voice {
    const IDLE: Self = Self {
        source: Source::Current,
        position: 0.0,
        key_step: 1.0,
        end: 0.0,
        amplitude: 0.0,
        envelope: EnvelopeState::IDLE,
        fade: 1.0,
        fading: false,
    };

    /// Adds this voice to `left` and `right`, the frames between two events, at `pitch_ratio`
    /// times the pitch of its key. `frames` holds the sample on its way; `scratch` the file
    /// frames the filter reads.
    fn render(
        &mut self,
        audio: &Audio,
        (left, right): (&mut [f32], &mut [f32]),
        (frames, scratch): (&mut [[f32; 2]], &mut [[f32; 2]]),
        envelope: &Envelope,
        (edge_frames, fade_step): (f64, f32),
        (filter, pitch_ratio): (&Varispeed, f64),
    ) {
        let count = left.len().min(frames.len());
        let frames = &mut frames[..count];
        let step = self.key_step * pitch_ratio;
        filter.render(audio, self.position, step, frames, scratch);
        for (index, ((left, right), frame)) in left
            .iter_mut()
            .zip(right.iter_mut())
            .zip(frames.iter())
            .enumerate()
        {
            // Engine frames from this one to the end of the part that plays.
            let to_end = (self.end - (self.position + step * index as f64)) / step;
            if to_end <= 0.0 {
                self.envelope = EnvelopeState::IDLE;
                break;
            }
            let level = self.envelope.next(envelope) as f32;
            if self.envelope.is_idle() {
                break;
            }
            if self.fading {
                self.fade -= fade_step;
                if self.fade <= 0.0 {
                    self.envelope = EnvelopeState::IDLE;
                    break;
                }
            }
            let edge = (to_end / edge_frames).min(1.0) as f32;
            let gain = level * self.amplitude * self.fade * edge;
            *left += frame[0] * gain;
            *right += frame[1] * gain;
        }
        self.position += step * count as f64;
        if self.is_idle() {
            *self = Self::IDLE;
        }
    }
}

pub struct Sampler {
    /// The record, with the values of the lanes that automate it.
    state: Automated<SamplerState, { AUTOMATED.len() }>,
    /// The record for the current sample.
    settings: Settings,
    current: Option<Arc<Audio>>,
    previous: Option<Arc<Audio>>,
    /// Zero until `prepare` runs.
    sample_rate: f32,
    envelope: Envelope,
    gain: Smoothed,
    /// Of the slots, `VOICES` play at once and the rest are for voices that fade out.
    voices: Voices<Voice, SLOTS>,
    /// The bend and the vibrato of every voice. At rest after every `AllOff`, like the pedal.
    wheels: Wheels,
    filter: &'static Varispeed,
    /// One stretch of one voice on its way into the output.
    frames: Box<[[f32; 2]]>,
    scratch: Box<[[f32; 2]]>,
    /// Where the last note started is in the file, for the card.
    position: Peaks,
}

impl Sampler {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<SamplerState, { AUTOMATED.len() }> =
        AutomationInput::new(1, AUTOMATED);

    /// A sampler with no sample, which the first update gives. `position` is where it says
    /// where its last note is in the file.
    pub fn new(position: Peaks) -> Self {
        let state = SamplerState::default();
        let settings = Settings::new(&state, None);
        Self {
            state: Automated::new(Self::AUTOMATION, state),
            settings,
            current: None,
            previous: None,
            sample_rate: 0.0,
            envelope: Envelope::default(),
            gain: Smoothed::new(settings.gain),
            voices: Voices::new(Voice::IDLE, VOICES),
            wheels: Wheels::default(),
            // Made here, on the control side, so the audio thread only reads it.
            filter: varispeed(),
            frames: vec![[0.0; 2]; MAX_BLOCK].into_boxed_slice(),
            scratch: vec![[0.0; 2]; SCRATCH_FRAMES].into_boxed_slice(),
            position,
        }
    }

    fn handle(&mut self, event: NoteEvent) {
        self.wheels.follow(event);
        let settings = self.settings;
        let playable = self.current.is_some() && settings.start < settings.end;
        if playable || !matches!(event, NoteEvent::On { .. }) {
            self.voices.handle(event, &(settings, self.sample_rate));
        }
    }

    /// Renders the voices into the frames between two events, then the gain over them.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let count = left.len();
        if count == 0 {
            return;
        }
        let edge_frames = (EDGE_SECONDS * f64::from(self.sample_rate)).max(1.0);
        let fade_step = 1.0 / (FADE_SECONDS * self.sample_rate).max(1.0);
        let pitch_ratio = f64::from(self.wheels.pitch_ratio(count, self.sample_rate));
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            let audio = match voice.source {
                Source::Current => self.current.as_deref(),
                Source::Previous => self.previous.as_deref(),
            };
            let Some(audio) = audio else {
                *voice = Voice::IDLE;
                continue;
            };
            voice.render(
                audio,
                (&mut *left, &mut *right),
                (&mut self.frames, &mut self.scratch),
                &self.envelope,
                (edge_frames, fade_step),
                (self.filter, pitch_ratio),
            );
        }
        self.voices.glide(count, &(self.settings, self.sample_rate));
        let gain_before = self.gain.current();
        let gain_step = (self.gain.advance(count) - gain_before) / count as f32;
        for (frame, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let gain = gain_before + gain_step * (frame + 1) as f32;
            *left *= gain;
            *right *= gain;
        }
    }

    /// The frames a change of the gain takes. Zero until `prepare` runs: then it is at once.
    fn ramp_frames(&self) -> f32 {
        GLIDE_SECONDS * self.sample_rate
    }

    /// Works out the record and its lanes for the current sample. The gain glides, the
    /// envelope applies at once, and the rest from the next note.
    fn aim(&mut self, targets: &SamplerTargets) {
        self.settings = Settings::new(&self.state, self.current.as_deref());
        self.envelope = envelope(&self.settings, self.sample_rate);
        self.gain
            .set_target(self.settings.gain, targets.ramp(&GAIN));
        if self.voices.is_idle() {
            // Nothing sounds, so there is nothing to smooth.
            self.gain.snap();
        }
    }

    /// Takes the sample of an update, unless it is the one that plays. The voices of the one
    /// before fade out.
    fn take(&mut self, sample: &mut Option<Arc<Audio>>) {
        let same = match (&self.current, &*sample) {
            (Some(current), Some(new)) => Arc::ptr_eq(current, new),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            match voice.source {
                // A second new sample within 5 ms: these lose theirs, which goes back now.
                Source::Previous => *voice = Voice::IDLE,
                Source::Current => voice.source = Source::Previous,
            }
        }
        self.voices.cut_all();
        // The current sample becomes the previous one, the new one comes in, and the one
        // before goes back to the control side inside the update, to be dropped there.
        std::mem::swap(&mut self.previous, &mut self.current);
        std::mem::swap(&mut self.current, sample);
    }

    /// Where the newest note that still sounds is in its file, for the card.
    fn show_position(&self) {
        // A voice that is not cut plays the current sample.
        if let Some(voice) = self.voices.newest() {
            let seconds = (voice.position / self.settings.file_rate) as f32;
            // A peak of 0 is no peak, so the very first frame of a file shows as the smallest
            // place after it.
            self.position.record(0, seconds.max(f32::MIN_POSITIVE));
        }
    }
}

impl Processor for Sampler {
    type Update = SamplerUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION.port())
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.envelope = envelope(&self.settings, self.sample_rate);
    }

    fn update(&mut self, update: &mut SamplerUpdate) {
        let targets = self.state.set_record(&mut update.state, self.ramp_frames());
        self.take(&mut update.sample);
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
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let mut rendered = 0;
        for timed in events {
            let offset = timed.offset.clamp(rendered, left.len());
            self.render(&mut left[rendered..offset], &mut right[rendered..offset]);
            rendered = offset;
            self.handle(timed.event);
        }
        self.render(&mut left[rendered..], &mut right[rendered..]);
        self.show_position();
    }
}
