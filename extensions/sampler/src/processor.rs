//! The sampler processor: 32 voices, each up to 8 layers, a layer one zone of the instrument
//! read at the speed of its key through an envelope.
//!
//! The instrument arrives in an update as an `Arc<Instrument>`, read on the control side. When
//! another one arrives, the voices of the old one fade out over 5 ms while it is kept as the
//! previous instrument, and the one before that rides back to the control side in the update.
//! So the audio thread never reads a file and never frees a sample.
//!
//! A key picks its zones when it goes down: by key, velocity, keyswitch, round robin and a
//! random value. Release zones are picked then too and wait in the voice until the key comes up.
//!
//! Everything is allocated with the processor or in the update. A sampler with no voice in use
//! returns before it touches its output.

use std::sync::Arc;

use sound_core::{
    AudioOutput, Automated, AutomationInput, Envelope, EnvelopeStage, EnvelopeState, EventInput,
    MAX_BLOCK, Peaks, Ports, PrepareConfig, ProcessContext, Processor, Smoothed, Targets,
    amplitude, pan_gains,
};
use sound_media::{SCRATCH_FRAMES, Varispeed, varispeed};
use sound_notes::{NoteEvent, Pitch, Velocity, Voice as _, Voices, Wheels};

use crate::instrument::{Instrument, Looping, Zone};
use crate::sfz::{OffMode, Trigger};
use crate::{ATTACK, DECAY, GAIN, RELEASE, SUSTAIN, SamplerState, VELOCITY};

/// The numbers an automation lane can move: every number but the root, a key.
const AUTOMATED: [&crate::Parameter; 6] = [&ATTACK, &DECAY, &SUSTAIN, &RELEASE, &VELOCITY, &GAIN];

type SamplerTargets = Targets<SamplerState, { AUTOMATED.len() }>;

/// Notes that sound at once. One more note takes over a voice, as [`Voices`] picks it. A sample
/// cannot take over in place as the synth's oscillator does: the new note starts at the start
/// of the file, a step from where the old one was. So the voice it takes fades out over 5 ms
/// next to the new note, in one of the slots kept for that. A piano under the pedal holds many
/// notes, hence more than the synth.
pub const VOICES: usize = 32;

/// Zones one note plays at once: velocity layers that sound together, microphones of a drum,
/// and the release zones that wait for the key to come up. More are left out.
pub const LAYERS: usize = 8;

/// Voices that fade out after they were taken over or their instrument was replaced. More than
/// this many at once, within 5 ms, and the new note takes the one closest to silence.
const FADING_VOICES: usize = 8;

const SLOTS: usize = VOICES + FADING_VOICES;

/// How long a voice that was taken over, or whose instrument went, takes to fade out, and a
/// zone that a choke group stops fast.
const FADE_SECONDS: f32 = 0.005;

/// The level before the end of the part of the file that plays ramps to silence over this long,
/// so a sample cut in the middle of its sound does not click. The ramp of an audio clip's edge.
const EDGE_SECONDS: f64 = 0.002;

/// How long the gain takes to reach a new value: the glide of every built-in device.
const GLIDE_SECONDS: f32 = 0.02;

/// The shortest envelope stage of an SFZ zone. Files often leave the release at 0, which would
/// click.
const SHORTEST_STAGE_SECONDS: f32 = 0.002;

/// What the processor plays from: the record and the instrument, made on the control side, and
/// a round robin count per zone. All are swapped in, and what they replace goes back to the
/// control side.
pub struct SamplerUpdate {
    state: SamplerState,
    instrument: Option<Arc<Instrument>>,
    counters: Box<[u32]>,
}

impl SamplerUpdate {
    /// The update for a record and what it plays, or nothing.
    pub fn new(state: &SamplerState, instrument: Option<Arc<Instrument>>) -> Self {
        let zones = instrument
            .as_ref()
            .map_or(0, |instrument| instrument.zones.len());
        Self {
            state: state.clone(),
            instrument,
            counters: vec![0; zones].into_boxed_slice(),
        }
    }
}

