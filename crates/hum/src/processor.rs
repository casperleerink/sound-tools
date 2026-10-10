//! The Hum processor: plays [`Machine`]s, one per voice, and fades each from old code to new
//! over a few milliseconds, so an edit of the code while it plays does not click.
//!
//! What every voice shares lives here and is worked out once per frame: the values of the
//! record, or of an automation lane, glided; the `live` controls and triggers the interface
//! sends, which nothing saves; the transport; and the note of each voice.
//!
//! Every kind has every port, as a hosted plugin does: what a kind does not use is connected to
//! nothing, and is silent.

use sound_core::{
    AudioInput, AudioOutput, Automation, CHANNELS, EventInput, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Timed, Watch,
};
use sound_notes::{NoteEvent, Pitch, Velocity, Voice, Voices, frequency_hz};

use crate::code::{Code, MAX_LIVES, MAX_PARAMETERS};
use crate::machine::{Inputs, LIMIT, Machine, Note, Values};

/// How long the old code fades out while the new one fades in.
const FADE_SECONDS: f32 = 0.01;
/// How long a new value of a param or a live control takes to arrive. A jump would click.
const RAMP_SECONDS: f32 = 0.02;
/// A released voice this quiet for this long is over and free for another note.
const QUIET: f32 = 1e-4;
const QUIET_SECONDS: f32 = 0.05;
/// How far the bend wheel moves every note, either way.
const BEND_SEMITONES: f32 = 2.0;

/// The most notes an instrument plays at once.
pub const MAX_VOICES: usize = 8;
/// The most notes and triggers that wait for their frame: a control loop plays a little ahead.
const MAX_SCHEDULED: usize = 256;

/// What a Hum tool is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Sound in, sound out: the code reads `input`.
    Effect,
    /// Notes in, sound out: the code runs once per note, up to `voices` at once.
    Instrument { voices: usize },
    /// Sound out, always running, as one voice that follows the newest held note.
    Source,
}

impl Kind {
    /// How many machines a processor of this kind plays, so the behaviour makes that many.
    pub fn machines(self) -> usize {
        match self {
            Self::Instrument { .. } => MAX_VOICES,
            Self::Effect | Self::Source => 1,
        }
    }
}

pub enum HumUpdate {
    /// From the behaviour, on every run: where the values stand, and, when the code is new, a
    /// machine per voice and the watches of that code. What it replaces rides back in here, to
    /// be dropped off the audio thread.
    Set {
        machines: Vec<Option<Box<Machine>>>,
        /// Boxed, so a live control or a trigger is a small update.
        values: Box<Values>,
        watches: Vec<Watch>,
    },
    /// From the interface: a `live` control moves. Not saved.
    Live { index: usize, value: f32 },
    /// From the interface: a trigger fires at the frame `at` of engine time, or in the next
    /// frame when there is none or it has passed. Not saved.
    Trigger { index: usize, at: Option<u64> },
    /// From the interface: a key held for `frames` from the frame `at` of engine time, or from
    /// the next frame, as if it came in on [`Hum::NOTES`]; with no `frames`, until a
    /// [`HumUpdate::Release`] of its pitch. Not saved.
    Note {
        pitch: Pitch,
        velocity: Velocity,
        at: Option<u64>,
        frames: Option<u64>,
    },
    /// From the interface: lets go of a key at the frame `at` of engine time, or in the next
    /// frame.
    Release { pitch: Pitch, at: Option<u64> },
}

/// What the interface plays at a frame.
#[derive(Copy, Clone)]
enum Scheduled {
    Note(NoteEvent),
    Trigger(usize),
}

