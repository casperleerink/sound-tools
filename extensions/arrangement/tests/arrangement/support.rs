//! A probe instrument, a harness on a temporary project folder and helpers to read levels.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use arrangement::{ArrangementState, Colour};
use serde::{Deserialize, Serialize};
use sound_core::{
    AudioOutput, BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, EngineStatus,
    EventInput, InputEndpoint, InstanceId, OutputEndpoint, Ports, PrepareConfig, ProcessContext,
    Processor, Project, Registry, State, Tempo, TempoMap, Ticks, TimeSignature,
};
use sound_notes::{
    AUDIO_OUTPUT, Amount, Bend, Clip, Expression, Length, NOTES_INPUT, Note, NoteEvent, Pedal,
    Pitch, Point, Velocity,
};

pub(crate) const SAMPLE_RATE: u32 = 48_000;
/// Frames per tick at 120 bpm and 48 kHz.
pub(crate) const TICK: usize = 25;

/// An instrument that makes no sound but a level: `scale` times the sum of the pitches it
/// holds. So one sample tells which notes are held, and two tracks with scales 1 and 1000 can
/// be told apart in one device channel. It can show where a lane stands instead.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Probe {
    pub scale: f32,
    #[serde(default)]
    pub shows: Shows,
}

/// What the level of a [`Probe`] is.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Shows {
    /// `scale` times the sum of the held pitches.
    #[default]
    Notes,
    /// Where the bend wheel stands, as a number, with no scale.
    Bend,
    ModWheel,
    Pressure,
    /// Where the bend wheel stood when the last note started, so a test sees whether the note
    /// started after the wheel on its frame.
    BendOfLastOn,
}

impl State for Probe {
    const TOOL: &'static str = "test.probe";
}

pub(crate) struct ProbeProcessor {
    probe: Probe,
    /// Held notes per pitch. An `On` adds one. An `Off` releases every note of its pitch, as
    /// the note contract says.
    held: [u32; 128],
    /// Notes whose key came up while the pedal was down. They sound until it comes up, which
    /// is what the note contract asks of an instrument.
    sustained: [u32; 128],
    pedal: Pedal,
    expression: Expression,
    bend_of_last_on: Bend,
}

impl ProbeProcessor {
    const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    const OUTPUT: AudioOutput = AudioOutput::new(0);

    pub(crate) fn new(probe: Probe) -> Self {
        Self {
            probe,
            held: [0; 128],
            sustained: [0; 128],
            pedal: Pedal::UP,
            expression: Expression::REST,
            bend_of_last_on: Bend::MIDDLE,
        }
    }

    fn level(&self) -> f32 {
        let pitches = self.held.iter().zip(&self.sustained).enumerate();
        let sum: u32 = pitches
            .map(|(pitch, (held, sustained))| pitch as u32 * (held + sustained))
            .sum();
        match self.probe.shows {
            Shows::Notes => self.probe.scale * sum as f32,
            Shows::Bend => f32::from(self.expression.bend.value()),
            Shows::ModWheel => f32::from(self.expression.mod_wheel.value()),
            Shows::Pressure => f32::from(self.expression.pressure.value()),
            Shows::BendOfLastOn => f32::from(self.bend_of_last_on.value()),
        }
    }

    fn handle(&mut self, event: NoteEvent) {
        self.expression.follow(event);
        match event {
            NoteEvent::On { pitch, .. } => {
                self.held[usize::from(pitch.number())] += 1;
                self.bend_of_last_on = self.expression.bend;
            }
            NoteEvent::Off { pitch } => {
                let pitch = usize::from(pitch.number());
                if self.pedal.is_down() {
                    self.sustained[pitch] += self.held[pitch];
                } else {
                    self.sustained[pitch] = 0;
                }
                self.held[pitch] = 0;
            }
            NoteEvent::Pedal(value) => {
                if self.pedal.is_down() && !value.is_down() {
                    self.sustained = [0; 128];
                }
                self.pedal = value;
            }
            NoteEvent::AllOff => {
                self.held = [0; 128];
                self.sustained = [0; 128];
                self.pedal = Pedal::UP;
            }
            NoteEvent::Bend(_) | NoteEvent::ModWheel(_) | NoteEvent::Pressure(_) => {}
        }
    }
}