/// The record in the units the audio thread works in, for a zone that plays as the record
/// says: places are frames of its file.
#[derive(Copy, Clone)]
struct Settings {
    root: Pitch,
    start_seconds: f64,
    end_seconds: Option<f64>,
    attack_seconds: f32,
    decay_seconds: f32,
    sustain: f32,
    release_seconds: f32,
    velocity_to_volume: f32,
    /// Linear.
    gain: f32,
}

impl Settings {
    fn new(state: &SamplerState) -> Self {
        Self {
            root: state.root,
            start_seconds: state.start_seconds,
            end_seconds: state.end_seconds,
            attack_seconds: state.attack_seconds,
            decay_seconds: state.decay_seconds,
            sustain: state.sustain,
            release_seconds: state.release_seconds,
            velocity_to_volume: state.velocity_to_volume,
            gain: amplitude(state.gain_db),
        }
    }
}

/// The envelope of the record: the synth's, from `sound_core`.
fn record_envelope(settings: &Settings, sample_rate: f32) -> Envelope {
    Envelope::new(
        settings.attack_seconds,
        settings.decay_seconds,
        settings.sustain,
        settings.release_seconds,
        sample_rate,
    )
}

/// The level of a note of `velocity`: `tracking` 0 plays every note at full level, 1 plays
/// velocity 64 a quarter as loud as 127. Below 0 a soft note is the louder.
fn velocity_level(tracking: f32, velocity: Velocity) -> f32 {
    let played = f32::from(velocity.value()) / 127.0;
    let curve = played * played;
    match tracking >= 0.0 {
        true => 1.0 - tracking + tracking * curve,
        false => 1.0 + tracking * curve,
    }
}

/// Which instrument a voice plays.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Source {
    Current,
    /// The one before the current, kept until its voices have faded out.
    Previous,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Phase {
    Idle,
    /// A release zone, waiting for the key to come up.
    Waiting,
    Playing,
}

/// One zone of a note.
#[derive(Copy, Clone)]
struct Layer {
    phase: Phase,
    zone: usize,
    /// Where it is in the file, in frames of the file.
    position: f64,
    /// Its pitch: `(pitch - keycenter) * keytrack + tune`, in semitones.
    keycenter: f32,
    keytrack: f32,
    tune: f32,
    /// Frames of the file per frame of the engine at the pitch of its zone: the ratio of the
    /// rates.
    rates: f64,
    /// Frames of the file per frame of the engine at the pitch of the note, before the wheels
    /// move it.
    key_step: f64,
    /// The frame after the last that plays.
    end: f64,
    looping: Looping,
    /// The level of each channel: velocity, volume, amplitude and pan.
    gains: [f32; 2],
    /// `None` follows the record's, which moves with its knobs, also under held notes.
    envelope: Option<Envelope>,
    state: EnvelopeState,
    /// From 1 down to 0 while it fades out, over `fade_seconds`; 0 seconds when it does not.
    fade: f32,
    fade_seconds: f32,
    /// The choke group whose notes stop it, and how.
    off_by: Option<i64>,
    off_mode: OffMode,
}

impl Layer {
    const IDLE: Self = Self {
        phase: Phase::Idle,
        zone: 0,
        position: 0.0,
        keycenter: 60.0,
        keytrack: 1.0,
        tune: 0.0,
        rates: 1.0,
        key_step: 1.0,
        end: 0.0,
        looping: Looping::No { one_shot: false },
        gains: [0.0; 2],
        envelope: None,
        state: EnvelopeState::IDLE,
        fade: 1.0,
        fade_seconds: 0.0,
        off_by: None,
        off_mode: OffMode::Fast,
    };

