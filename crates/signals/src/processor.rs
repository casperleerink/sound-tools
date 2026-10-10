//! The [`Signals`] processor: plays [`Machine`]s, one per voice, and fades each from old code to
//! new over a few milliseconds, so an edit of the code while it plays does not click.
//!
//! What every voice shares lives here and is worked out once per block, frame by frame: the
//! values of the record, or of an automation lane, glided; the `live` controls and triggers the
//! interface sends, which nothing saves; and the transport. A block is played in spans: it is
//! split where a note comes or goes, so the note of each voice is the same over a span.
//!
//! Every kind has every port, as a hosted plugin does: what a kind does not use is connected to
//! nothing, and is silent.

use std::ops::Range;

use sound_core::{
    AudioInput, AudioOutput, Automation, CHANNELS, EventInput, MAX_BLOCK, Ports, PrepareConfig,
    ProcessContext, Processor, Smoothed, Timed, Watch,
};
use sound_notes::{NoteEvent, Pitch, Velocity, Voice, Voices, frequency_hz};

use crate::code::{Code, MAX_LIVES, MAX_PARAMETERS};
use crate::machine::{Block, Frames, Inputs, LIMIT, Machine, Note, Values};

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

/// What a tool is.
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

pub enum SignalsUpdate {
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
    /// the next frame, as if it came in on [`Signals::NOTES`]; with no `frames`, until a
    /// [`SignalsUpdate::Release`] of its pitch. Not saved.
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

pub struct Signals {
    players: Players,
    parameters: [Smoothed; MAX_PARAMETERS],
    /// Where each param stands in the record, and where it was last aimed.
    records: [f32; MAX_PARAMETERS],
    aimed: [f32; MAX_PARAMETERS],
    /// The param each automation lane moves, see [`Values::automated`].
    automated: Vec<u16>,
    lives: [Smoothed; MAX_LIVES],
    /// What every voice reads in this block.
    block: Box<Block>,
    /// What the voices play in this block, added up.
    mix: Frames,
    /// What one voice plays in a span, and what its old code plays while it fades out.
    voice_frames: [Frames; 2],
    /// How many params and live controls the code has, so a block moves only those.
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
    Many(Box<Voices<SignalsVoice, MAX_VOICES>>),
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
    voice: SignalsVoice,
    keys: Keys,
}

#[derive(Clone)]
struct SignalsVoice {
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

impl Signals {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    pub const AUTOMATION: EventInput<Automation> = EventInput::new(1);