impl Processor for ProbeProcessor {
    type Update = Probe;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, probe: &mut Probe) {
        self.probe = *probe;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let events = context.event_inputs.get(Self::NOTES);
        // The level in both channels, as an instrument in the middle gives it.
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        let mut events = events.iter().peekable();
        for (frame, sample) in left.iter_mut().enumerate() {
            while let Some(timed) = events.next_if(|timed| timed.offset <= frame) {
                self.handle(timed.event);
            }
            *sample = self.level();
        }
        right.copy_from_slice(left);
    }
}

fn apply_probe(state: &Probe, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let probe = context.processor("probe", || ProbeProcessor::new(*state))?;
    context.update(probe, *state)?;
    context.input(
        NOTES_INPUT,
        InputEndpoint::new(probe, ProbeProcessor::NOTES),
    );
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(probe, ProbeProcessor::OUTPUT),
    );
    Ok(())
}

/// A test effect: every sample becomes `sample * gain + offset`, and what it played a frame
/// ago comes back times `tail`.
///
/// It has the ports of an effect and nothing else, like a hosted plugin in an effect slot, so
/// the arrangement needs no plugin to be told what a chain does. The offset is what makes two
/// of them say by their samples which came first: with gains of a half, A then B is
/// `x / 4 + offset_a / 2 + offset_b`, and the other way round the last two swap. The tail is
/// what a delay or a reverb has, and it is 0 unless a test asks for one.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Trim {
    pub gain: f32,
    #[serde(default)]
    pub offset: f32,
    #[serde(default)]
    pub tail: f32,
}

impl Trim {
    pub(crate) fn new(gain: f32, offset: f32) -> Self {
        Self {
            gain,
            offset,
            tail: 0.0,
        }
    }
}

impl State for Trim {
    const TOOL: &'static str = "test.trim";
}

pub(crate) struct TrimProcessor {
    settings: Trim,
    /// What each channel played a frame ago, which is the whole of its tail.
    held: [f32; 2],
}

impl TrimProcessor {
    const INPUT: sound_core::AudioInput = sound_core::AudioInput::new(0);
    const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for TrimProcessor {
    type Update = Trim;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, settings: &mut Trim) {
        self.settings = *settings;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let input = context.audio_inputs.get(Self::INPUT);
        let output = context.audio_outputs.get(Self::OUTPUT);
        let Trim { gain, offset, tail } = self.settings;
        for ((output, input), held) in output.into_iter().zip(input).zip(&mut self.held) {
            for (sample, played) in output.iter_mut().zip(input) {
                *sample = played * gain + offset + *held * tail;
                *held = *sample;
            }
        }
    }
}

fn apply_trim(state: &Trim, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let trim = context.processor("trim", || TrimProcessor {
        settings: *state,
        held: [0.0; 2],
    })?;
    context.update(trim, *state)?;
    context.input(
        sound_notes::AUDIO_INPUT,
        InputEndpoint::new(trim, TrimProcessor::INPUT),
    );
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(trim, TrimProcessor::OUTPUT),
    );
    Ok(())
}

/// A test effect a sidechain keys: each sample is divided by one plus the key, the left
/// channel of its `sidechain` input, and comes out `latency` frames late, as from a compressor
/// with a lookahead. With nothing in its sidechain it only delays. The latency is the one it
/// was made with.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Duck {
    #[serde(default)]
    pub latency: u32,
}

impl State for Duck {
    const TOOL: &'static str = "test.duck";
}

pub(crate) struct DuckProcessor {
    /// The frames still to come out, oldest first: `latency` of them.
    late: std::collections::VecDeque<f32>,
}

impl DuckProcessor {
    const INPUT: sound_core::AudioInput = sound_core::AudioInput::new(0);
    const KEY: sound_core::AudioInput = sound_core::AudioInput::new(1);
    const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for DuckProcessor {
    type Update = Duck;

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .side_audio_input(Self::KEY)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut Duck) {}

    fn latency(&self) -> u32 {
        self.late.len() as u32
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let [input, _] = context.audio_inputs.get(Self::INPUT);
        let [key, _] = context.audio_inputs.get(Self::KEY);
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for ((output, input), key) in left.iter_mut().zip(input).zip(key) {
            self.late.push_back(*input);
            let played = self.late.pop_front().unwrap_or_default();
            *output = played / (1.0 + key.abs());
        }
        right.copy_from_slice(left);
    }
}

