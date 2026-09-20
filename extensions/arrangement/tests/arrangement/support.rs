//! A probe instrument, a harness on a temporary project folder and helpers to read levels.

use std::path::PathBuf;

use arrangement::{ArrangementState, Colour};
use serde::{Deserialize, Serialize};
use sound_core::{
    AudioOutput, BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, EngineStatus,
    EventInput, InputEndpoint, InstanceId, OutputEndpoint, Ports, PrepareConfig, ProcessContext,
    Processor, Project, Registry, State, Tempo, TempoMap, Ticks, TimeSignature,
};
use sound_notes::{AUDIO_OUTPUT, Clip, Length, NOTES_INPUT, Note, NoteEvent, Pitch, Velocity};

pub const SAMPLE_RATE: u32 = 48_000;
/// Frames per tick at 120 bpm and 48 kHz.
pub const TICK: usize = 25;

/// An instrument that makes no sound but a level: `scale` times the sum of the pitches it
/// holds. So one sample tells which notes are held, and two tracks with scales 1 and 1000 can
/// be told apart in one device channel.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    pub scale: f32,
}

impl State for Probe {
    const TOOL: &'static str = "test.probe";
}

pub struct ProbeProcessor {
    scale: f32,
    /// Held notes per pitch. An `On` adds one. An `Off` releases every note of its pitch, as
    /// the note contract says.
    held: [u32; 128],
}

impl ProbeProcessor {
    const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    const OUTPUT: AudioOutput = AudioOutput::new(0);

    fn level(&self) -> f32 {
        let pitches = self.held.iter().enumerate();
        let sum: u32 = pitches.map(|(pitch, count)| pitch as u32 * count).sum();
        self.scale * sum as f32
    }
}

impl Processor for ProbeProcessor {
    type Update = f32;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, scale: &mut f32) {
        self.scale = *scale;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let events = context.event_inputs.get(Self::NOTES);
        let output = context.audio_outputs.get(Self::OUTPUT);
        let mut events = events.iter().peekable();
        for (frame, sample) in output.iter_mut().enumerate() {
            while let Some(timed) = events.next_if(|timed| timed.offset <= frame) {
                match timed.event {
                    NoteEvent::On { pitch, .. } => self.held[usize::from(pitch.number())] += 1,
                    NoteEvent::Off { pitch } => self.held[usize::from(pitch.number())] = 0,
                    NoteEvent::AllOff => self.held = [0; 128],
                }
            }
            *sample = self.level();
        }
    }
}

fn apply_probe(state: &Probe, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let probe = context.processor("probe", || ProbeProcessor {
        scale: state.scale,
        held: [0; 128],
    })?;
    context.update(probe, state.scale)?;
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

pub fn registry() -> Registry {
    let mut registry = Registry::new();
    arrangement::register(&mut registry).unwrap();
    registry
        .tool::<Probe>("test")
        .unwrap()
        .behaviour(apply_probe);
    registry
}

pub fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

pub fn note(start: u64, length: u64, pitch: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
    }
}

pub fn clip(start: u64, length: u64, notes: Vec<Note>) -> Clip {
    Clip {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        notes,
    }
}

/// The record of a clip as an agent would write it.
pub fn clip_json(clip: &Clip) -> String {
    format!(
        r#"{{"tool": "arrangement.clip", "state": {}}}"#,
        serde_json::to_string(clip).unwrap()
    )
}

pub fn tempo(bpm: f64) -> TempoMap {
    TempoMap::constant(TimeSignature::default(), Tempo::from_bpm(bpm).unwrap())
}

/// An open project on a temporary folder with an offline mono engine and the arrangement
/// `arrangement`.
pub struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// Last, so the folder outlives the project that holds its lock.
    _folder: tempfile::TempDir,
}

impl Harness {
    pub fn new() -> Self {
        let folder = tempfile::tempdir().unwrap();
        let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 1));
        let mut project = Project::open(folder.path(), registry(), control).unwrap();
        let mut changes = Changes::new();
        changes.create(id("arrangement"), ArrangementState {});
        project.commit("Add arrangement", changes).unwrap();
        Self {
            project,
            engine,
            _folder: folder,
        }
    }

    /// One track `arrangement/<name>` with a probe of this scale.
    pub fn add_track(&mut self, name: &str, scale: f32) {
        let mut changes = Changes::new();
        let track = arrangement::add_track(
            &self.project,
            &mut changes,
            &id("arrangement"),
            name,
            Colour::Blue,
            Probe { scale },
        );
        assert_eq!(track.unwrap().id(), &id(&format!("arrangement/{name}")));
        self.project.commit("Add track", changes).unwrap();
    }

    /// A track `arrangement/piano` with scale 1 and these clips, named `clip-0`, `clip-1`, ...
    pub fn with_clips(clips: Vec<Clip>) -> Self {
        let mut harness = Self::new();
        harness.add_track("piano", 1.0);
        let mut changes = Changes::new();
        for (index, clip) in clips.into_iter().enumerate() {
            changes.create(id(&format!("arrangement/piano/clip-{index}")), clip);
        }
        harness.project.commit("Add clips", changes).unwrap();
        harness
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    /// Writes a file and applies it, as the watcher would. Returns how many records changed.
    pub fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        self.project.apply_outside_changes(&[path]).unwrap()
    }

    pub fn apply(&mut self, relative: &[&str]) -> usize {
        let paths: Vec<PathBuf> = relative.iter().map(|path| self.path(path)).collect();
        self.project.apply_outside_changes(&paths).unwrap()
    }

    pub fn problems(&self) -> Vec<String> {
        let problems = self.project.problems().into_iter();
        problems
            .map(|problem| format!("{}: {}", problem.path, problem.message))
            .collect()
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let (output, status) = self.render_with_status(frames);
        assert_eq!(status.event_overflows, 0);
        output
    }

    pub fn render_with_status(&mut self, frames: usize) -> (Vec<f32>, EngineStatus) {
        let mut output = vec![0.0; frames];
        for buffer in output.chunks_mut(480) {
            self.engine.process_block(buffer);
        }
        let status = self.project.engine().poll().unwrap();
        assert_eq!(status.port_misuses, 0);
        (output, status)
    }

    pub fn play(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().play();
        self.render(frames)
    }
}

/// Every frame at which the level changes, with the new level. The level before frame 0 is 0.
pub fn level_changes(samples: &[f32]) -> Vec<(usize, f32)> {
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
