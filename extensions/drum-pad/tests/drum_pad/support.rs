//! A test-only track tool, a harness on a temporary project folder and measurements.

use std::path::Path;
use std::sync::Arc;

use drum_pad::{DrumPadState, FIRST_NOTE};
use serde::{Deserialize, Serialize};
use sound_core::{
    BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, EventOutput, InstanceId,
    OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, Project, Registry, State,
    Ticks,
};
use sound_notes::{AUDIO_OUTPUT, Length, NOTES_INPUT, Note, NoteEvent, Pitch, Velocity};

pub const SAMPLE_RATE: u32 = 48_000;

/// Ticks per quarter note.
pub const QUARTER: u64 = 960;

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

/// Sends the notes of one snapshot from the transport tick range, and one `AllOff` when the
/// transport stops or jumps, as the sequencer of a track does.
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
        std::mem::swap(&mut self.0, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        if transport.jumped || transport.stopped_playing {
            context
                .event_outputs
                .push(Self::NOTES, 0, NoteEvent::AllOff);
        }
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
        context.connect(audio.to_device(0))?;
    }
    Ok(())
}

pub fn registry() -> Registry {
    let mut registry = Registry::new();
    drum_pad::register(&mut registry).unwrap();
    registry
        .tool::<Track>("test")
        .unwrap()
        .behaviour(apply_track);
    registry
}

pub fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

/// A hit of the pad with this note, a sixteenth long.
pub fn hit(start: u64, note: u8, velocity: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(QUARTER / 4)).unwrap(),
        pitch: Pitch::new(note).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

/// The note of pad `pad`, 0 to 15.
pub fn note(pad: usize) -> u8 {
    FIRST_NOTE + pad as u8
}

/// Left and right of an interleaved stereo render.
pub struct Stereo {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl Stereo {
    fn new(interleaved: &[f32]) -> Self {
        Self {
            left: interleaved.iter().step_by(2).copied().collect(),
            right: interleaved.iter().skip(1).step_by(2).copied().collect(),
        }
    }

    /// Left and right added, for measures of the sound of a pad in the middle.
    pub fn sum(&self) -> Vec<f32> {
        self.left
            .iter()
            .zip(&self.right)
            .map(|(l, r)| l + r)
            .collect()
    }

    /// Writes a 32-bit float WAV, for a person to listen to.
    pub fn write_wav(&self, path: &Path) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for (left, right) in self.left.iter().zip(&self.right) {
            writer.write_sample(*left).unwrap();
            writer.write_sample(*right).unwrap();
        }
        writer.finalize().unwrap();
    }
}

/// An open project on a temporary folder with an offline stereo engine.
pub struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// Last, so the folder outlives the project that holds its lock.
    _folder: tempfile::TempDir,
}

impl Harness {
    pub fn new() -> Self {
        Self::at_rate(SAMPLE_RATE)
    }

    pub fn at_rate(rate: u32) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let (control, engine) = Engine::new(EngineConfig::new(rate, 2));
        let project = Project::open(folder.path(), registry(), control).unwrap();
        Self {
            project,
            engine,
            _folder: folder,
        }
    }

    /// One track named `track` with these notes and a Drum pad as its instrument.
    pub fn with_track(notes: Vec<Note>, drums: DrumPadState) -> Self {
        let mut harness = Self::new();
        harness.add_track("track", notes, drums);
        harness
    }

    pub fn add_track(&mut self, name: &str, notes: Vec<Note>, drums: DrumPadState) {
        let mut changes = Changes::new();
        let track = changes.create(id(name), Track { notes });
        changes.create(track.id().child(INSTRUMENT).unwrap(), drums);
        self.project.commit("Add track", changes).unwrap();
    }

    /// Waits for the sounds that were asked for and puts them in their kits, as an offline
    /// render of the runtime does before each block.
    pub fn take_sounds(&mut self) {
        drum_pad::wait_for_sounds();
        self.take_ready_sounds();
    }

    /// Puts the sounds that are made in their kits, without waiting.
    pub fn take_ready_sounds(&mut self) {
        for (instance, _sounds) in drum_pad::take_ready(self.project.assets()) {
            self.project.rebind(&instance).unwrap();
        }
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render,
    /// after every sound that was asked for is in its kit.
    pub fn render(&mut self, frames: usize) -> Stereo {
        self.take_sounds();
        self.render_as_it_is(frames)
    }

    /// The same, with the kits as they are now.
    pub fn render_as_it_is(&mut self, frames: usize) -> Stereo {
        let mut output = vec![0.0; frames * 2];
        for buffer in output.chunks_mut(960) {
            self.engine.process_block(buffer);
        }
        let status = self.project.engine().poll().unwrap();
        assert_eq!(status.port_misuses, 0);
        assert_eq!(status.event_overflows, 0);
        Stereo::new(&output)
    }

    /// Plays from the start and renders.
    pub fn play(&mut self, frames: usize) -> Stereo {
        self.project.engine().play();
        self.render(frames)
    }
}