fn apply_duck(state: &Duck, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    // Room for one more, so a frame goes in before one comes out with no allocation.
    let latency = state.latency as usize;
    let mut late = std::collections::VecDeque::with_capacity(latency + 1);
    late.extend(std::iter::repeat_n(0.0, latency));
    let duck = context.processor("duck", || DuckProcessor { late })?;
    context.update(duck, *state)?;
    context.input(
        sound_notes::AUDIO_INPUT,
        InputEndpoint::new(duck, DuckProcessor::INPUT),
    );
    context.input(
        sound_notes::SIDECHAIN_INPUT,
        InputEndpoint::new(duck, DuckProcessor::KEY),
    );
    context.output(
        AUDIO_OUTPUT,
        OutputEndpoint::new(duck, DuckProcessor::OUTPUT),
    );
    Ok(())
}

pub(crate) fn registry() -> Registry {
    let mut registry = Registry::new();
    arrangement::register(&mut registry).unwrap();
    registry
        .tool::<Probe>("test")
        .unwrap()
        .behaviour(apply_probe);
    registry.tool::<Trim>("test").unwrap().behaviour(apply_trim);
    registry.tool::<Duck>("test").unwrap().behaviour(apply_duck);
    registry
}

pub(crate) fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

pub(crate) fn note(start: u64, length: u64, pitch: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

pub(crate) fn clip(start: u64, length: u64, notes: Vec<Note>) -> Clip {
    Clip::new(Ticks(start), Length::new(Ticks(length)).unwrap(), notes)
}

/// A clip with pedal moves, each `(start, value)` counted from the clip start.
pub(crate) fn clip_with_pedal(
    start: u64,
    length: u64,
    notes: Vec<Note>,
    pedal: &[(u64, u8)],
) -> Clip {
    let mut clip = clip(start, length, notes);
    clip.pedal = pedal
        .iter()
        .map(|&(start, value)| sound_notes::PedalChange {
            start: Ticks(start),
            value: Pedal::new(value).unwrap(),
        })
        .collect();
    clip
}

/// A point of a bend lane, counted from the clip start.
pub(crate) fn bend(tick: u64, value: i16) -> Point<Bend> {
    Point {
        tick: Ticks(tick),
        value: Bend::new(value).unwrap(),
    }
}

/// A point of a mod wheel or pressure lane.
pub(crate) fn amount(tick: u64, value: u8) -> Point<Amount> {
    Point {
        tick: Ticks(tick),
        value: Amount::new(value).unwrap(),
    }
}

/// The record of a clip as an agent would write it.
pub(crate) fn clip_json(clip: &Clip) -> String {
    format!(
        r#"{{"tool": "arrangement.clip", "state": {}}}"#,
        serde_json::to_string(clip).unwrap()
    )
}

pub(crate) fn tempo(bpm: f64) -> TempoMap {
    TempoMap::constant(TimeSignature::default(), Tempo::from_bpm(bpm).unwrap())
}

/// An arrangement whose limiter is off. The probe makes levels such as 60 that are numbers and
/// not sound, and the limiter would take them down to its ceiling. The tests of the limiter
/// turn it on.
pub(crate) fn probe_arrangement() -> ArrangementState {
    let mut arrangement = ArrangementState::default();
    arrangement.master.limiter.bypass = true;
    arrangement
}

/// An open project on a temporary folder with an offline mono engine and the arrangement
/// `arrangement`.
pub(crate) struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// The time of the last outside change. Each one comes a minute after the one before, so
    /// it is an undo step of its own, as for changes made by hand. Without this, outside
    /// changes that a test makes within milliseconds would join (`OUTSIDE_UNDO_WINDOW`).
    now: Instant,
    /// Last, so the folder outlives the project that holds its lock.
    _folder: tempfile::TempDir,
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::with_config(EngineConfig::new(SAMPLE_RATE, 1))
    }

    pub(crate) fn with_config(config: EngineConfig) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let (control, engine) = Engine::new(config);
        let mut project = Project::open(folder.path(), registry(), control).unwrap();
        let mut changes = Changes::new();
        changes.create(id("arrangement"), probe_arrangement());
        project.commit("Add arrangement", changes).unwrap();
        Self {
            project,
            engine,
            now: Instant::now(),
            _folder: folder,
        }
    }

    /// One track `arrangement/<name>` with a probe of this scale.
    pub(crate) fn add_track(&mut self, name: &str, scale: f32) {
        let shows = Shows::Notes;
        self.add_probe_track(name, Probe { scale, shows });
    }

    pub(crate) fn add_probe_track(&mut self, name: &str, probe: Probe) {
        let mut changes = Changes::new();
        let track = arrangement::add_track(
            &self.project,
            &mut changes,
            &id("arrangement"),
            name,
            Colour::Blue,
            probe,
        );
        assert_eq!(track.unwrap().id(), &id(&format!("arrangement/{name}")));
        self.project.commit("Add track", changes).unwrap();
    }

    /// A track `arrangement/piano` with scale 1 and these clips, named `clip-0`, `clip-1`, ...
    pub(crate) fn with_clips(clips: Vec<Clip>) -> Self {
        Self::new().and_clips(clips)
    }

    pub(crate) fn and_clips(self, clips: Vec<Clip>) -> Self {
        self.and_clips_showing(Shows::Notes, clips)
    }

    /// The same, with a probe that shows `shows`.
    pub(crate) fn and_clips_showing(self, shows: Shows, clips: Vec<Clip>) -> Self {
        let mut harness = self;
        harness.add_probe_track("piano", Probe { scale: 1.0, shows });
        let mut changes = Changes::new();
        for (index, clip) in clips.into_iter().enumerate() {
            changes.create(id(&format!("arrangement/piano/clip-{index}")), clip);
        }
        harness.project.commit("Add clips", changes).unwrap();
        harness
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    /// Writes a file and applies it, as the watcher would. Returns how many records changed.
    pub(crate) fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        self.apply_paths(&[path])
    }

    pub(crate) fn apply(&mut self, relative: &[&str]) -> usize {
        let paths: Vec<PathBuf> = relative.iter().map(|path| self.path(path)).collect();
        self.apply_paths(&paths)
    }

    fn apply_paths(&mut self, paths: &[PathBuf]) -> usize {
        self.now += Duration::from_secs(60);
        let changed = self.project.apply_outside_changes_at(paths, self.now);
        changed.unwrap()
    }

    pub(crate) fn problems(&self) -> Vec<String> {
        let problems = self.project.problems().into_iter();
        problems
            .map(|problem| format!("{}: {}", problem.path, problem.message))
            .collect()
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    pub(crate) fn render(&mut self, frames: usize) -> Vec<f32> {
        let (output, status) = self.render_with_status(frames);
        assert_eq!(status.event_overflows, 0);
        output
    }

    pub(crate) fn render_with_status(&mut self, frames: usize) -> (Vec<f32>, EngineStatus) {
        let mut output = vec![0.0; frames];
        for buffer in output.chunks_mut(480) {
            self.engine.process_block(buffer);
        }
        let status = self.project.engine().poll().unwrap();
        assert_eq!(status.port_misuses, 0);
        (output, status)
    }

    pub(crate) fn play(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().play();
        self.render(frames)
    }
}

/// Every frame at which the level changes, with the new level. The level before frame 0 is 0.
pub(crate) fn level_changes(samples: &[f32]) -> Vec<(usize, f32)> {
    let mut changes = Vec::new();
    let mut level = 0.0;
    for (frame, sample) in samples.iter().enumerate() {
        if *sample != level {
            level = *sample;
            changes.push((frame, level));
        }
    }
    changes
}

/// A 32-bit float WAV file of these frames at `rate`, under `assets/audio/` of the project,
/// written with `hound`, which is not the code under test. Gives the name a record uses.
pub(crate) fn write_wav(harness: &Harness, name: &str, rate: u32, frames: &[[f32; 2]]) -> String {
    let path = harness.path(&format!("assets/audio/{name}"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for frame in frames {
        writer.write_sample(frame[0]).unwrap();
        writer.write_sample(frame[1]).unwrap();
    }
    writer.finalize().unwrap();
    name.to_string()
}

/// The frames of a stereo render, left and right.
pub(crate) fn stereo(samples: &[f32]) -> Vec<[f32; 2]> {
    samples
        .chunks_exact(2)
        .map(|frame| [frame[0], frame[1]])
        .collect()
}
