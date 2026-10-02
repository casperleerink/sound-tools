//! A test-only track tool, a harness on a temporary project folder and what the tests measure
//! with.

use std::path::PathBuf;
use std::sync::Arc;

use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};
use sound_core::{
    BehaviourContext, BehaviourError, Changes, Engine, EngineConfig, EventOutput, InstanceId,
    OutputEndpoint, Ports, PrepareConfig, ProcessContext, Processor, Project, Registry, State,
    Tempo, TempoMap, Ticks, TimeSignature,
};
use sound_notes::{
    AUDIO_OUTPUT, Amount, Bend, Length, NOTES_INPUT, Note, NoteEvent, Pedal, PedalChange, Pitch,
    Velocity,
};
use wavetable::WavetableState;

pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// Ticks per second at the default tempo of 120 bpm.
pub(crate) const TICKS_PER_SECOND: u64 = 1_920;

/// The smallest owner of an instrument: its notes and wheel moves are in its own record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Track {
    pub notes: Vec<Note>,
    /// Sustain pedal moves at their ticks, as a clip holds them.
    pub pedal: Vec<PedalChange>,
    /// Bend wheel moves: tick and value, -8192 to 8191.
    pub bend: Vec<(u64, i16)>,
    /// Mod wheel moves: tick and value, 0 to 127.
    pub mod_wheel: Vec<(u64, u8)>,
    /// Key pressure moves: tick and value, 0 to 127.
    pub pressure: Vec<(u64, u8)>,
}

impl State for Track {
    const TOOL: &'static str = "test.track";
    const OWNS_CHILDREN: bool = true;
}

/// The name of the child a track plays.
pub(crate) const INSTRUMENT: &str = "instrument";

/// What the track sends: its notes, its pedal moves and its wheel moves.
#[derive(Default, PartialEq, Eq)]
pub(crate) struct Part {
    pub notes: Vec<Note>,
    pub pedal: Vec<PedalChange>,
    pub wheels: Vec<(Ticks, NoteEvent)>,
}

/// Sends the notes of one immutable snapshot from the transport tick range, and one `AllOff`
/// when the transport stops or jumps.
#[derive(Default)]
pub(crate) struct Sequencer(Arc<Part>);