pub struct Hum {
    players: Players,
    parameters: [Smoothed; MAX_PARAMETERS],
    /// Where each param stands in the record, and where it was last aimed.
    records: [f32; MAX_PARAMETERS],
    aimed: [f32; MAX_PARAMETERS],
    /// The param each automation lane moves, see [`Values::automated`].
    automated: Vec<u16>,
    lives: [Smoothed; MAX_LIVES],
    /// The values of this frame, which every voice reads.
    current_parameters: [f32; MAX_PARAMETERS],
    current_lives: [f32; MAX_LIVES],
    /// How many of each the code has, so a frame moves only those.
    counts: (usize, usize),
    arrays: Vec<Vec<f32>>,
    /// What the interface plays at a frame of engine time, the latest first. It never grows
    /// past what it was made with, so the audio thread does not allocate.
    scheduled: Vec<(u64, Scheduled)>,
    /// The engine time of the next frame, where what has no time of its own plays.
    next_frame: u64,
    watches: Vec<Watch>,
    /// Where the transport was in the last frame: it stands still while stopped.
    beat: f64,
    bend: f32,
    sample_rate: f32,
    fade_frames: usize,
}

enum Players {
    One(Box<Single>),
    Many(Box<Voices<HumVoice, MAX_VOICES>>),
}

impl Players {
    /// The code the voices play.
    fn code(&self) -> Option<&Code> {
        match self {
            Self::One(single) => Some(single.voice.machine.code()),
            Self::Many(voices) => voices.iter().next().map(|voice| voice.machine.code()),
        }
    }
}

/// The one voice of an effect or a source, and the keys a source follows.
struct Single {
    voice: HumVoice,
    keys: Keys,
}

#[derive(Clone)]
struct HumVoice {
    machine: Box<Machine>,
    /// The machine of the code before. It fades out, then stays until the next new code takes
    /// it back off the audio thread.
    fading: Option<Box<Machine>>,
    fade_left: usize,
    note: Note,
    /// Runs whatever the notes do: an effect and a source.
    always: bool,
    /// Started and not yet quiet after its release.
    sounding: bool,
    quiet_frames: usize,
    loudness: f32,
}

/// The keys a source follows: the newest held one plays, and the note ends when none is held.
struct Keys {
    /// For each key while it is down, the count of the note that pressed it; 0 while up.
    pressed: [u64; 128],
    count: u64,
}

impl Hum {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const AUTOMATION: EventInput<Automation> = EventInput::new(1);

    /// `machine` is the first voice; an instrument plays copies of it.
    pub fn new(kind: Kind, machine: Box<Machine>, values: Values, watches: Vec<Watch>) -> Self {
        let code = machine.code();
        let counts = counts_of(code);
        let lives = lives_of(code);
        let voice = HumVoice {
            machine,
            fading: None,
            fade_left: 0,
            note: Note::default(),
            always: true,
            sounding: false,
            quiet_frames: 0,
            loudness: 0.0,
        };
        let players = match kind {
            Kind::Instrument { voices } => {
                let idle = HumVoice {
                    always: false,
                    ..voice
                };
                Players::Many(Box::new(Voices::new(idle, voices)))
            }
            Kind::Effect | Kind::Source => Players::One(Box::new(Single {
                voice,
                keys: Keys {
                    pressed: [0; 128],
                    count: 0,
                },
            })),
        };
        Self {
            players,
            parameters: values.parameters.map(Smoothed::new),
            records: values.parameters,
            aimed: values.parameters,
            automated: values.automated,
            lives,
            // Every frame moves them before a voice reads them.
            current_parameters: [0.0; MAX_PARAMETERS],
            current_lives: [0.0; MAX_LIVES],
            counts,
            arrays: values.arrays.unwrap_or_default(),
            scheduled: Vec::with_capacity(MAX_SCHEDULED),
            next_frame: 0,
            watches,
            beat: 0.0,
            bend: 0.0,
            sample_rate: 48_000.0,
            fade_frames: 1,
        }
    }

