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
    AudioInput, AudioInputs, AudioOutput, Automation, CHANNELS, EventInput, EventOutput,
    EventOutputs, EventTap, Lane, MAX_BLOCK, Ports, PrepareConfig, ProcessContext, Processor,
    Smoothed, Timed, Watch,
};
use sound_notes::{
    NoteEvent, Performed, Pitch, Schedule, Scheduled, Velocity, Voice, Voices, frequency_hz,
};

use crate::cable::{Bang, Control, Number};
use crate::code::{
    Code, MAX_DEVICE_NUMBERS, MAX_LIVES, MAX_OUTPUTS, MAX_PARAMETERS, MAX_STAGE_PORTS,
};
use crate::machine::{Block, Frames, Inputs, LIMIT, Machine, Note, QUIET, Values, held};

/// How long the old code fades out while the new one fades in.
const FADE_SECONDS: f32 = 0.01;
/// How long a new value of a param or a live control takes to arrive, also a lane that takes a
/// param over or lets it go. A jump would click.
const RAMP_SECONDS: f32 = 0.02;
/// A released voice this quiet for this long, with no delay that holds a sound, is over and free
/// for another note. Quiet for as long again as its delays reach, it is over whatever they hold:
/// what goes round a loop that long unheard is never heard.
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
    /// One voice, always running: it hears `input` and follows the newest held note.
    One,
    /// The code runs once per note, up to `voices` at once.
    Instrument { voices: usize },
}

