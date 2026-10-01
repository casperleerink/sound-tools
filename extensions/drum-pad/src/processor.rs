//! The Drum pad processor: it plays the sound of a pad, from memory, on each note of that pad.
//!
//! The rules that shape this file:
//!
//! - Nothing is made or read here. The sound of each pad arrives ready, in the kit.
//! - Nothing is freed here. A voice holds its sound by an `Arc`. When a voice ends, its sound is
//!   either still the sound of its pad, which the processor holds too, so letting go of it only
//!   counts down; or it was replaced by an edit, and it goes into a graveyard that the next kit
//!   takes back to the control thread. At most [`VOICES`] sounds can wait there: only a voice
//!   that sounded when a kit arrived can hold a replaced sound, and each kit empties it. What
//!   waits there is a render, at most 10 s of sound, never the file of a sample, so a sound
//!   that waits for the next edit costs little.
//! - One voice per pad sounds at a time: a new hit of a pad fades its last one out over
//!   [`FADE_SECONDS`], and so does a hit of a pad of the choke group for every other pad of it.
//!   Fading voices are the rest of the voices. Nothing stops with a step, so nothing clicks.
//! - Volume and pan glide over [`RAMP_SECONDS`], also while a pad sounds. Sound, pitch and decay
//!   apply from the next hit: a hit is one strike, rendered as it was when it was struck.

use std::sync::Arc;

use sound_core::{
    AudioOutput, Automated, AutomationInput, EventInput, Peaks, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Targets,
};
use sound_notes::{NoteEvent, Velocity};

use crate::sounds::Rendered;
use crate::{AUTOMATED, DrumPadState, PAD_LANES, PADS, Pad, pad_of};

type DrumTargets = Targets<DrumPadState, { AUTOMATED.len() }>;

/// Voices that sound at once: one per pad, and the rest for the ones that fade out.
pub const VOICES: usize = 32;

/// How long a voice takes to fade out when it is cut: by a new hit of its pad, by a hit of its
/// choke group, or when the transport stops or jumps.
pub const FADE_SECONDS: f32 = 0.005;

/// How long a change of volume or pan takes to arrive.
pub const RAMP_SECONDS: f32 = 0.02;

/// The gain of each channel for a pad, left first: its volume, and its pan with the pan law of
/// a track (equal power, the middle exactly 1), so a pad keeps its loudness wherever it is.
pub fn pad_gains(pad: &Pad) -> [f32; 2] {
    gains_at(level_of(pad), pad.pan)
}

/// The factor of the volume of a pad.
fn level_of(pad: &Pad) -> f32 {
    10.0_f64.powf(f64::from(pad.volume_db) / 20.0) as f32
}

fn gains_at(level: f32, pan: f32) -> [f32; 2] {
    sound_core::pan_gains(f64::from(level), pan.clamp(-1.0, 1.0))
}

/// One pad as the audio thread plays it. Its volume and pan are in the record of the kit.
pub struct PadPlay {
    /// `None` when the pad is silent, such as a sample pad whose file is not there.
    sound: Option<Arc<Rendered>>,
    /// Frames from the hit to silence.
    decay_frames: usize,
    choke: bool,
}

impl PadPlay {
    pub(crate) fn new(pad: &Pad, sound: Option<Arc<Rendered>>, rate: u32) -> Self {
        Self {
            sound,
            decay_frames: (f64::from(pad.decay_ms) / 1000.0 * f64::from(rate)).round() as usize,
            choke: pad.choke,
        }
    }

    const SILENT: Self = Self {
        sound: None,
        decay_frames: 0,
        choke: false,
    };
}

/// Every pad and the record, and room for what the audio thread gives back.
pub struct Kit {
    pads: [PadPlay; PADS],
    state: DrumPadState,
    /// Sounds that voices held after an edit replaced them, going back to be let go of.
    returned: Vec<Arc<Rendered>>,
}

/// What a [`DrumPad`] gets: every pad from its behaviour, or one hit from an interface.
pub enum DrumUpdate {
    Kit(Box<Kit>),
    /// Plays a pad now, as a click on it does. Not an edit.
    Hit {
        pad: usize,
        velocity: Velocity,
    },
}

impl DrumUpdate {
    pub(crate) fn kit(pads: [PadPlay; PADS], state: &DrumPadState) -> Self {
        Self::Kit(Box::new(Kit {
            pads,
            state: state.clone(),
            returned: Vec::with_capacity(VOICES),
        }))
    }

    /// Plays pad `pad`, 0 to 15, as a note of it at `velocity` would.
    pub fn hit(pad: usize, velocity: Velocity) -> Self {
        Self::Hit { pad, velocity }
    }
}

struct Voice {
    sound: Arc<Rendered>,
    pad: usize,
    /// The next frame of the sound.
    position: usize,
    /// Where it ends: the end of the sound or of the decay.
    end: usize,
    /// The decay of the pad when it was struck, in frames, for the fade over it.
    decay_frames: f32,
    /// The level of the hit, from its velocity.
    velocity: f32,
    /// Frames left of a fade out, when it is fading.
    fading: Option<u32>,
}