    /// A layer of `zone` for a note of `velocity`, ready to start, at the engine's
    /// `sample_rate`.
    fn new(
        (index, zone): (usize, &Zone),
        velocity: Velocity,
        record: (&Settings, bool),
        sample_rate: f32,
    ) -> Self {
        let (settings, from_record) = record;
        let region = &zone.region;
        let file_rate = f64::from(zone.audio.sample_rate());
        let frames = zone.audio.frames() as f64;
        let (keycenter, offset, end, envelope, level) = match from_record {
            true => (
                settings.root.number(),
                settings.start_seconds * file_rate,
                settings
                    .end_seconds
                    .map_or(frames, |end| (end * file_rate).min(frames)),
                None,
                velocity_level(settings.velocity_to_volume, velocity),
            ),
            false => {
                let adsr = region.envelope;
                let envelope = Envelope::new(
                    adsr.attack.max(SHORTEST_STAGE_SECONDS),
                    adsr.decay.max(SHORTEST_STAGE_SECONDS),
                    adsr.sustain,
                    adsr.release.max(SHORTEST_STAGE_SECONDS),
                    sample_rate,
                );
                let level = velocity_level(region.velocity_tracking, velocity)
                    * amplitude(region.volume_db)
                    * region.amplitude;
                (
                    region.keycenter,
                    region.offset as f64,
                    region
                        .end
                        .map_or(frames, |end| ((end + 1) as f64).min(frames)),
                    Some(envelope),
                    level,
                )
            }
        };
        let phase = match region.trigger {
            Trigger::Attack => Phase::Playing,
            Trigger::Release => Phase::Waiting,
        };
        Self {
            phase,
            zone: index,
            position: offset,
            keycenter: f32::from(keycenter),
            keytrack: region.keytrack_cents / 100.0,
            tune: region.tune_cents / 100.0,
            rates: file_rate / f64::from(sample_rate.max(1.0)),
            key_step: 1.0,
            end,
            looping: zone.looping,
            gains: pan_gains(f64::from(level), region.pan),
            envelope,
            state: EnvelopeState::IDLE,
            fade: 1.0,
            fade_seconds: 0.0,
            off_by: region.off_by,
            off_mode: region.off_mode,
        }
    }

    fn is_idle(&self) -> bool {
        self.phase == Phase::Idle
    }

    fn set_pitch(&mut self, pitch: f32) {
        let semitones = (pitch - self.keycenter) * self.keytrack + self.tune;
        self.key_step = (f64::from(semitones) / 12.0).exp2() * self.rates;
    }

    /// Starts to sound: from silence through its attack.
    fn sound(&mut self) {
        self.phase = Phase::Playing;
        self.state = EnvelopeState {
            stage: EnvelopeStage::Attack,
            level: 0.0,
        };
    }

    fn release(&mut self) {
        match self.phase {
            Phase::Waiting => self.sound(),
            Phase::Playing => {
                if let Looping::Loop { sustain: true, .. } = self.looping {
                    self.looping = Looping::No { one_shot: false };
                }
                if self.looping != (Looping::No { one_shot: true }) {
                    self.state.release();
                }
            }
            Phase::Idle => {}
        }
    }

    /// Fades out over `seconds`. A release zone that has not started never will.
    fn fade_out(&mut self, seconds: f32) {
        match self.phase {
            Phase::Waiting => *self = Self::IDLE,
            Phase::Playing => {
                // A fade already on its way keeps the faster of the two.
                self.fade_seconds = match self.fade_seconds > 0.0 {
                    true => self.fade_seconds.min(seconds),
                    false => seconds,
                };
            }
            Phase::Idle => {}
        }
    }

    /// Adds this layer to `left` and `right` at `pitch_ratio` times the pitch of its key.
    /// `frames` holds the sample on its way; `scratch` the file frames the filter reads.
    fn render(
        &mut self,
        zone: &Zone,
        (left, right): (&mut [f32], &mut [f32]),
        (frames, scratch): (&mut [[f32; 2]], &mut [[f32; 2]]),
        record_envelope: &Envelope,
        (edge_frames, sample_rate): (f64, f32),
        (filter, pitch_ratio): (&Varispeed, f64),
    ) {
        let count = left.len().min(frames.len());
        let fade_step = match self.fade_seconds > 0.0 {
            true => 1.0 / (self.fade_seconds * sample_rate).max(1.0),
            false => 0.0,
        };
        let frames = &mut frames[..count];
        let step = self.key_step * pitch_ratio;
        let start = self.position;
        self.read(zone, step, frames, scratch, filter);
        let envelope = self.envelope.as_ref().unwrap_or(record_envelope);
        for (index, ((left, right), frame)) in left
            .iter_mut()
            .zip(right.iter_mut())
            .zip(frames.iter())
            .enumerate()
        {
            let edge = match self.looping {
                Looping::Loop { .. } => 1.0,
                Looping::No { .. } => {
                    // Engine frames from this one to the end of the part that plays.
                    let to_end = (self.end - (start + step * index as f64)) / step;
                    if to_end <= 0.0 {
                        self.phase = Phase::Idle;
                        break;
                    }
                    (to_end / edge_frames).min(1.0) as f32
                }
            };
            let level = self.state.next(envelope) as f32;
            if self.state.is_idle() {
                self.phase = Phase::Idle;
                break;
            }
            if fade_step > 0.0 {
                self.fade -= fade_step;
                if self.fade <= 0.0 {
                    self.phase = Phase::Idle;
                    break;
                }
            }
            let gain = level * self.fade * edge;
            *left += frame[0] * gain * self.gains[0];
            *right += frame[1] * gain * self.gains[1];
        }
        if self.is_idle() {
            *self = Self::IDLE;
        }
    }