    fn set(
        &mut self,
        machines: &mut [Option<Box<Machine>>],
        values: &mut Values,
        watches: &mut Vec<Watch>,
    ) {
        if let Some(Some(machine)) = machines.first() {
            // The params and lives of new code may differ in number and order: they start
            // where they stand, and the fade covers the jump.
            let code = machine.code();
            self.counts = counts_of(code);
            // A live control the new code has too stays where the interface put it.
            let mut was = std::mem::replace(&mut self.lives, lives_of(code));
            if let Some(old) = self.players.code() {
                for (live, spec) in self.lives.iter_mut().zip(&code.lives) {
                    if let Some(index) = old.lives.iter().position(|old| old.name == spec.name) {
                        std::mem::swap(live, &mut was[index]);
                    }
                }
            }
            self.parameters = values.parameters.map(Smoothed::new);
            self.aimed = values.parameters;
            std::mem::swap(&mut self.watches, watches);
        }
        // Aimed at in the next block, unless a lane moves them.
        self.records = values.parameters;
        if let Some(arrays) = &mut values.arrays {
            std::mem::swap(&mut self.arrays, arrays);
        }
        std::mem::swap(&mut self.automated, &mut values.automated);
        let fade_frames = self.fade_frames;
        let swap = |voice: &mut HumVoice, machine: &mut Option<Box<Machine>>| {
            if let Some(new) = machine.take() {
                let old = std::mem::replace(&mut voice.machine, new);
                // The one that faded before rides back with this update, to be dropped there.
                *machine = voice.fading.replace(old);
                // An idle voice is silent, so it has nothing to fade from. It also plays no
                // frame that would count the fade down before its next note.
                voice.fade_left = if voice.is_idle() { 0 } else { fade_frames };
            }
        };
        match &mut self.players {
            Players::One(single) => {
                if let Some(machine) = machines.first_mut() {
                    swap(&mut single.voice, machine);
                }
            }
            Players::Many(voices) => {
                for (voice, machine) in voices.iter_mut().zip(machines.iter_mut()) {
                    swap(voice, machine);
                }
            }
        }
    }

    /// Aims every param at its lane in this block, or at its record when no lane moves it.
    fn aim(&mut self, lanes: &[Timed<Automation>]) {
        let mut targets = self.records;
        for lane in lanes {
            let param = self.automated.get(usize::from(lane.event.parameter));
            if let Some(target) = param.and_then(|param| targets.get_mut(usize::from(*param))) {
                *target = lane.event.value;
            }
        }
        let ramp = RAMP_SECONDS * self.sample_rate;
        let count = self.counts.0;
        let moving = (self.parameters.iter_mut().zip(&mut self.aimed).zip(targets)).take(count);
        for ((parameter, aimed), target) in moving {
            if *aimed != target {
                parameter.set_target(target, ramp);
                *aimed = target;
            }
        }
    }

    /// Keeps `what` for its frame, after what is already there for that frame. A full list
    /// drops it, and says so.
    fn schedule(&mut self, at: Option<u64>, what: Scheduled) -> bool {
        if self.scheduled.len() == MAX_SCHEDULED {
            return false;
        }
        let at = at.unwrap_or(0).max(self.next_frame);
        let index = self.scheduled.partition_point(|(other, _)| *other > at);
        self.scheduled.insert(index, (at, what));
        true
    }

    fn follow(&mut self, event: NoteEvent) {
        if let NoteEvent::Bend(bend) = event {
            self.bend = bend.fraction() * BEND_SEMITONES;
        }
        match &mut self.players {
            Players::Many(voices) => voices.handle(event, &()),
            Players::One(single) => single.keys.follow(event, &mut single.voice.note),
        }
    }
}

impl Keys {
    fn follow(&mut self, event: NoteEvent, note: &mut Note) {
        match event {
            NoteEvent::On { pitch, velocity } => {
                self.count += 1;
                self.pressed[usize::from(pitch.number())] = self.count;
                note.pitch = f32::from(pitch.number());
                note.velocity = velocity_of(velocity);
                note.gate = true;
                note.onset = true;
            }
            NoteEvent::Off { pitch } => {
                self.pressed[usize::from(pitch.number())] = 0;
                let newest = self
                    .pressed
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, order)| **order);
                match newest {
                    Some((number, order)) if *order > 0 => note.pitch = number as f32,
                    _ => note.gate = false,
                }
            }
            NoteEvent::AllOff => {
                self.pressed = [0; 128];
                note.gate = false;
            }
            _ => {}
        }
    }
}