impl Kind {
    /// How many machines a processor of this kind plays, so the behaviour makes that many.
    pub fn machines(self) -> usize {
        match self {
            Self::Instrument { .. } => MAX_VOICES,
            Self::One => 1,
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
    /// From the interface: a key or another note event, as if it came in on
    /// [`Signals::NOTES`]. Not saved.
    Note(Performed),
    /// From the behaviour: where to write what comes in on [`Signals::NOTES`], for the
    /// interface, or nowhere. The one it replaces rides back.
    Tap(Option<EventTap<NoteEvent>>),
}

/// What the interface plays at a frame.
#[derive(Copy, Clone)]
enum Played {
    Note(NoteEvent),
    Trigger(usize),
}

impl From<NoteEvent> for Played {
    fn from(event: NoteEvent) -> Self {
        Self::Note(event)
    }
}

impl Scheduled for Played {
    fn note(&self) -> Option<NoteEvent> {
        match self {
            Self::Note(event) => Some(*event),
            Self::Trigger(_) => None,
        }
    }
}

pub struct Signals {
    players: Players,
    parameters: [Smoothed; MAX_PARAMETERS],
    /// Where each param stands in the record, and where it was last aimed.
    records: [f32; MAX_PARAMETERS],
    aimed: [f32; MAX_PARAMETERS],
    /// The param each automation lane moves, see [`Values::automated`].
    automated: Vec<u16>,
    /// How fast the lane of each param moves, decided as a built-in device decides it.
    lanes: [Lane; MAX_PARAMETERS],
    /// Whether a block played yet: in the first, the lanes take their values at once.
    followed: bool,
    lives: [Smoothed; MAX_LIVES],
    /// What every voice reads in this block.
    block: Box<Block>,
    /// What the voices play in this block, added up.
    mix: Frames,
    /// What one voice plays in a span, and what its old code plays while it fades out.
    voice_frames: [Frames; 2],
    /// Whether it has the ports of a stage: what it takes from before and hands on.
    stage_ports: bool,
    /// What it hands on in this block, and what its old code hands on while it fades out.
    exports: Box<[[Frames; MAX_STAGE_PORTS]; 2]>,
    /// How many params and live controls the code has, so a block moves only those.
    counts: (usize, usize),
    arrays: Vec<Vec<f32>>,
    /// What the interface plays at a frame of engine time.
    scheduled: Schedule<Played>,
    watches: Vec<Watch>,
    tap: Option<EventTap<NoteEvent>>,
    sending: Sending,
    /// Where the transport was in the last frame: it stands still while stopped.
    beat: f64,
    /// What a copy left of moving the beat on: by how much a frame, for how many frames. The
    /// next block that reads the beat adds it frame by frame, so it rounds as the code would.
    owed_beat: (f64, usize),
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

/// The one voice of an effect or a source, and the keys it follows.
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

/// What the bangs and the notes a sound sends out were in the last frame: whether the signal of
/// each bang was above 0, and the note each notes holds.
struct Sending {
    bangs: [bool; MAX_OUTPUTS],
    notes: [Option<Pitch>; MAX_OUTPUTS],
}

/// The keys an effect or a source follows: the newest held one plays, and the note ends when
/// none is held.
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
    /// The live controls and the triggers, as cables reach them.
    pub const CONTROLS: EventInput<Control> = EventInput::new(2);

    /// The port of the number at `index` of [`Code::numbers`].
    pub const fn number(index: usize) -> EventOutput<Number> {
        EventOutput::new(index)
    }

    /// The port of the bang at `index` of [`Code::bangs`].
    pub const fn bang(index: usize) -> EventOutput<Bang> {
        EventOutput::new(MAX_OUTPUTS + index)
    }

    /// The port of the notes at `index` of [`Code::notes`].
    pub const fn notes(index: usize) -> EventOutput<NoteEvent> {
        EventOutput::new(2 * MAX_OUTPUTS + index)
    }

    /// The port of the number of a device at `index` of what a stage moves.
    pub const fn device_number(index: usize) -> EventOutput<Number> {
        EventOutput::new(3 * MAX_OUTPUTS + index)
    }

    /// The port of a stage that takes the signal at `index` from before.
    pub const fn import(index: usize) -> AudioInput {
        AudioInput::new(1 + index)
    }

    /// The port of a stage that hands on the signal at `index`.
    pub const fn export(index: usize) -> AudioOutput {
        AudioOutput::new(1 + index)
    }

    /// As a stage of a sound that plays through devices, with the ports of one. A processor
    /// has its ports for good, and the others need none.
    #[must_use]
    pub fn with_stage_ports(mut self) -> Self {
        self.stage_ports = true;
        self
    }

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
            Kind::One => Players::One(Box::new(Single {
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
            lanes: [Lane::default(); MAX_PARAMETERS],
            followed: false,
            lives,
            block: Box::new(Block::new()),
            mix: [[0.0; MAX_BLOCK]; CHANNELS],
            voice_frames: [[[0.0; MAX_BLOCK]; CHANNELS]; 2],
            stage_ports: false,
            exports: Box::new([[[[0.0; MAX_BLOCK]; CHANNELS]; MAX_STAGE_PORTS]; 2]),
            counts,
            arrays: values.arrays.unwrap_or_default(),
            scheduled: Schedule::new(MAX_SCHEDULED),
            watches,
            tap: None,
            sending: Sending {
                bangs: [false; MAX_OUTPUTS],
                notes: [None; MAX_OUTPUTS],
            },
            beat: 0.0,
            owed_beat: (0.0, 0),
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
                self.sending.follow(old, code);
            }
            self.parameters = values.parameters.map(Smoothed::new);
            self.aimed = values.parameters;
            std::mem::swap(&mut self.watches, watches);
        }
        // Aimed at in the next block, unless a lane moves them.
        self.records = values.parameters;
        let ramp = RAMP_SECONDS * self.sample_rate;
        for lane in &mut self.lanes {
            lane.edited(ramp);
        }
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
                voice.fade_left = fade_frames;
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

    /// Aims every param at its lane in this block, or at its record when no lane moves it. A
    /// lane that moves ramps over the block; one that takes a param over or lets it go glides.
    fn aim(&mut self, context: &ProcessContext<'_>) {
        let mut heard = [None; MAX_PARAMETERS];
        let specs = self.players.code().map(|code| &code.parameters[..]);
        for lane in context.event_inputs.get(Self::AUTOMATION) {
            let Some(param) = self.automated.get(usize::from(lane.event.parameter)) else {
                continue;
            };
            let param = usize::from(*param);
            let spec = specs.and_then(|specs| specs.get(param));
            if let (Some(heard), Some(spec)) = (heard.get_mut(param), spec) {
                // Not `clamp`: it panics on a NaN, and nothing may panic on the audio thread.
                *heard = Some(lane.event.value.max(spec.min).min(spec.max));
            }
        }
        let first = !std::mem::replace(&mut self.followed, true);
        let jumped = context.transport.jumped;
        let block = context.frames as f32;
        let edit = RAMP_SECONDS * self.sample_rate;
        let params = (self.parameters.iter_mut().zip(&mut self.aimed)).zip(&mut self.lanes);
        let params = params.zip(self.records.iter().zip(heard));
        for (((parameter, aimed), lane), (record, heard)) in params.take(self.counts.0) {
            let target = heard.unwrap_or(*record);
            let ramp = lane.hear(heard, block, first, jumped, edit);
            match ramp {
                Some(ramp) => parameter.set_target(target, ramp),
                // An edit of the record glides.
                None if *aimed != target => parameter.set_target(target, edit),
                None => continue,
            }
            *aimed = target;
        }
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
        // An idle voice is silent, so it has nothing to fade from.
        if self.is_idle() {
            self.fade_left = 0;
        }
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
        (mix, exports): (&mut Frames, Option<&mut [[Frames; MAX_STAGE_PORTS]; 2]>),
    ) {
        let [exports, old_exports] = match exports {
            Some([exports, old_exports]) => [&mut exports[..], &mut old_exports[..]],
            None => [&mut [][..], &mut [][..]],
        };
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
                // Quiet long enough, it waits for its delays a part at a time.
                let quiet_left = shared.quiet_limit.saturating_sub(self.quiet_frames);
                if quiet_left > 0 {
                    end = end.min(start + quiet_left);
                }
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
            self.machine
                .render(&inputs, part.clone(), sound, &mut *exports);
            if self.fade_left > 0
                && let Some(fading) = &mut self.fading
            {
                // Old code may hand on fewer signals: the rest fade in from silence.
                for channel in old_exports.iter_mut().flatten() {
                    channel.get_mut(part.clone()).unwrap_or_default().fill(0.0);
                }
                fading.render(&inputs, part.clone(), old, &mut *old_exports);
                for frame in part.clone() {
                    let new = 1.0 - self.fade_left as f32 / shared.fade_frames as f32;
                    let faded = std::iter::once((&mut *sound, &*old))
                        .chain(exports.iter_mut().zip(old_exports.iter()));
                    for (sound, old) in faded.flat_map(|(sound, old)| sound.iter_mut().zip(old)) {
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
                    } else {
                        self.quiet_frames = 0;
                    }
                }
                *mix_left += left;
                *mix_right += right;
            }
            if ending && self.quiet_frames >= shared.quiet_limit {
                let unheard = self.quiet_frames - shared.quiet_limit >= self.machine.delays_reach();
                self.sounding = !unheard && self.machine.delays_sound();
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
    fn fill(&mut self, inputs: &AudioInputs<'_>, frames: usize, bpm: f64, playing: bool) {
        let block = &mut *self.block;
        let imports = self.players.code().map_or(0, |code| code.imports);
        let ports = std::iter::once(Self::INPUT).chain((0..imports).map(Self::import));
        let rows = std::iter::once(&mut block.input).chain(&mut block.imports);
        for (rows, port) in rows.zip(ports) {
            for (row, samples) in rows.iter_mut().zip(inputs.get(port)) {
                for (frame, sample) in row.iter_mut().take(frames).enumerate() {
                    *sample = samples.get(frame).copied().unwrap_or(0.0);
                }
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
            Players::One(single) => {
                let exports = Some(&mut *self.exports);
                single
                    .voice
                    .render(&shared, frames, voice_frames, (mix, exports));
            }
            // An instrument plays through no device, so it hands nothing on.
            Players::Many(voices) => {
                for voice in voices.iter_mut().filter(|voice| !voice.is_idle()) {
                    voice.render(&shared, frames.clone(), voice_frames, (mix, None));
                }
            }
        }
    }
}

impl Processor for Signals {
    type Update = SignalsUpdate;

    fn ports(&self) -> Ports {
        let ports = Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
            .event_input(Self::NOTES)
            .event_input(Self::AUTOMATION)
            .event_input(Self::CONTROLS);
        let ports = (0..MAX_OUTPUTS).fold(ports, |ports, index| {
            ports.event_output(Self::number(index))
        });
        let ports =
            (0..MAX_OUTPUTS).fold(ports, |ports, index| ports.event_output(Self::bang(index)));
        let ports =
            (0..MAX_OUTPUTS).fold(ports, |ports, index| ports.event_output(Self::notes(index)));
        if !self.stage_ports {
            return ports;
        }
        let ports = (0..MAX_STAGE_PORTS).fold(ports, |ports, index| {
            (ports.audio_input(Self::import(index))).audio_output(Self::export(index))
        });
        (0..MAX_DEVICE_NUMBERS).fold(ports, |ports, index| {
            ports.event_output(Self::device_number(index))
        })
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
                    self.scheduled.add(*at, Played::Trigger(*index));
                }
            }
            SignalsUpdate::Note(performed) => {
                if let Some(now) = self.scheduled.perform(*performed) {
                    self.follow(now);
                }
            }
            SignalsUpdate::Tap(tap) => std::mem::swap(&mut self.tap, tap),
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some(tap) = &self.tap {
            let notes = context.event_inputs.get(Self::NOTES).iter();
            tap.write(notes.map(|timed| (context.start_frame + timed.offset as u64, timed.event)));
        }
        for _ in 0..self.scheduled.take_dropped() {
            context.event_outputs.count_dropped();
        }
        if self.copies() {
            self.copy(context);
            return;
        }
        self.settle_beat();
        self.aim(context);
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
        self.scheduled.playing(context.start_frame, context.frames);
        let ramp = RAMP_SECONDS * self.sample_rate;
        let specs = self.players.code().map_or(&[][..], |code| &code.lives[..]);
        for timed in context.event_inputs.get(Self::CONTROLS) {
            if let Control::Live { index, value } = timed.event
                && let (Some(live), Some(spec)) = (
                    self.lives.get_mut(usize::from(index)),
                    specs.get(usize::from(index)),
                )
            {
                live.set_target(value.max(spec.min).min(spec.max), ramp);
            }
        }
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
        let next = context.start_frame + context.frames as u64;
        if silent
            && context.event_inputs.get(Self::NOTES).is_empty()
            && self.scheduled.next().is_none_or(|at| at >= next)
        {
            left_out.fill(0.0);
            right_out.fill(0.0);
            return;
        }
        self.fill(&context.audio_inputs, frames, bpm, playing);
        // After `fill`, which clears the triggers, where the live controls had to come before.
        for timed in context.event_inputs.get(Self::CONTROLS) {
            if let Control::Trigger { index } = timed.event {
                self.block.fire(usize::from(index), timed.offset);
            }
        }
        for channel in &mut self.mix {
            channel.fill(0.0);
        }

        let notes = context.event_inputs.get(Self::NOTES);
        self.spans((notes, context.start_frame), frames, Some(quiet_limit));

        for (out, channel) in [left_out, right_out].into_iter().zip(&self.mix) {
            for (sample, mixed) in out.iter_mut().zip(channel) {
                *sample = mixed.clamp(-LIMIT, LIMIT);
            }
        }
        let exported = self.players.code().map_or(0, |code| code.exports.len());
        for (index, exports) in self.exports[0].iter().enumerate().take(exported) {
            let outputs = context.audio_outputs.get(Self::export(index));
            for (out, channel) in outputs.into_iter().zip(exports) {
                // A fade between two values near the ends of `f32` overflows.
                for (sample, exported) in out.iter_mut().zip(channel) {
                    *sample = if exported.is_finite() { *exported } else { 0.0 };
                }
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
            // Every block, so a device it drives hears it is still there. An instrument with no
            // note sends none, and lets go.
            for (index, value) in voice.machine.sent().iter().enumerate() {
                (context.event_outputs).push(Self::number(index), 0, Number(*value));
            }
            for (index, value) in voice.machine.device_numbers().iter().enumerate() {
                (context.event_outputs).push(Self::device_number(index), 0, Number(*value));
            }
        }
        // An instrument sends none: a voice that ends or loses its place would hold a note.
        if let Players::One(single) = &self.players {
            let machine = &single.voice.machine;
            let counts = (machine.code().bangs.len(), machine.code().notes.len());
            let events = machine.events();
            (self.sending).send(events, counts, frames, &mut context.event_outputs);
        }
    }
}

impl Signals {
    /// Whether its code passes its input as it comes, with nothing that glides and no old code
    /// fading out: the code of a tool with no sound, which a copy plays.
    fn copies(&self) -> bool {
        let passes = |single: &Single| {
            single.voice.fade_left == 0 && single.voice.machine.code().passes_input()
        };
        self.counts == (0, 0) && matches!(&self.players, Players::One(single) if passes(single))
    }

    /// Plays a block as [`Self::copies`] says. What code that comes after reads goes on as
    /// the code would move it: the beat, and the keys the notes hold.
    fn copy(&mut self, context: &mut ProcessContext<'_>) {
        let frames = context.frames.min(MAX_BLOCK);
        self.followed = true;
        let transport = &context.transport;
        if let Some(quarters) = transport.quarters() {
            self.beat = quarters;
            let bpm = transport.clock.tempo_at(transport.tick_range.start).bpm();
            self.owed_beat = (bpm / 60.0 / f64::from(self.sample_rate), frames);
        }
        self.scheduled.playing(context.start_frame, context.frames);
        let notes = context.event_inputs.get(Self::NOTES);
        self.spans((notes, context.start_frame), frames, None);
        if let Players::One(single) = &mut self.players {
            single.voice.note.onset = false;
        }
        let input = context.audio_inputs.get(Self::INPUT);
        let output = context.audio_outputs.get(Self::OUTPUT);
        // An output starts silent, and an input that nothing connects is empty.
        for (output, input) in output.into_iter().zip(input) {
            for (sample, input) in output.iter_mut().zip(input) {
                *sample = held(*input);
            }
        }
    }

    fn settle_beat(&mut self) {
        let (beats_per_frame, frames) = std::mem::take(&mut self.owed_beat);
        for _ in 0..frames {
            self.beat += beats_per_frame;
        }
    }

    /// Follows the notes and what the interface plays over the first `frames` of the block, in
    /// spans from each to the next, and renders each span when given how long a voice may be
    /// quiet.
    fn spans(
        &mut self,
        (notes, start_frame): (&[Timed<NoteEvent>], u64),
        frames: usize,
        render: Option<usize>,
    ) {
        let mut events = notes.iter().peekable();
        let mut start = 0;
        while start < frames {
            while let Some(timed) = events.next_if(|timed| timed.offset <= start) {
                self.follow(timed.event);
            }
            let now = start_frame + start as u64;
            while let Some((_, what)) = self.scheduled.take(now) {
                match what {
                    Played::Note(event) => self.follow(event),
                    Played::Trigger(index) => self.block.fire(index, start),
                }
            }
            let next_event = events.peek().map_or(frames, |timed| timed.offset);
            let next_scheduled = self.scheduled.next().map_or(frames, |at| {
                usize::try_from(at - start_frame).unwrap_or(frames)
            });
            let end = next_event.min(next_scheduled).min(frames);
            if let Some(quiet_limit) = render {
                self.render(start..end, quiet_limit);
            }
            start = end;
        }
    }
}

impl Sending {
    /// Follows the outputs from `old` code to `new` by name: the port of an output may move. An
    /// output that is gone forgets its note: the feed it played into lets go when it changes.
    fn follow(&mut self, old: &Code, new: &Code) {
        let mut sending = Self {
            bangs: [false; MAX_OUTPUTS],
            notes: [None; MAX_OUTPUTS],
        };
        for (index, name) in new.bangs.iter().enumerate() {
            if let (Some(old), Some(bang)) = (
                old.bangs.iter().position(|old| old == name),
                sending.bangs.get_mut(index),
            ) {
                *bang = self.bangs.get(old).copied().unwrap_or(false);
            }
        }
        for (index, name) in new.notes.iter().enumerate() {
            if let (Some(old), Some(note)) = (
                old.notes.iter().position(|old| old == name),
                sending.notes.get_mut(index),
            ) {
                *note = self.notes.get(old).copied().flatten();
            }
        }
        *self = sending;
    }

    /// Sends what `events` of the bangs and the notes of the code give in the first `frames`:
    /// a bang where its signal goes above 0, and a note while its gate is above 0, at the
    /// pitch and velocity it starts with, ended where the gate falls or the pitch moves.
    fn send(
        &mut self,
        events: &[[f32; MAX_BLOCK]],
        (bangs, notes): (usize, usize),
        frames: usize,
        outputs: &mut EventOutputs<'_>,
    ) {
        let (bang_rows, note_rows) = events.split_at(bangs.min(events.len()));
        for (index, (row, was_above)) in bang_rows.iter().zip(&mut self.bangs).enumerate() {
            for (frame, value) in row.iter().take(frames).enumerate() {
                let above = *value > 0.0;
                if above && !*was_above {
                    outputs.push(Signals::bang(index), frame, Bang);
                }
                *was_above = above;
            }
        }
        let mut rows = note_rows.chunks_exact(3).take(notes);
        for (index, held) in self.notes.iter_mut().enumerate() {
            let port = Signals::notes(index);
            let Some([gate, pitch, velocity]) = rows.next() else {
                continue;
            };
            let frames = gate
                .iter()
                .zip(pitch)
                .zip(velocity)
                .take(frames)
                .enumerate();
            for (frame, ((gate, pitch), velocity)) in frames {
                let wanted = (*gate > 0.0).then(|| Pitch::nearest(pitch.round() as i64));
                if wanted == *held {
                    continue;
                }
                if let Some(pitch) = *held {
                    // A dropped off would hold the note for ever: what follows waits for it.
                    if !outputs.push(port, frame, NoteEvent::Off { pitch }) {
                        break;
                    }
                    *held = None;
                }
                if let Some(pitch) = wanted {
                    let velocity = Velocity::nearest((velocity * 127.0).round() as i64);
                    // A note that does not fit is not held, so no off goes after it.
                    if outputs.push(port, frame, NoteEvent::On { pitch, velocity }) {
                        *held = Some(pitch);
                    }
                }
            }
        }
    }
}