    /// Reads `frames.len()` frames of its file from where it is at `step`, around its loop, and
    /// moves on.
    fn read(
        &mut self,
        zone: &Zone,
        step: f64,
        frames: &mut [[f32; 2]],
        scratch: &mut [[f32; 2]],
        filter: &Varispeed,
    ) {
        let mut done = 0;
        while done < frames.len() {
            let left = frames.len() - done;
            let count = match self.looping {
                Looping::Loop { end, .. } if step > 0.0 => {
                    // Frames until the place passes the end of the loop, at least one.
                    let until = ((end - self.position) / step).ceil();
                    (until.max(1.0) as usize).min(left)
                }
                _ => left,
            };
            filter.render(
                &zone.audio,
                self.position,
                step,
                &mut frames[done..done + count],
                scratch,
            );
            self.position += step * count as f64;
            if let Looping::Loop { start, end, .. } = self.looping
                && self.position >= end
            {
                self.position = start + (self.position - end) % (end - start);
            }
            done += count;
        }
    }
}

/// The layers a note starts with, picked from the instrument when its key went down.
#[derive(Copy, Clone)]
pub(crate) struct Start {
    layers: [Layer; LAYERS],
}

impl Start {
    const NONE: Self = Self {
        layers: [Layer::IDLE; LAYERS],
    };

    fn is_empty(&self) -> bool {
        self.layers.iter().all(Layer::is_idle)
    }
}

#[derive(Copy, Clone)]
struct Voice {
    source: Source,
    layers: [Layer; LAYERS],
}

/// A voice takes its layers from the note's [`Start`].
impl sound_notes::Voice for Voice {
    type Context = Start;

    fn is_idle(&self) -> bool {
        self.layers.iter().all(Layer::is_idle)
    }

    fn loudness(&self) -> f32 {
        let playing = self
            .layers
            .iter()
            .filter(|layer| layer.phase == Phase::Playing);
        playing
            .map(|layer| layer.state.level as f32 * layer.fade * layer.gains[0].max(layer.gains[1]))
            .fold(0.0, f32::max)
    }

    fn start(&mut self, pitch: f32, _: Velocity, start: &Start) {
        self.source = Source::Current;
        self.layers = start.layers;
        for layer in &mut self.layers {
            layer.set_pitch(pitch);
            if layer.phase == Phase::Playing {
                layer.sound();
            }
        }
    }

    fn set_pitch(&mut self, pitch: f32, _: &Start) {
        for layer in &mut self.layers {
            layer.set_pitch(pitch);
        }
    }

    fn release(&mut self) {
        self.layers.iter_mut().for_each(Layer::release);
    }

    /// Fades out over 5 ms.
    fn cut(&mut self) {
        for layer in &mut self.layers {
            layer.fade_out(FADE_SECONDS);
        }
    }
}

impl Voice {
    const IDLE: Self = Self {
        source: Source::Current,
        layers: [Layer::IDLE; LAYERS],
    };