    /// `machine` is the first voice; an instrument plays copies of it.
    pub fn new(kind: Kind, machine: Box<Machine>, values: Values, watches: Vec<Watch>) -> Self {
        let code = machine.code();
        let counts = counts_of(code);
        let lives = lives_of(code);
        let voice = SignalsVoice {
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
                let idle = SignalsVoice {
                    always: false,
                    ..voice
                };
                let mut voices = Box::new(Voices::new(idle, voices));
                for (index, voice) in voices.iter_mut().enumerate() {
                    voice.machine.seed(index);
                }
                Players::Many(voices)
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
            block: Box::new(Block::new()),
            mix: [[0.0; MAX_BLOCK]; CHANNELS],
            voice_frames: [[[0.0; MAX_BLOCK]; CHANNELS]; 2],
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
            let counts = counts_of(code);
            // The old code fades out, and may read a param or a live control past those of
            // the new code: it reads the last value there was.
            let block = &mut *self.block;
            hold_rows(
                &mut block.parameters,
                &self.parameters,
                counts.0..self.counts.0,
            );
            hold_rows(&mut block.lives, &self.lives, counts.1..self.counts.1);
            self.counts = counts;
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
        let swap = |voice: &mut SignalsVoice, machine: &mut Option<Box<Machine>>, index| {
            if let Some(mut new) = machine.take() {
                new.seed(index);
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
                    swap(&mut single.voice, machine, 0);
                }
            }
            Players::Many(voices) => {
                let pairs = voices.iter_mut().zip(machines.iter_mut()).enumerate();
                for (index, (voice, machine)) in pairs {
                    swap(voice, machine, index);
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

/// Each row of `rows` in `range` at the value of its param or live control now, for the rest
/// of the time.
fn hold_rows(rows: &mut [[f32; MAX_BLOCK]], values: &[Smoothed], range: Range<usize>) {
    let rows = rows
        .iter_mut()
        .zip(values)
        .take(range.end)
        .skip(range.start);
    for (row, value) in rows {
        row.fill(value.current());
    }
}

// A bit per param and per live control, in a `u32`.
const _: () = assert!(MAX_PARAMETERS <= 32 && MAX_LIVES <= 32);

/// Moves the first `count` of `rows` along the glide of their param or live control over
/// `frames`, and gives a bit for each row that is one value in all of them. Rows past `count`
/// keep their bit: [`hold_rows`] set each to one value, and nothing moves them after.
fn advance_rows(
    rows: &mut [[f32; MAX_BLOCK]],
    values: &mut [Smoothed],
    count: usize,
    frames: usize,
) -> u32 {
    let mut steady = u32::MAX;
    for (index, (row, value)) in rows.iter_mut().zip(values).take(count).enumerate() {
        let row = row.get_mut(..frames).unwrap_or_default();
        // Frame by frame: a glide moved by `n` frames at once rounds otherwise. A frame that
        // gives what the one before gave leaves the glide as it found it, so every frame after
        // gives that too.
        let mut settled = row.len();
        let mut before = None;
        for (frame, current) in row.iter_mut().enumerate() {
            *current = value.advance(1);
            if before == Some(current.to_bits()) {
                settled = frame;
                break;
            }
            before = Some(current.to_bits());
        }
        if let Some((kept, rest)) = row.get_mut(settled..).and_then(<[f32]>::split_first_mut) {
            rest.fill(*kept);
        }
        if settled > 1 {
            steady &= !(1 << index);
        }
    }
    steady
}

fn velocity_of(velocity: Velocity) -> f32 {
    f32::from(velocity.value()) / 127.0
}

impl Voice for SignalsVoice {
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

/// What a voice reads in a span besides its note.
struct Shared<'a> {
    block: &'a Block,
    arrays: &'a [Vec<f32>],
    bend: f32,
    fade_frames: usize,
    quiet_limit: usize,
}

impl SignalsVoice {
    /// Plays `frames` of this voice and adds them to `mix`, up to the frame it goes idle in.
    /// `voice_frames` is room for what it plays.
    fn render(
        &mut self,
        shared: &Shared<'_>,
        frames: Range<usize>,
        voice_frames: &mut [Frames; 2],
        mix: &mut Frames,
    ) {
        let [sound, old] = voice_frames;
        let mut start = frames.start;
        while start < frames.end && !self.is_idle() {
            // Cut where the fade ends, and, after the release, where the voice may go idle,
            // so either holds for every frame of a part.
            let mut end = frames.end;
            if self.fade_left > 0 {
                end = end.min(start + self.fade_left);
            }
            let ending = !self.always && !self.note.gate;
            if ending {
                let quiet_left = shared.quiet_limit.saturating_sub(self.quiet_frames);
                end = end.min(start + quiet_left.max(1));
            }
            let part = start..end;
            let inputs = Inputs {
                block: shared.block,
                arrays: shared.arrays,
                note: Note {
                    frequency: frequency_hz(self.note.pitch + shared.bend),
                    ..self.note
                },
            };
            self.machine.render(&inputs, part.clone(), sound);
            if self.fade_left > 0
                && let Some(fading) = &mut self.fading
            {
                fading.render(&inputs, part.clone(), old);
                for frame in part.clone() {
                    let new = 1.0 - self.fade_left as f32 / shared.fade_frames as f32;
                    for (sound, old) in sound.iter_mut().zip(old.iter()) {
                        if let (Some(sample), Some(old)) = (sound.get_mut(frame), old.get(frame)) {
                            *sample = old + (*sample - old) * new;
                        }
                    }
                    self.fade_left -= 1;
                }
            }
            self.note.onset = false;
            let [left, right] = &*sound;
            let [mix_left, mix_right] = &mut *mix;
            let played = (in_part(left, &part).iter())
                .zip(in_part(right, &part))
                .zip(in_part_mut(mix_left, &part))
                .zip(in_part_mut(mix_right, &part));
            for (((left, right), mix_left), mix_right) in played {
                self.loudness = [left, right]
                    .iter()
                    .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()));
                if ending {
                    if self.loudness < QUIET {
                        self.quiet_frames += 1;
                        self.sounding = self.quiet_frames < shared.quiet_limit;
                    } else {
                        self.quiet_frames = 0;
                    }
                }
                *mix_left += left;
                *mix_right += right;
            }
            start = end;
        }
    }
}

fn in_part<'a>(samples: &'a [f32; MAX_BLOCK], part: &Range<usize>) -> &'a [f32] {
    samples.get(part.clone()).unwrap_or_default()
}

fn in_part_mut<'a>(samples: &'a mut [f32; MAX_BLOCK], part: &Range<usize>) -> &'a mut [f32] {
    samples.get_mut(part.clone()).unwrap_or_default()
}

impl Signals {
    /// Works out what every voice reads in the first `frames` frames of this block, but the
    /// params, the live controls and the beat.
    fn fill(&mut self, input: [&[f32]; CHANNELS], frames: usize, bpm: f64, playing: bool) {
        let block = &mut *self.block;
        for (row, samples) in block.input.iter_mut().zip(input) {
            for (frame, sample) in row.iter_mut().take(frames).enumerate() {
                *sample = samples.get(frame).copied().unwrap_or(0.0);
            }
        }
        block.triggers.fill(0);
        block.bpm = bpm as f32;
        block.playing = playing;
    }