/// One hit of pad `pad` at `velocity` in a new project, the pad as `drums` has it, `seconds`
/// long.
pub fn one_hit(pad: usize, velocity: u8, drums: DrumPadState, seconds: f64) -> Stereo {
    let mut harness = Harness::with_track(vec![hit(0, note(pad), velocity)], drums);
    harness.play((seconds * f64::from(SAMPLE_RATE)) as usize)
}

pub fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

pub fn decibels(amplitude: f32) -> f32 {
    20.0 * amplitude.log10()
}

pub fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .fold(0.0, |step, pair| step.max((pair[1] - pair[0]).abs()))
}

/// The last frame whose sample is at least `db` (below 0) relative to the loudest.
pub fn last_above(samples: &[f32], db: f32) -> usize {
    let floor = peak(samples) * 10.0_f32.powf(db / 20.0);
    samples
        .iter()
        .rposition(|sample| sample.abs() >= floor)
        .unwrap_or(0)
}

/// The power of the signal in bands of frequency, as a part of the whole: a fingerprint of a
/// sound. The bands are split at `edges_hz`. A plain discrete Fourier transform at the centre
/// of each bin of 10 Hz up to 20 kHz, over the first `frames` frames.
pub fn spectrum(samples: &[f32], edges_hz: &[f32], frames: usize) -> Vec<f32> {
    let samples = &samples[..frames.min(samples.len())];
    let rate = SAMPLE_RATE as f64;
    let mut bands = vec![0.0_f64; edges_hz.len() + 1];
    let step_hz = 20.0;
    let mut hz = step_hz;
    // A Goertzel filter per bin: a few multiplies per sample, no allocation.
    while hz < 20_000.0 {
        let coefficient = 2.0 * (std::f64::consts::TAU * hz / rate).cos();
        let (mut previous, mut before) = (0.0_f64, 0.0_f64);
        for sample in samples {
            let next = f64::from(*sample) + coefficient * previous - before;
            before = previous;
            previous = next;
        }
        let power = previous * previous + before * before - coefficient * previous * before;
        let band = edges_hz.iter().filter(|edge| hz as f32 >= **edge).count();
        bands[band] += power;
        hz += step_hz;
    }
    let total: f64 = bands.iter().sum();
    bands.iter().map(|power| (power / total) as f32).collect()
}

/// Where the power of a sound is, the centre of mass of its spectrum in Hz.
pub fn centroid(samples: &[f32], frames: usize) -> f32 {
    let samples = &samples[..frames.min(samples.len())];
    let rate = SAMPLE_RATE as f64;
    let (mut weighted, mut total) = (0.0_f64, 0.0_f64);
    let mut hz = 20.0;
    while hz < 20_000.0 {
        let coefficient = 2.0 * (std::f64::consts::TAU * hz / rate).cos();
        let (mut previous, mut before) = (0.0_f64, 0.0_f64);
        for sample in samples {
            let next = f64::from(*sample) + coefficient * previous - before;
            before = previous;
            previous = next;
        }
        let power = previous * previous + before * before - coefficient * previous * before;
        weighted += power * hz;
        total += power;
        hz *= 1.02;
    }
    (weighted / total) as f32
}

/// The frames at which the signal crosses zero upward.
pub fn rising_zero_crossings(samples: &[f32]) -> Vec<usize> {
    let pairs = samples.windows(2).enumerate();
    pairs
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(frame, _)| frame + 1)
        .collect()
}

/// Writes a WAV of a sine at `hz` for `seconds` at `rate`, 16-bit mono, into `path`.
pub fn write_sine(path: &Path, hz: f64, seconds: f64, rate: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    let frames = (seconds * f64::from(rate)) as usize;
    for frame in 0..frames {
        let t = frame as f64 / f64::from(rate);
        let sample = 0.5 * (std::f64::consts::TAU * hz * t).sin();
        writer.write_sample((sample * 32_767.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
}