impl Voice {
    /// The level at `position`: the hit, the fade over the decay, and a fade out.
    fn level(&self, position: usize, fade_frames: f32) -> f32 {
        let through = position as f32 / self.decay_frames;
        let decay = 1.0 - through * through * through * through;
        let fade = self.fading.map_or(1.0, |left| left as f32 / fade_frames);
        self.velocity * decay.max(0.0) * fade
    }
}

pub struct DrumPad {
    /// The record, with the values of the lanes that automate it.
    state: Automated<DrumPadState, { AUTOMATED.len() }>,
    pads: [PadPlay; PADS],
    /// The factor of the volume and the pan of each pad. They glide apart, each in its own ramp,
    /// and the gains of a pad are worked out from where both are at the end of each stretch.
    levels: [Smoothed; PADS],
    pans: [Smoothed; PADS],
    /// The gains of each pad at the end of the last stretch, where the next starts.
    gains: [[f32; 2]; PADS],
    voices: [Option<Voice>; VOICES],
    /// Replaced sounds that voices held, waiting for the next kit to take them back.
    graveyard: [Option<Arc<Rendered>>; VOICES],
    /// How loud each pad sounds, for the card.
    peaks: [Peaks; PADS],
    fade_frames: u32,
    ramp_frames: f32,
}

impl DrumPad {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<DrumPadState, { AUTOMATED.len() }> =
        AutomationInput::new(1, AUTOMATED);

    pub fn new(peaks: [Peaks; PADS]) -> Self {
        Self {
            state: Automated::new(Self::AUTOMATION, DrumPadState::default()),
            pads: [PadPlay::SILENT; PADS],
            levels: std::array::from_fn(|_| Smoothed::new(0.0)),
            pans: std::array::from_fn(|_| Smoothed::new(0.0)),
            gains: [[0.0; 2]; PADS],
            voices: std::array::from_fn(|_| None),
            graveyard: std::array::from_fn(|_| None),
            peaks,
            fade_frames: 1,
            ramp_frames: 1.0,
        }
    }

    /// Voices that sound, fading or not. For tests.
    pub fn sounding(&self) -> usize {
        self.voices.iter().flatten().count()
    }

    fn is_idle(&self) -> bool {
        self.voices.iter().all(Option::is_none)
    }

    /// Aims the volume and the pan of every pad at the record and its lanes, each in its own
    /// ramp.
    fn aim(&mut self, targets: &DrumTargets) {
        let numbers = self.levels.iter_mut().zip(&mut self.pans);
        let pads = numbers.zip(&self.state.pads).zip(&PAD_LANES);
        for (((level, pan), pad), [volume_lane, pan_lane]) in pads {
            level.set_target(level_of(pad), targets.ramp(volume_lane));
            pan.set_target(pad.pan, targets.ramp(pan_lane));
        }
        // Nothing sounds, so there is nothing to glide: the next hit starts on the new values.
        if self.is_idle() || targets.snaps() {
            self.snap();
        }
    }

    /// Takes the volume and the pan of every pad at once.
    fn snap(&mut self) {
        let numbers = self.levels.iter_mut().zip(&mut self.pans);
        for ((level, pan), gains) in numbers.zip(&mut self.gains) {
            level.snap();
            pan.snap();
            *gains = gains_at(level.current(), pan.current());
        }
    }

    fn fade(voice: &mut Voice, fade_frames: u32) {
        if voice.fading.is_none() {
            voice.fading = Some(fade_frames);
        }
    }

    /// Lets go of a voice without freeing anything here, see the rules of this file.
    fn retire(&mut self, voice: Voice) {
        let current = self.pads[voice.pad].sound.as_ref();
        if current.is_some_and(|current| Arc::ptr_eq(current, &voice.sound)) {
            // The pad holds this sound too, so this only counts down.
            drop(voice);
            return;
        }
        match self.graveyard.iter_mut().find(|place| place.is_none()) {
            Some(place) => *place = Some(voice.sound),
            // Cannot happen, see the rules of this file. Were it to, leaking one sound is
            // better than freeing it here.
            None => std::mem::forget(voice.sound),
        }
    }