    fn render(
        &mut self,
        instrument: &Instrument,
        (left, right): (&mut [f32], &mut [f32]),
        buffers: (&mut [[f32; 2]], &mut [[f32; 2]]),
        record_envelope: &Envelope,
        edges: (f64, f32),
        filter: (&Varispeed, f64),
    ) {
        let (frames, scratch) = buffers;
        for layer in &mut self.layers {
            if layer.phase != Phase::Playing {
                continue;
            }
            let Some(zone) = instrument.zones.get(layer.zone) else {
                *layer = Layer::IDLE;
                continue;
            };
            layer.render(
                zone,
                (&mut *left, &mut *right),
                (&mut *frames, &mut *scratch),
                record_envelope,
                edges,
                filter,
            );
        }
    }
}

/// A small fixed generator for `lorand` and `hirand`, so a render gives the same bytes every
/// time.
struct Random(u32);

impl Random {
    fn next(&mut self) -> f32 {
        // xorshift32.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }
}

pub struct Sampler {
    /// The record, with the values of the lanes that automate it.
    state: Automated<SamplerState, { AUTOMATED.len() }>,
    settings: Settings,
    current: Option<Arc<Instrument>>,
    previous: Option<Arc<Instrument>>,
    /// How many times each zone of the current instrument matched a note, for round robin.
    counters: Box<[u32]>,
    /// The articulation the last keyswitch picked.
    switch: Option<u8>,
    random: Random,
    /// Zero until `prepare` runs.
    sample_rate: f32,
    envelope: Envelope,
    gain: Smoothed,
    /// Of the slots, `VOICES` play at once and the rest are for voices that fade out.
    voices: Voices<Voice, SLOTS>,
    /// The bend and the vibrato of every voice. At rest after every `AllOff`, like the pedal.
    wheels: Wheels,
    filter: &'static Varispeed,
    /// One stretch of one layer on its way into the output.
    frames: Box<[[f32; 2]]>,
    scratch: Box<[[f32; 2]]>,
    /// Where the last note started is in its file, for the card.
    position: Peaks,
}

impl Sampler {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const AUTOMATION: AutomationInput<SamplerState, { AUTOMATED.len() }> =
        AutomationInput::new(1, AUTOMATED);

    /// A sampler with nothing to play, which the first update gives. `position` is where it
    /// says where its last note is in the file.
    pub fn new(position: Peaks) -> Self {
        let state = SamplerState::default();
        let settings = Settings::new(&state);
        Self {
            state: Automated::new(Self::AUTOMATION, state),
            settings,
            current: None,
            previous: None,
            counters: Box::new([]),
            switch: None,
            random: Random(0x9E37_79B9),
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
        match event {
            NoteEvent::On { pitch, velocity } => {
                let start = self.pick(pitch, velocity);
                // A key with no zone, such as a keyswitch, takes no voice.
                if !start.is_empty() {
                    self.voices.handle(event, &start);
                }
            }
            _ => self.voices.handle(event, &Start::NONE),
        }
    }

    /// The zones a key plays, and what it changes: the keyswitch, the round robin and the
    /// choke groups.
    fn pick(&mut self, pitch: Pitch, velocity: Velocity) -> Start {
        let mut start = Start::NONE;
        let Some(instrument) = self.current.as_deref() else {
            return start;
        };
        let key = pitch.number();
        if let Some((low, high)) = instrument.switch_keys
            && (low..=high).contains(&key)
        {
            self.switch = Some(key);
        }
        let random = self.random.next();
        let record = (&self.settings, instrument.from_record);
        let mut count = 0;
        // The choke groups of the zones it starts, stopped once the zones are picked.
        let mut chokes = [0; LAYERS];
        for (index, zone) in instrument.zones.iter().enumerate() {
            let region = &zone.region;
            let (low_key, high_key) = region.keys;
            let (low_velocity, high_velocity) = region.velocities;
            if !(low_key..=high_key).contains(&key)
                || !(low_velocity..=high_velocity).contains(&velocity.value())
            {
                continue;
            }
            if let Some((low, high)) = region.switch
                && !self
                    .switch
                    .is_some_and(|switch| (low..=high).contains(&switch))
            {
                continue;
            }
            let (length, position) = region.sequence;
            if let Some(counter) = self.counters.get_mut(index) {
                let turn = *counter % length;
                *counter = counter.wrapping_add(1);
                if turn + 1 != position {
                    continue;
                }
            }
            let (low, high) = region.random;
            if random < low || (random >= high && high < 1.0) {
                continue;
            }
            if count == LAYERS {
                break;
            }
            start.layers[count] = Layer::new((index, zone), velocity, record, self.sample_rate);
            if region.trigger == Trigger::Attack {
                chokes[count] = region.group;
            }
            count += 1;
        }
        for group in chokes.into_iter().filter(|group| *group != 0) {
            self.choke(group);
        }
        start
    }

    /// Stops what group `group` stops: the zones with `off_by` that group.
    fn choke(&mut self, group: i64) {
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            for layer in voice.layers.iter_mut() {
                if layer.off_by != Some(group) {
                    continue;
                }
                match layer.off_mode {
                    OffMode::Fast => layer.fade_out(FADE_SECONDS),
                    OffMode::Time(seconds) => layer.fade_out(seconds.max(SHORTEST_STAGE_SECONDS)),
                    OffMode::Normal => layer.release(),
                }
            }
        }
    }