    /// The beat of each of the first `frames` frames of this block, moving it on frame by frame
    /// while `playing`.
    fn move_beat(&mut self, frames: usize, beats_per_frame: f64, playing: bool) {
        for beat in self.block.beat.iter_mut().take(frames) {
            *beat = self.beat as f32;
            if playing {
                self.beat += beats_per_frame;
            }
        }
    }

    /// Plays `frames` of every voice that sounds into the mix.
    fn render(&mut self, frames: Range<usize>, quiet_limit: usize) {
        let shared = Shared {
            block: &self.block,
            arrays: &self.arrays,
            bend: self.bend,
            fade_frames: self.fade_frames,
            quiet_limit,
        };
        let (voice_frames, mix) = (&mut self.voice_frames, &mut self.mix);
        match &mut self.players {
            Players::One(single) => single.voice.render(&shared, frames, voice_frames, mix),
            Players::Many(voices) => {
                for voice in voices.iter_mut().filter(|voice| !voice.is_idle()) {
                    voice.render(&shared, frames.clone(), voice_frames, mix);
                }
            }
        }
    }
}

impl Processor for Signals {
    type Update = SignalsUpdate;

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

    fn update(&mut self, update: &mut SignalsUpdate) {
        match update {
            SignalsUpdate::Set {
                machines,
                values,
                watches,
            } => self.set(machines, values, watches),
            SignalsUpdate::Live { index, value } => {
                let ramp = RAMP_SECONDS * self.sample_rate;
                if let Some(live) = self.lives.get_mut(*index) {
                    live.set_target(*value, ramp);
                }
            }
            SignalsUpdate::Trigger { index, at } => {
                if *index < MAX_LIVES {
                    self.schedule(*at, Scheduled::Trigger(*index));
                }
            }
            SignalsUpdate::Note {
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
            SignalsUpdate::Release { pitch, at } => {
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
        // The engine's promise: it splits a device buffer into sub-blocks of at most this.
        debug_assert!(context.frames <= MAX_BLOCK);
        let frames = context.frames.min(MAX_BLOCK);
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
        // Also in a block that plays silence: a stop reads where it ended.
        self.move_beat(frames, beats_per_frame, playing);
        self.next_frame = context.start_frame + context.frames as u64;
        let (parameters, lives) = self.counts;
        let block = &mut *self.block;
        block.steady_parameters = advance_rows(
            &mut block.parameters,
            &mut self.parameters,
            parameters,
            frames,
        );
        block.steady_lives = advance_rows(&mut block.lives, &mut self.lives, lives, frames);
        let [left_out, right_out] = context.audio_outputs.get(Self::OUTPUT);
        // An instrument with no voice that sounds, and no note in this block, plays silence.
        let silent = match &self.players {
            Players::Many(voices) => voices.is_idle(),
            Players::One(_) => false,
        };
        let next = self.next_frame;
        if silent
            && context.event_inputs.get(Self::NOTES).is_empty()
            && self.scheduled.last().is_none_or(|(at, _)| *at >= next)
        {
            left_out.fill(0.0);
            right_out.fill(0.0);
            return;
        }
        let input = context.audio_inputs.get(Self::INPUT);
        self.fill(input, frames, bpm, playing);
        for channel in &mut self.mix {
            channel.fill(0.0);
        }

        // A span from each note or what the interface plays to the next.
        let mut events = context.event_inputs.get(Self::NOTES).iter().peekable();
        let mut start = 0;
        while start < frames {
            while let Some(timed) = events.next_if(|timed| timed.offset <= start) {
                self.follow(timed.event);
            }
            let now = context.start_frame + start as u64;
            while let Some(&(at, what)) = self.scheduled.last()
                && at <= now
            {
                self.scheduled.pop();
                match what {
                    Scheduled::Note(event) => self.follow(event),
                    Scheduled::Trigger(index) => {
                        if let Some(triggers) = self.block.triggers.get_mut(start) {
                            *triggers |= 1 << index;
                        }
                    }
                }
            }
            let next_event = events.peek().map_or(frames, |timed| timed.offset);
            let next_scheduled = self.scheduled.last().map_or(frames, |(at, _)| {
                usize::try_from(at - context.start_frame).unwrap_or(frames)
            });
            let end = next_event.min(next_scheduled).min(frames);
            self.render(start..end, quiet_limit);
            start = end;
        }

        for (out, channel) in [left_out, right_out].into_iter().zip(&self.mix) {
            for (sample, mixed) in out.iter_mut().zip(channel) {
                *sample = mixed.clamp(-LIMIT, LIMIT);
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
