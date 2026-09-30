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
    AudioOutput, Envelope, EnvelopeState, EventInput, MAX_BLOCK, Peaks, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed,
};
use sound_media::{Audio, SCRATCH_FRAMES, Varispeed, varispeed};
use sound_notes::{NoteEvent, Pedal, Pitch, Velocity, Wheels};

use crate::SamplerState;

/// Notes that sound at once. One more note takes over a voice: the quietest released one, or
/// the oldest held one when none is released. The voice it takes fades out over 5 ms next to
/// the new note, in one of the slots kept for that.
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

/// What the processor plays from: the numbers of the record and the sample, read on the control
/// side. The sample is swapped in, and what it held before goes back to the control side.
pub struct SamplerUpdate {
    settings: Settings,
    sample: Option<Arc<Audio>>,
}

impl SamplerUpdate {
    /// The update for a record and the file it names, or no file.
    pub fn new(state: &SamplerState, sample: Option<Arc<Audio>>) -> Self {
        let (rate, frames) = sample.as_ref().map_or((1.0, 0.0), |audio| {
            (f64::from(audio.sample_rate()), audio.frames() as f64)
        });
        let end = state
            .end_seconds
            .map_or(frames, |end| (end * rate).min(frames));
        let settings = Settings {
            root: state.root,
            start: state.start_seconds * rate,
            end,
            file_rate: rate,
            attack_seconds: state.attack_seconds,
            decay_seconds: state.decay_seconds,
            sustain: state.sustain,
            release_seconds: state.release_seconds,
            velocity_to_volume: state.velocity_to_volume,
            gain: 10_f32.powf(state.gain_db / 20.0),
        };
        Self { settings, sample }
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
    /// The key of this note is up and only the sustain pedal keeps it sounding.
    sustained: bool,
    pitch: Option<Pitch>,
    /// The count of the note on that started this voice. The lowest is the oldest.
    started: u64,
    source: Source,
    /// Where the voice is in the file, in frames of the file.
    position: f64,
    /// Frames of the file per frame of the engine: the pitch of the key and the rates, before
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

impl Voice {
    const IDLE: Self = Self {
        sustained: false,
        pitch: None,
        started: 0,
        source: Source::Current,
        position: 0.0,
        key_step: 1.0,
        end: 0.0,
        amplitude: 0.0,
        envelope: EnvelopeState::IDLE,
        fade: 1.0,
        fading: false,
    };

    fn is_idle(&self) -> bool {
        self.envelope.is_idle()
    }

    /// A voice that counts against the 16: it sounds and is not fading out.
    fn is_sounding(&self) -> bool {
        !self.is_idle() && !self.fading
    }

    fn is_held(&self) -> bool {
        self.envelope.is_held()
    }

    fn loudness(&self) -> f32 {
        self.envelope.level as f32 * self.amplitude * self.fade
    }

    fn release(&mut self) {
        self.sustained = false;
        self.envelope.release();
    }

    /// The key came up. With the pedal down the note sounds on until the pedal comes up.
    fn key_up(&mut self, pedal_is_down: bool) {
        if pedal_is_down && self.is_held() {
            self.sustained = true;
        } else {
            self.release();
        }
    }

    fn fade_out(&mut self) {
        if !self.is_idle() {
            self.fading = true;
        }
    }

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
    settings: Settings,
    current: Option<Arc<Audio>>,
    previous: Option<Arc<Audio>>,
    /// Zero until `prepare` runs.
    sample_rate: f32,
    envelope: Envelope,
    gain: Smoothed,
    voices: [Voice; SLOTS],
    notes_started: u64,
    pedal: Pedal,
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

    /// A sampler with no sample, which the first update gives. `position` is where it says
    /// where its last note is in the file.
    pub fn new(position: Peaks) -> Self {
        let settings = SamplerUpdate::new(&SamplerState::default(), None).settings;
        Self {
            settings,
            current: None,
            previous: None,
            sample_rate: 0.0,
            envelope: Envelope::default(),
            gain: Smoothed::new(settings.gain),
            voices: [Voice::IDLE; SLOTS],
            notes_started: 0,
            pedal: Pedal::UP,
            wheels: Wheels::default(),
            // Made here, on the control side, so the audio thread only reads it.
            filter: varispeed(),
            frames: vec![[0.0; 2]; MAX_BLOCK].into_boxed_slice(),
            scratch: vec![[0.0; 2]; SCRATCH_FRAMES].into_boxed_slice(),
            position,
        }
    }

    fn any_voice(&self) -> bool {
        self.voices.iter().any(|voice| !voice.is_idle())
    }

    fn handle(&mut self, event: NoteEvent) {
        self.wheels.follow(event);
        match event {
            NoteEvent::On { pitch, velocity } => self.start(pitch, velocity),
            NoteEvent::Off { pitch } => {
                let pedal_is_down = self.pedal.is_down();
                let of_this_pitch = self
                    .voices
                    .iter_mut()
                    .filter(|voice| voice.pitch == Some(pitch));
                of_this_pitch.for_each(|voice| voice.key_up(pedal_is_down));
            }
            NoteEvent::Pedal(value) => {
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
            // The wheels followed above, and the pressure does nothing here.
            NoteEvent::Bend(_) | NoteEvent::ModWheel(_) | NoteEvent::Pressure(_) => {}
        }
    }

    fn start(&mut self, pitch: Pitch, velocity: Velocity) {
        let settings = self.settings;
        if self.current.is_none() || settings.start >= settings.end {
            return;
        }
        let sounding = self.voices.iter().filter(|voice| voice.is_sounding());
        if sounding.count() >= VOICES
            && let Some(taken) = self.voice_to_take_over()
        {
            self.voices[taken].fade_out();
        }
        let slot = self.free_slot();
        self.notes_started += 1;
        let semitones = f64::from(pitch.number()) - f64::from(settings.root.number());
        let rates = settings.file_rate / f64::from(self.sample_rate.max(1.0));
        let played = f64::from(velocity.value()) / 127.0;
        let amount = settings.velocity_to_volume;
        self.voices[slot] = Voice {
            sustained: false,
            pitch: Some(pitch),
            started: self.notes_started,
            source: Source::Current,
            position: settings.start,
            key_step: (semitones / 12.0).exp2() * rates,
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

    /// The sounding voice a note takes over: the quietest released one, or else the oldest.
    fn voice_to_take_over(&self) -> Option<usize> {
        let sounding = || {
            let voices = self.voices.iter().enumerate();
            voices.filter(|(_, voice)| voice.is_sounding())
        };
        let released = sounding().filter(|(_, voice)| !voice.is_held());
        let quietest = released.min_by(|(_, a), (_, b)| a.loudness().total_cmp(&b.loudness()));
        let oldest = || sounding().min_by_key(|(_, voice)| voice.started);
        quietest.or_else(oldest).map(|(index, _)| index)
    }

    /// An idle slot, or the fading voice closest to silence.
    fn free_slot(&self) -> usize {
        let voices = self.voices.iter().enumerate();
        let idle = voices.clone().find(|(_, voice)| voice.is_idle());
        let quietest = || {
            voices
                .filter(|(_, voice)| voice.fading)
                .min_by(|(_, a), (_, b)| a.loudness().total_cmp(&b.loudness()))
        };
        idle.or_else(quietest).map_or(0, |(index, _)| index)
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
        let gain_before = self.gain.current();
        let gain_step = (self.gain.advance(count) - gain_before) / count as f32;
        for (frame, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let gain = gain_before + gain_step * (frame + 1) as f32;
            *left *= gain;
            *right *= gain;
        }
    }

    /// Where the newest note that still sounds is in its file, for the card.
    fn show_position(&self) {
        let newest = self
            .voices
            .iter()
            .filter(|voice| voice.is_sounding() && voice.source == Source::Current)
            .max_by_key(|voice| voice.started);
        if let Some(voice) = newest {
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
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.envelope = envelope(&self.settings, self.sample_rate);
    }

    fn update(&mut self, update: &mut SamplerUpdate) {
        self.settings = update.settings;
        self.envelope = envelope(&self.settings, self.sample_rate);
        self.gain
            .set_target(self.settings.gain, GLIDE_SECONDS * self.sample_rate);
        if !self.any_voice() {
            // Nothing sounds, so there is nothing to smooth.
            self.gain.snap();
        }
        let same = match (&self.current, &update.sample) {
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
                Source::Current => {
                    voice.source = Source::Previous;
                    voice.fade_out();
                }
            }
        }
        // The current sample becomes the previous one, the new one comes in, and the one
        // before goes back to the control side inside the update, to be dropped there.
        std::mem::swap(&mut self.previous, &mut self.current);
        std::mem::swap(&mut self.current, &mut update.sample);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let events = context.event_inputs.get(Self::NOTES);
        if events.is_empty() && !self.any_voice() {
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