    fn hit(&mut self, pad: usize, velocity: Velocity) {
        let Some(play) = self.pads.get(pad) else {
            return;
        };
        let (choke, fade_frames) = (play.choke, self.fade_frames);
        for voice in self.voices.iter_mut().flatten() {
            let chokes = choke && self.pads[voice.pad].choke;
            if voice.pad == pad || chokes {
                Self::fade(voice, fade_frames);
            }
        }
        let play = &self.pads[pad];
        let Some(sound) = play.sound.clone() else {
            return;
        };
        let voice = Voice {
            end: sound.frames().len().min(play.decay_frames),
            decay_frames: play.decay_frames.max(1) as f32,
            sound,
            pad,
            position: 0,
            velocity: (f32::from(velocity.value()) / 127.0).powi(2),
            fading: None,
        };
        let free = self.voices.iter().position(Option::is_none);
        // Every voice sounds: the one that fades out soonest makes room. One voice per pad
        // does not fade, so at least half of them are fading.
        let soonest = || {
            let fading = self.voices.iter().enumerate();
            let fading = fading.filter_map(|(index, voice)| Some((index, voice.as_ref()?.fading?)));
            fading.min_by_key(|(_, left)| *left).map(|(index, _)| index)
        };
        let Some(index) = free.or_else(soonest) else {
            return;
        };
        if let Some(taken) = self.voices[index].replace(voice) {
            self.retire(taken);
        }
    }

    fn handle(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::On { pitch, velocity } => {
                if let Some(pad) = pad_of(pitch) {
                    self.hit(pad, velocity);
                }
            }
            // A drum is struck and rings out: a note off, the pedal and the wheels do nothing.
            NoteEvent::Off { .. }
            | NoteEvent::Pedal(_)
            | NoteEvent::Bend(_)
            | NoteEvent::ModWheel(_)
            | NoteEvent::Pressure(_) => {}
            // A stop or a jump of the transport: everything fades out.
            NoteEvent::AllOff => {
                let fade_frames = self.fade_frames;
                for voice in self.voices.iter_mut().flatten() {
                    Self::fade(voice, fade_frames);
                }
            }
        }
    }

    /// Renders the frames between two events into `left` and `right`.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        if frames == 0 {
            return;
        }
        // Where each pad's gains start and how far they move per frame in this stretch.
        let mut ramps = [[(0.0_f32, 0.0_f32); 2]; PADS];
        let numbers = self.levels.iter_mut().zip(&mut self.pans);
        for ((level, pan), (ramp, gains)) in numbers.zip(ramps.iter_mut().zip(&mut self.gains)) {
            let before = *gains;
            if level.is_moving() || pan.is_moving() {
                *gains = gains_at(level.advance(frames), pan.advance(frames));
            }
            for ((ramp, before), after) in ramp.iter_mut().zip(before).zip(*gains) {
                *ramp = (before, (after - before) / frames as f32);
            }
        }
        let fade_frames = self.fade_frames as f32;
        let mut loudest = [0.0_f32; PADS];
        let mut ended = [false; VOICES];
        for (index, slot) in self.voices.iter_mut().enumerate() {
            let Some(voice) = slot else {
                continue;
            };
            let [(left_gain, left_step), (right_gain, right_step)] = ramps[voice.pad];
            let sound = voice.sound.frames();
            let mut frame = 0;
            while frame < frames {
                if voice.position >= voice.end || voice.fading == Some(0) {
                    ended[index] = true;
                    break;
                }
                let Some(sample) = sound.get(voice.position) else {
                    ended[index] = true;
                    break;
                };
                let level = voice.level(voice.position, fade_frames);
                let step = (frame + 1) as f32;
                left[frame] += sample[0] * level * (left_gain + left_step * step);
                right[frame] += sample[1] * level * (right_gain + right_step * step);
                voice.position += 1;
                if let Some(left) = &mut voice.fading {
                    *left -= 1;
                }
                frame += 1;
            }
            let level =
                voice.sound.level(voice.position) * voice.level(voice.position, fade_frames);
            loudest[voice.pad] = loudest[voice.pad].max(level);
        }
        for (index, ended) in ended.into_iter().enumerate() {
            if ended && let Some(voice) = self.voices[index].take() {
                self.retire(voice);
            }
        }
        for (peaks, level) in self.peaks.iter().zip(loudest) {
            peaks.record(0, level);
        }
    }
}

impl Processor for DrumPad {
    type Update = DrumUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION.port())
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        let rate = config.sample_rate as f32;
        self.fade_frames = (FADE_SECONDS * rate).round().max(1.0) as u32;
        self.ramp_frames = (RAMP_SECONDS * rate).max(1.0);
    }

    fn update(&mut self, update: &mut DrumUpdate) {
        match update {
            DrumUpdate::Hit { pad, velocity } => self.hit(*pad, *velocity),
            DrumUpdate::Kit(kit) => {
                std::mem::swap(&mut self.pads, &mut kit.pads);
                for place in &mut self.graveyard {
                    if kit.returned.len() < kit.returned.capacity()
                        && let Some(sound) = place.take()
                    {
                        kit.returned.push(sound);
                    }
                }
                let targets = self.state.set_record(&mut kit.state, self.ramp_frames);
                self.aim(&targets);
            }
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(targets) = self.state.follow(context, self.ramp_frames) {
            self.aim(&targets);
        }
        let events = context.event_inputs.get(Self::NOTES);
        if events.is_empty() && self.is_idle() {
            self.snap();
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
    }
}