/// How many params and live controls `code` has, so a frame moves only those.
fn counts_of(code: &Code) -> (usize, usize) {
    (code.parameters.len(), code.lives.len())
}

/// The live controls of `code` at their defaults.
fn lives_of(code: &Code) -> [Smoothed; MAX_LIVES] {
    std::array::from_fn(|index| {
        Smoothed::new(code.lives.get(index).map_or(0.0, |live| live.default))
    })
}

fn velocity_of(velocity: Velocity) -> f32 {
    f32::from(velocity.value()) / 127.0
}

impl Voice for HumVoice {
    type Context = ();

    fn is_idle(&self) -> bool {
        !self.always && !self.sounding
    }

    fn loudness(&self) -> f32 {
        self.loudness
    }

    fn start(&mut self, pitch: f32, velocity: Velocity, _: &()) {
        self.note = Note {
            pitch,
            frequency: 0.0,
            velocity: velocity_of(velocity),
            gate: true,
            onset: true,
        };
        self.sounding = true;
        self.quiet_frames = 0;
    }

    fn set_pitch(&mut self, pitch: f32, _: &()) {
        self.note.pitch = pitch;
    }

    fn release(&mut self) {
        self.note.gate = false;
    }
}

impl HumVoice {
    /// One frame of this voice. `inputs` has the note of this voice.
    fn frame(
        &mut self,
        inputs: &Inputs<'_>,
        fade_frames: usize,
        quiet_limit: usize,
    ) -> [f32; CHANNELS] {
        let mut output = self.machine.frame(inputs);
        if self.fade_left > 0
            && let Some(fading) = &mut self.fading
        {
            let old = fading.frame(inputs);
            let new = 1.0 - self.fade_left as f32 / fade_frames as f32;
            for (sample, old) in output.iter_mut().zip(old) {
                *sample = old + (*sample - old) * new;
            }
            self.fade_left -= 1;
        }
        self.note.onset = false;
        self.loudness = output
            .iter()
            .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()));
        if !self.always && !self.note.gate {
            if self.loudness < QUIET {
                self.quiet_frames += 1;
                self.sounding = self.quiet_frames < quiet_limit;
            } else {
                self.quiet_frames = 0;
            }
        }
        output
    }
}