impl Sequencer {
    pub(crate) const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Processor for Sequencer {
    type Update = Arc<Part>;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut Arc<Part>) {
        std::mem::swap(&mut self.0, update);
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let transport = &context.transport;
        if transport.jumped || transport.stopped_playing {
            context
                .event_outputs
                .push(Self::NOTES, 0, NoteEvent::AllOff);
        }
        // The pedal and the wheels first, then all offs, then all ons: the order of the note
        // contract.
        for change in &self.0.pedal {
            if let Some(offset) = transport.offset_of(change.start) {
                let event = NoteEvent::Pedal(change.value);
                context.event_outputs.push(Self::NOTES, offset, event);
            }
        }
        for (tick, event) in &self.0.wheels {
            if let Some(offset) = transport.offset_of(*tick) {
                context.event_outputs.push(Self::NOTES, offset, *event);
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
    let moves = |moves: &[(u64, u8)], event: fn(Amount) -> NoteEvent| {
        let moves = moves
            .iter()
            .map(move |&(tick, value)| (Ticks(tick), event(Amount::nearest(i64::from(value)))));
        moves.collect::<Vec<_>>()
    };
    let bends = state.bend.iter().map(|&(tick, value)| {
        let bend = Bend::nearest(i64::from(value));
        (Ticks(tick), NoteEvent::Bend(bend))
    });
    let mut wheels: Vec<_> = bends.collect();
    wheels.extend(moves(&state.mod_wheel, NoteEvent::ModWheel));
    wheels.extend(moves(&state.pressure, NoteEvent::Pressure));
    let part = Part {
        notes: state.notes.clone(),
        pedal: state.pedal.clone(),
        wheels,
    };
    context.update(sequencer, Arc::new(part))?;
    if let Some(notes) = context.child_input(INSTRUMENT, NOTES_INPUT) {
        context.connect(OutputEndpoint::new(sequencer, Sequencer::NOTES).to(notes))?;
    }
    // A stereo port fills the device channel it goes to and the one after it.
    if let Some(audio) = context.child_output(INSTRUMENT, AUDIO_OUTPUT) {
        context.connect(audio.to_device(0))?;
    }
    Ok(())
}

pub(crate) fn registry() -> Registry {
    let mut registry = Registry::new();
    wavetable::register(&mut registry).unwrap();
    registry
        .tool::<Track>("test")
        .unwrap()
        .behaviour(apply_track);
    registry
}

pub(crate) fn id(id: &str) -> InstanceId {
    InstanceId::new(id).unwrap()
}

/// A note at `start` ticks, `length` ticks long.
pub(crate) fn note(start: u64, length: u64, pitch: u8, velocity: u8) -> Note {
    Note {
        start: Ticks(start),
        length: Length::new(Ticks(length)).unwrap(),
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

/// A pedal move at a tick.
pub(crate) fn pedal(start: u64, value: u8) -> PedalChange {
    PedalChange {
        start: Ticks(start),
        value: Pedal::new(value).unwrap(),
    }
}

/// An open project on a temporary folder with an offline stereo engine.
pub(crate) struct Harness {
    pub project: Project,
    pub engine: Engine,
    /// Last, so the folder outlives the project that holds its lock.
    _folder: tempfile::TempDir,
}

impl Harness {
    pub(crate) fn new() -> Self {
        let folder = tempfile::tempdir().unwrap();
        let (control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let project = Project::open(folder.path(), registry(), control).unwrap();
        Self {
            project,
            engine,
            _folder: folder,
        }
    }

    /// One track named `track` that plays `track` on a synth with `synth`.
    pub(crate) fn with(track: Track, synth: WavetableState) -> Self {
        let mut harness = Self::new();
        harness.add("track", track, synth);
        harness
    }

    /// One track with these notes.
    pub(crate) fn with_notes(notes: Vec<Note>, synth: WavetableState) -> Self {
        Self::with(
            Track {
                notes,
                ..Track::default()
            },
            synth,
        )
    }

    pub(crate) fn add(&mut self, name: &str, track: Track, synth: WavetableState) {
        let mut changes = Changes::new();
        let track = changes.create(id(name), track);
        changes.create(track.id().child(INSTRUMENT).unwrap(), synth);
        self.project.commit("Add track", changes).unwrap();
    }

    /// Changes the synth of the track `track`, as a view or an agent would.
    pub(crate) fn edit(&mut self, synth: WavetableState) {
        let instrument = id("track").child(INSTRUMENT).unwrap();
        let instance = self.project.resolve(&instrument).unwrap();
        let mut changes = Changes::new();
        changes.set(&instance, synth);
        self.project.commit("Edit", changes).unwrap();
    }

    pub(crate) fn set_tempo(&mut self, bpm: f64) {
        let mut changes = Changes::new();
        let tempo = Tempo::from_bpm(bpm).unwrap();
        changes.set_tempo_map(TempoMap::constant(TimeSignature::default(), tempo));
        self.project.commit("Tempo", changes).unwrap();
    }

    /// Writes a file and applies it, as the watcher would.
    pub(crate) fn write_and_apply(&mut self, relative: &str, contents: &str) -> usize {
        let path: PathBuf = self.project.root().join(relative);
        std::fs::write(&path, contents).unwrap();
        self.project.apply_outside_changes(&[path]).unwrap()
    }

    /// Renders in device buffers of 480 frames, so short sub-blocks are part of every render.
    /// Left and right.
    pub(crate) fn render(&mut self, frames: usize) -> [Vec<f32>; 2] {
        let mut output = vec![0.0; 2 * frames];
        for buffer in output.chunks_mut(2 * 480) {
            self.engine.process_block(buffer);
        }
        let status = self.project.engine().poll().unwrap();
        assert_eq!(status.port_misuses, 0);
        assert_eq!(status.event_overflows, 0);
        let channel = |channel: usize| output.iter().skip(channel).step_by(2).copied().collect();
        [channel(0), channel(1)]
    }

    /// Plays from the start and renders. Left and right.
    pub(crate) fn play(&mut self, frames: usize) -> [Vec<f32>; 2] {
        self.project.engine().play();
        self.render(frames)
    }

    /// Plays from the start and renders the left channel.
    pub(crate) fn play_left(&mut self, frames: usize) -> Vec<f32> {
        let [left, _] = self.play(frames);
        left
    }
}

/// The frames of `seconds`.
pub(crate) fn frames(seconds: f32) -> usize {
    (seconds * SAMPLE_RATE as f32) as usize
}

/// Ticks of `seconds` at 120 bpm.
pub(crate) fn ticks(seconds: f32) -> u64 {
    (seconds * TICKS_PER_SECOND as f32) as u64
}

pub(crate) fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

pub(crate) fn rms(samples: &[f32]) -> f32 {
    let sum: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// The largest step from one sample to the next.
pub(crate) fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .fold(0.0, |step, pair| step.max((pair[1] - pair[0]).abs()))
}

/// The Blackman-Harris window at `index` of `length`: a partial leaks less than about 90 dB
/// into frequencies far from it.
fn window(index: usize, length: usize) -> f64 {
    let x = std::f64::consts::TAU * index as f64 / length as f64;
    0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
}

/// The amplitude of the part of the signal at one frequency: one bin of a Fourier transform
/// through a window, so the other partials do not leak into it.
pub(crate) fn level_at(samples: &[f32], frequency_hz: f32) -> f32 {
    let step = std::f64::consts::TAU * f64::from(frequency_hz) / f64::from(SAMPLE_RATE);
    let (mut real, mut imaginary, mut weight) = (0.0, 0.0, 0.0);
    for (frame, sample) in samples.iter().enumerate() {
        let angle = step * frame as f64;
        let window = window(frame, samples.len());
        real += f64::from(*sample) * window * angle.cos();
        imaginary += f64::from(*sample) * window * angle.sin();
        weight += window;
    }
    (2.0 * real.hypot(imaginary) / weight) as f32
}

/// The power spectrum of `samples` through the window, one value per bin of
/// `SAMPLE_RATE / samples.len()` Hz, from 0 Hz to half the sample rate.
pub(crate) fn power_spectrum(samples: &[f32]) -> Vec<f64> {
    let length = samples.len();
    let transform = RealFftPlanner::<f64>::new().plan_fft_forward(length);
    let mut input = transform.make_input_vec();
    for (index, (input, sample)) in input.iter_mut().zip(samples).enumerate() {
        let x = std::f64::consts::TAU * index as f64 / length as f64;
        let window =
            0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos();
        *input = f64::from(*sample) * window;
    }
    let mut spectrum = transform.make_output_vec();
    transform.process(&mut input, &mut spectrum).unwrap();
    spectrum.iter().map(|bin| bin.norm_sqr()).collect()
}

/// Up to where the tests look for what folds back. Above it nobody hears it, and the
/// oversampler of the SDK lets some through between 0.45 and 0.55 of the sample rate.
pub(crate) const AUDIBLE_HZ: f32 = 18_000.0;

/// The power of `samples` up to [`AUDIBLE_HZ`] that is not within `width_hz` of a multiple of
/// `fundamental_hz`, against the power that is, in dB: how far under the harmonics everything
/// else is, such as what folds back from above half the sample rate.
pub(crate) fn inharmonic_db(samples: &[f32], fundamental_hz: f32, width_hz: f32) -> f64 {
    let spectrum = power_spectrum(samples);
    let bin_hz = SAMPLE_RATE as f32 / samples.len() as f32;
    let (mut harmonic, mut other) = (0.0, 0.0);
    // Leave out the lowest bins: the start of the note and the window are there.
    for (bin, power) in spectrum.iter().enumerate().skip(4) {
        let hz = bin as f32 * bin_hz;
        if hz > AUDIBLE_HZ {
            break;
        }
        let nearest = (hz / fundamental_hz).round() * fundamental_hz;
        if nearest > 0.0 && (hz - nearest).abs() <= width_hz {
            harmonic += power;
        } else {
            other += power;
        }
    }
    10.0 * (other / harmonic).log10()
}

/// The power above `hz` against all of it, in dB: how bright a sound is.
pub(crate) fn brightness_db(samples: &[f32], hz: f32) -> f64 {
    let spectrum = power_spectrum(samples);
    let bin_hz = SAMPLE_RATE as f32 / samples.len() as f32;
    let total: f64 = spectrum.iter().sum();
    let above: f64 = spectrum
        .iter()
        .enumerate()
        .filter(|(bin, _)| *bin as f32 * bin_hz > hz)
        .map(|(_, power)| power)
        .sum();
    10.0 * (above / total).log10()
}

/// The frequency of the loudest bin between `low_hz` and `high_hz`, sharpened between the
/// bins around it.
pub(crate) fn loudest_hz(samples: &[f32], low_hz: f32, high_hz: f32) -> f32 {
    let spectrum = power_spectrum(samples);
    let bin_hz = SAMPLE_RATE as f32 / samples.len() as f32;
    let low = (low_hz / bin_hz) as usize;
    let high = ((high_hz / bin_hz) as usize).min(spectrum.len() - 2);
    let loudest = (low.max(1)..=high)
        .max_by(|a, b| spectrum[*a].total_cmp(&spectrum[*b]))
        .unwrap();
    // The peak of a parabola through the log power of the three bins around it.
    let [before, at, after] = [loudest - 1, loudest, loudest + 1].map(|bin| spectrum[bin].ln());
    let offset = 0.5 * (before - after) / (before - 2.0 * at + after);
    (loudest as f64 + offset) as f32 * bin_hz
}