    /// Renders the voices into the frames between two events, then the gain over them.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let count = left.len();
        if count == 0 {
            return;
        }
        let edge_frames = (EDGE_SECONDS * f64::from(self.sample_rate)).max(1.0);
        let pitch_ratio = f64::from(self.wheels.pitch_ratio(count, self.sample_rate));
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            let instrument = match voice.source {
                Source::Current => self.current.as_deref(),
                Source::Previous => self.previous.as_deref(),
            };
            let Some(instrument) = instrument else {
                *voice = Voice::IDLE;
                continue;
            };
            voice.render(
                instrument,
                (&mut *left, &mut *right),
                (&mut self.frames, &mut self.scratch),
                &self.envelope,
                (edge_frames, self.sample_rate),
                (self.filter, pitch_ratio),
            );
        }
        self.voices.glide(count, &Start::NONE);
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

    /// Works out the record and its lanes. The gain glides, the envelope applies at once, and
    /// the rest from the next note.
    fn aim(&mut self, targets: &SamplerTargets) {
        self.settings = Settings::new(&self.state);
        self.envelope = record_envelope(&self.settings, self.sample_rate);
        self.gain
            .set_target(self.settings.gain, targets.ramp(&GAIN));
        if self.voices.is_idle() {
            // Nothing sounds, so there is nothing to smooth.
            self.gain.snap();
        }
    }

    /// Takes the instrument of an update, unless it plays the same as the one that plays. The
    /// voices of the one before fade out.
    fn take(&mut self, update: &mut SamplerUpdate) {
        let same = match (&self.current, &update.instrument) {
            (Some(current), Some(new)) => current.same_as(new),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        for voice in self.voices.iter_mut().filter(|voice| !voice.is_idle()) {
            match voice.source {
                // A second new instrument within 5 ms: these lose theirs, which goes back now.
                Source::Previous => *voice = Voice::IDLE,
                Source::Current => voice.source = Source::Previous,
            }
        }
        self.voices.cut_all();
        // The current instrument becomes the previous one, the new one comes in, and the one
        // before goes back to the control side inside the update, to be dropped there.
        std::mem::swap(&mut self.previous, &mut self.current);
        std::mem::swap(&mut self.current, &mut update.instrument);
        std::mem::swap(&mut self.counters, &mut update.counters);
        self.switch = self
            .current
            .as_ref()
            .and_then(|instrument| instrument.switch_default);
    }

    /// Where the newest note that still sounds is in its file, for the card.
    fn show_position(&self) {
        // A voice that is not cut plays the current instrument.
        let Some(voice) = self.voices.newest() else {
            return;
        };
        let Some(layer) = voice
            .layers
            .iter()
            .find(|layer| layer.phase == Phase::Playing)
        else {
            return;
        };
        let file_rate = layer.rates * f64::from(self.sample_rate);
        let seconds = (layer.position / file_rate.max(1.0)) as f32;
        // A peak of 0 is no peak, so the very first frame of a file shows as the smallest
        // place after it.
        self.position.record(0, seconds.max(f32::MIN_POSITIVE));
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
        self.envelope = record_envelope(&self.settings, self.sample_rate);
    }

    fn update(&mut self, update: &mut SamplerUpdate) {
        let targets = self.state.set_record(&mut update.state, self.ramp_frames());
        self.take(update);
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
