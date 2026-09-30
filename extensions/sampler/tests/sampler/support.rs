//! A test-only track tool, a harness on a temporary project folder, sample files and checks.

use std::f64::consts::TAU;
use std::path::PathBuf;
use std::sync::Arc;

use sampler::SamplerState;
use serde::{Deserialize, Serialize};
use sound_core::{
    BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, EventOutput, InstanceId,
    OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, Project, Registry, State,
    Ticks,
};
use sound_media::AudioAsset;
use sound_notes::{AUDIO_OUTPUT, Bend, Length, NOTES_INPUT, Note, NoteEvent, Pitch, Velocity};

pub const SAMPLE_RATE: u32 = 48_000;
/// At 120 bpm, 960 ticks a beat: 25 frames a tick.
pub const FRAMES_PER_TICK: u64 = 25;

/// The smallest owner of an instrument: its notes are in its own record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub notes: Vec<Note>,
    /// Bend wheel moves: tick and value, -8192 to 8191.
    #[serde(default)]
    pub bend: Vec<(u64, i16)>,
}

impl State for Track {
    const TOOL: &'static str = "test.track";
    const OWNS_CHILDREN: bool = true;
}

pub const INSTRUMENT: &str = "instrument";

/// Sends the notes and the bend of one snapshot from the transport tick range, and one `AllOff`
/// when the transport stops or jumps.
#[derive(Default)]
pub struct Sequencer(Arc<Track>);

impl Sequencer {
    pub const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Processor for Sequencer {
    type Update = Arc<Track>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Arc<Track>) {
        std::mem::swap(&mut self.0, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        if transport.jumped || transport.stopped_playing {
            context
                .event_outputs
                .push(Self::NOTES, 0, NoteEvent::AllOff);
        }
        // The bend first, so a note on the same frame starts bent.
        for &(tick, value) in &self.0.bend {
            if let Some(offset) = transport.offset_of(Ticks(tick)) {
                let bend = NoteEvent::Bend(Bend::nearest(i64::from(value)));
                context.event_outputs.push(Self::NOTES, offset, bend);
            }
        }
        for note in &self.0.notes {
            if let Some(offset) = transport.offset_of(note.end()) {
                context.event_outputs.push(Self::NOTES, offset, note.off());
            }
        }
        for note in &self.0.notes {
            if let Some(offset) = transport.offset_of(note.start) {
                context.event_outputs.push(Self::NOTES, offset, note.on());
            }
        }
    }
}

fn apply_track(state: &Track, context: &mut BehaviourContext<'_>) -> Result<(), BehaviourError> {
    let sequencer = context.processor("sequencer", Sequencer::default)?;
    context.update(sequencer, Arc::new(state.clone()))?;
    if let Some(notes) = context.child_input(INSTRUMENT, NOTES_INPUT) {
        context.connect(OutputEndpoint::new(sequencer, Sequencer::NOTES).to(notes))?;
    }
    if let Some(audio) = context.child_output(INSTRUMENT, AUDIO_OUTPUT) {
        // A stereo port takes the device channel it names and the next one.
        context.connect(audio.to_device(0))?;
    }
    Ok(())
}

pub fn registry() -> Registry {
    let mut registry = Registry::new();
    sampler::register(&mut registry).unwrap();
    registry
        .tool::<Track>("test")
        .unwrap()
        .behaviour(apply_track);
    registry
}

pub fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

/// A note, with its start and length in frames at 120 bpm.
pub fn note(start_frame: u64, length_frames: u64, pitch: u8, velocity: u8) -> Note {
    Note {
        start: Ticks(start_frame / FRAMES_PER_TICK),
        length: Length::new(Ticks(length_frames / FRAMES_PER_TICK)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

/// A float WAV of these mono samples at `rate`, as `assets/audio/<name>`.
pub fn write_sample(folder: &std::path::Path, name: &str, rate: u32, samples: &[f32]) {
    let path = folder.join("assets/audio").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for sample in samples {
        writer.write_sample(*sample).unwrap();
    }
    writer.finalize().unwrap();
}

/// `seconds` of a sine of `hz` at `amplitude`, from `phase` in cycles.
pub fn sine(rate: u32, hz: f64, amplitude: f32, seconds: f64, phase: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(rate)) as usize;
    (0..frames)
        .map(|frame| {
            let cycles = phase + hz * frame as f64 / f64::from(rate);
            (TAU * cycles).sin() as f32 * amplitude
        })
        .collect()
}

/// A sampler that plays `name`, with every other field as given.
pub fn playing(name: &str, state: SamplerState) -> SamplerState {
    SamplerState {
        sample: Some(AudioAsset::new(name).unwrap()),
        ..state
    }
}

/// An open project on a temporary folder with an offline stereo engine.
pub struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// Last, so the folder outlives the project that holds its lock.
    pub folder: tempfile::TempDir,
}

impl Harness {
    /// A project whose folder has these samples, each `(name, rate, samples)`.
    pub fn with_samples(samples: &[(&str, u32, Vec<f32>)]) -> Self {
        let folder = tempfile::tempdir().unwrap();
        for (name, rate, samples) in samples {
            write_sample(folder.path(), name, *rate, samples);
        }
        let config = EngineConfig::new(SAMPLE_RATE, 2).rendering_offline();
        let (control, engine) = Engine::new(config);
        let project = Project::open(folder.path(), registry(), control).unwrap();
        Self {
            project,
            engine,
            folder,
        }
    }

    /// One track named `track` with these notes and a sampler as its instrument.
    pub fn add_track(&mut self, notes: Vec<Note>, sampler: SamplerState) {
        let mut changes = Changes::new();
        let track = changes.create(
            id("track"),
            Track {
                notes,
                ..Track::default()
            },
        );
        changes.create(track.id().child(INSTRUMENT).unwrap(), sampler);
        self.project.commit("Add track", changes).unwrap();
    }

    /// A harness with one sample and one track that plays it.
    pub fn playing(sample: (&str, u32, Vec<f32>), notes: Vec<Note>, sampler: SamplerState) -> Self {
        let name = sample.0;
        let mut harness = Self::with_samples(&[sample]);
        harness.add_track(notes, playing(name, sampler));
        assert_eq!(harness.project.problems(), []);
        harness
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.project.root().join(relative)
    }

    /// Writes a file and applies it, as the watcher would.
    pub fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path = self.path(relative);
        std::fs::write(&path, contents).unwrap();
        self.project.apply_outside_changes(&[path]).unwrap()
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(480 * 2) {
            self.engine.process_block(buffer);
        }
        let status = self.project.engine().poll().unwrap();
        assert_eq!(status.port_misuses, 0);
        assert_eq!(status.event_overflows, 0);
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    /// Plays from the start and renders the left channel.
    pub fn play(&mut self, frames: usize) -> Vec<f32> {
        self.project.engine().play();
        let [left, _] = self.render(frames);
        left
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

/// The frequency of a steady sine, from its first and last rising zero crossing, each placed
/// between two frames on a straight line.
pub fn frequency(samples: &[f32]) -> f64 {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(frame, pair)| {
            let (a, b) = (f64::from(pair[0]), f64::from(pair[1]));
            frame as f64 + a / (a - b)
        })
        .collect();
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    (crossings.len() - 1) as f64 * f64::from(SAMPLE_RATE) / (last - first)
}

/// Decibels of a ratio of levels.
pub fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}