impl Processor for Hum {
    type Update = HumUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION)
    }

    fn prepare(&mut self, config: &PrepareConfig) {
        self.sample_rate = config.sample_rate as f32;
        self.fade_frames = ((FADE_SECONDS * self.sample_rate) as usize).max(1);
    }

    fn update(&mut self, update: &mut HumUpdate) {
        match update {
            HumUpdate::Set {
                machines,
                values,
                watches,
            } => self.set(machines, values, watches),
            HumUpdate::Live { index, value } => {
                let ramp = RAMP_SECONDS * self.sample_rate;
                if let Some(live) = self.lives.get_mut(*index) {
                    live.set_target(*value, ramp);
                }
            }
            HumUpdate::Trigger { index, at } => {
                if *index < MAX_LIVES {
                    self.schedule(*at, Scheduled::Trigger(*index));
                }
            }
            HumUpdate::Note {
                pitch,
                velocity,
                at,
                frames,
            } => {
                // Both or neither, so no key stays down.
                if self.scheduled.len() + 2 <= MAX_SCHEDULED {
                    let start = at.unwrap_or(0).max(self.next_frame);
                    let (pitch, velocity) = (*pitch, *velocity);
                    self.schedule(
                        Some(start),
                        Scheduled::Note(NoteEvent::On { pitch, velocity }),
                    );
                    if let Some(frames) = frames {
                        let end = start.saturating_add((*frames).max(1));
                        self.schedule(Some(end), Scheduled::Note(NoteEvent::Off { pitch }));
                    }
                }
            }
            HumUpdate::Release { pitch, at } => {
                let off = NoteEvent::Off { pitch: *pitch };
                // A full list lets go at once: a dropped release would hold the note for ever.
                if !self.schedule(*at, Scheduled::Note(off)) {
                    self.follow(off);
                }
            }
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        self.aim(context.event_inputs.get(Self::AUTOMATION));
        let [left_in, right_in] = context.audio_inputs.get(Self::INPUT);
        let mut events = context.event_inputs.get(Self::NOTES).iter().peekable();
        let transport = &context.transport;
        let playing = transport.playing;
        if let Some(quarters) = transport.quarters() {
            self.beat = quarters;
        }
        let tempo_tick = if playing {
            transport.tick_range.start
        } else {
            transport.heard_tick
        };
        let bpm = transport.clock.tempo_at(tempo_tick).bpm();
        let beats_per_frame = bpm / 60.0 / f64::from(self.sample_rate);
        let quiet_limit = (QUIET_SECONDS * self.sample_rate) as usize;
        self.next_frame = context.start_frame + context.frames as u64;

        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        for frame in 0..context.frames {
            // Each note at its own frame, and what the interface plays at its own.
            while let Some(timed) = events.next_if(|timed| timed.offset <= frame) {
                self.follow(timed.event);
            }
            let now = context.start_frame + frame as u64;
            let mut triggers = 0;
            while let Some(&(at, what)) = self.scheduled.last()
                && at <= now
            {
                self.scheduled.pop();
                match what {
                    Scheduled::Note(event) => self.follow(event),
                    Scheduled::Trigger(index) => triggers |= 1 << index,
                }
            }
            let (parameters, lives) = self.counts;
            for (current, parameter) in (self.current_parameters.iter_mut())
                .zip(&mut self.parameters)
                .take(parameters)
            {
                *current = parameter.advance(1);
            }
            for (current, live) in self
                .current_lives
                .iter_mut()
                .zip(&mut self.lives)
                .take(lives)
            {
                *current = live.advance(1);
            }
            let mut inputs = Inputs {
                input: [
                    left_in.get(frame).copied().unwrap_or(0.0),
                    right_in.get(frame).copied().unwrap_or(0.0),
                ],
                parameters: &self.current_parameters,
                lives: &self.current_lives,
                arrays: &self.arrays,
                triggers,
                beat: self.beat,
                bpm: bpm as f32,
                playing,
                note: Note::default(),
            };
            let fade_frames = self.fade_frames;
            let mut output = [0.0; CHANNELS];
            let mut play = |voice: &mut HumVoice, inputs: &mut Inputs<'_>| {
                inputs.note = voice.note;
                inputs.note.frequency = frequency_hz(voice.note.pitch + self.bend);
                let sound = voice.frame(inputs, fade_frames, quiet_limit);
                for (sum, sample) in output.iter_mut().zip(sound) {
                    *sum += sample;
                }
            };
            match &mut self.players {
                Players::One(single) => play(&mut single.voice, &mut inputs),
                Players::Many(voices) => {
                    for voice in voices.iter_mut().filter(|voice| !voice.is_idle()) {
                        play(voice, &mut inputs);
                    }
                }
            }
            if let Some(left) = left_out.get_mut(frame) {
                *left = output[0].clamp(-LIMIT, LIMIT);
            }
            if let Some(right) = right_out.get_mut(frame) {
                *right = output[1].clamp(-LIMIT, LIMIT);
            }
            if playing {
                self.beat += beats_per_frame;
            }
        }

        let shown = match &self.players {
            Players::One(single) => Some(&single.voice),
            Players::Many(voices) => voices.newest(),
        };
        if let Some(voice) = shown {
            for (watch, value) in self.watches.iter().zip(voice.machine.watched()) {
                watch.set(*value);
            }
        }
    }
}
