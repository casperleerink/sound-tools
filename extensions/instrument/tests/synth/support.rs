//! A test-only track tool, a harness on a temporary project folder and sample checks.

use std::path::PathBuf;
use std::sync::Arc;

use instrument::SynthState;
use serde::{Deserialize, Serialize};
use sound_core::{
    BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, EventOutput, InstanceId,
    OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, Project, Registry, State,
    Ticks,
};
use sound_notes::{AUDIO_OUTPUT, Length, NOTES_INPUT, Note, NoteEvent, Pitch, Velocity};

pub const SAMPLE_RATE: u32 = 48_000;

/// The smallest owner of an instrument: its notes are in its own record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub notes: Vec<Note>,
}

impl State for Track {
    const TOOL: &'static str = "test.track";
    const OWNS_CHILDREN: bool = true;
}

/// The name of the child a track plays.
pub const INSTRUMENT: &str = "instrument";

/// Sends the notes of one immutable snapshot from the transport tick range. It keeps no list
/// of held notes: when the transport stops or jumps it sends one `AllOff`.
#[derive(Default)]
pub struct Sequencer(Arc<Vec<Note>>);

impl Sequencer {
    pub const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Processor for Sequencer {
    type Update = Arc<Vec<Note>>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Arc<Vec<Note>>) {
        // The old snapshot rides back to the control thread inside the update.
        std::mem::swap(&mut self.0, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        if transport.jumped || transport.stopped_playing {
            context
                .event_outputs
                .push(Self::NOTES, 0, NoteEvent::AllOff);
        }
        // All offs before all ons: on one frame, a note that ends must not end the note of
        // the same pitch that starts there.
        for note in self.0.iter() {
            if let Some(offset) = transport.offset_of(note.end()) {
                context.event_outputs.push(Self::NOTES, offset, note.off());
            }
        }
        for note in self.0.iter() {
            if let Some(offset) = transport.offset_of(note.start) {
                context.event_outputs.push(Self::NOTES, offset, note.on());
            }
        }
    }
}

fn apply_track(state: &Track, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let sequencer = context.processor("sequencer", Sequencer::default)?;
    context.update(sequencer, Arc::new(state.notes.clone()))?;
    if let Some(notes) = context.child_input(INSTRUMENT, NOTES_INPUT) {
        context.connect(OutputEndpoint::new(sequencer, Sequencer::NOTES).to(notes))?;
    }
    if let Some(audio) = context.child_output(INSTRUMENT, AUDIO_OUTPUT) {
        for channel in 0..context.device_channels() {
            context.connect(audio.to_device(channel))?;
        }
    }
    Ok(())
}

pub fn registry() -> Registry {
    let mut registry = Registry::new();
    instrument::register(&mut registry).unwrap();
    registry
        .tool::<Track>("test")
        .unwrap()
        .behaviour(apply_track);
    registry
}

pub fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

pub fn note(start: u64, length: u64, pitch: u8, velocity: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

/// An open project on a temporary folder with an offline mono engine.
pub struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// Last, so the folder outlives the project that holds its lock.
    pub folder: tempfile::TempDir,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_config(EngineConfig::new(SAMPLE_RATE, 1))
    }

    pub fn with_config(config: EngineConfig) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let (control, engine) = Engine::new(config);
        let project = Project::open(folder.path(), registry(), control).unwrap();
        Self {
            project,
            engine,
            folder,
        }
    }

    /// One track named `track` with these notes and a synth as its instrument.
    pub fn with_track(notes: Vec<Note>, synth: SynthState) -> Self {
        let mut harness = Self::new();
        harness.add_track("track", notes, synth);
        harness
    }

    pub fn add_track(&mut self, name: &str, notes: Vec<Note>, synth: SynthState) {
        let mut changes = Changes::new();
        let track = changes.create(id(name), Track { notes });
        changes.create(track.id().child(INSTRUMENT).unwrap(), synth);
        self.project.commit("Add track", changes).unwrap();
    }

    /// Writes a file and applies it, as the watcher would.
    pub fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path: PathBuf = self.project.root().join(relative);
        std::fs::write(&path, contents).unwrap();
        self.project.apply_outside_changes(&[path]).unwrap()
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        for buffer in output.chunks_mut(480) {
            self.engine.process_block(buffer);
        }
        let status = self.project.engine().poll().unwrap();
        assert_eq!(status.port_misuses, 0);
        assert_eq!(status.event_overflows, 0);
        output
    }

    /// Plays from the start and renders.
    pub fn play(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().play();
        self.render(frames)
    }
}

pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

pub fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .fold(0.0, |step, pair| step.max((pair[1] - pair[0]).abs()))
}

/// The frames at which the signal crosses zero upward.
pub fn rising_zero_crossings(samples: &[f32]) -> Vec<usize> {
    let pairs = samples.windows(2).enumerate();
    pairs
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(frame, _)| frame + 1)
        .collect()
}

/// The amplitude of the part of the signal at one frequency (one bin of a Fourier transform).
pub fn level_at(samples: &[f32], frequency_hz: f32) -> f32 {
    let step = std::f64::consts::TAU * f64::from(frequency_hz) / f64::from(SAMPLE_RATE);
    let (mut real, mut imaginary) = (0.0, 0.0);
    for (frame, sample) in samples.iter().enumerate() {
        let angle = step * frame as f64;
        real += f64::from(*sample) * angle.cos();
        imaginary += f64::from(*sample) * angle.sin();
    }
    (2.0 * real.hypot(imaginary) / samples.len() as f64) as f32
}
