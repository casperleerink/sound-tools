//! A source and notes into a pitch correction into the device, and what the tests measure
//! with.

use std::f64::consts::TAU;

use pitch_correction::{Meters, PitchCorrection, PitchCorrectionState};
use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, EventOutput, Node, Ports,
    PrepareConfig, ProcessContext, Processor, Ticks,
};
use sound_media::analysis::{Pitch as Reading, PitchSettings, PitchTracker};
use sound_notes::{NoteEvent, Pitch, Velocity};

pub(crate) const SAMPLE_RATE: u32 = 48_000;
const RATE: f64 = SAMPLE_RATE as f64;

/// The left and right of one frame, from the frame of the song or of the engine it plays at.
pub(crate) type Signal = Box<dyn FnMut(u64) -> [f32; 2] + Send>;

/// Plays a signal: at each frame of the engine, or, while the song plays, at each frame of the
/// song, as a region does, and silence while it does not.
pub(crate) struct Source {
    signal: Signal,
    follows_song: bool,
}

impl Source {
    pub(crate) const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Source {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let frames = context.frames;
        let song = context.transport.frame_range.clone();
        let start = context.start_frame;
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for (offset, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let frame = match self.follows_song {
                // A short range after a play or a seek ends with the block.
                true => (song.end.0 + offset as u64)
                    .checked_sub(frames as u64)
                    .filter(|frame| *frame >= song.start.0),
                false => Some(start + offset as u64),
            };
            [*left, *right] = frame.map_or([0.0; 2], &mut self.signal);
        }
    }
}

/// Sends notes at frames of the engine.
struct Notes {
    events: Vec<(u64, NoteEvent)>,
}

impl Notes {
    const OUTPUT: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Processor for Notes {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let block = context.start_frame..context.start_frame + context.frames as u64;
        for (frame, event) in &self.events {
            if block.contains(frame) {
                let offset = (frame - block.start) as usize;
                context.event_outputs.push(Self::OUTPUT, offset, *event);
            }
        }
    }
}

/// A note on at `from` seconds and off at `to`.
pub(crate) fn held(pitch: u8, from: f64, to: f64) -> [(u64, NoteEvent); 2] {
    let pitch = Pitch::new(pitch).unwrap();
    let velocity = Velocity::new(100).unwrap();
    [
        (seconds(from), NoteEvent::On { pitch, velocity }),
        (seconds(to), NoteEvent::Off { pitch }),
    ]
}

pub(crate) fn seconds(seconds: f64) -> u64 {
    (seconds * RATE) as u64
}

/// A source, notes and a pitch correction into the device.
pub(crate) struct Rig {
    pub control: EngineControl,
    pub engine: Engine,
    pub correction: Node<PitchCorrection>,
    pub meters: Meters,
}

impl Rig {
    pub(crate) fn new(state: PitchCorrectionState, signal: Signal) -> Self {
        Self::with(state, signal, false, Vec::new(), true)
    }

    pub(crate) fn with(
        state: PitchCorrectionState,
        signal: Signal,
        follows_song: bool,
        notes: Vec<(u64, NoteEvent)>,
        offline: bool,
    ) -> Self {
        let config = EngineConfig::new(SAMPLE_RATE, 2);
        let config = if offline {
            config.rendering_offline()
        } else {
            config
        };
        let (mut control, engine) = Engine::new(config);
        let meters = Meters::default();
        let mut edit = control.edit();
        let source = Source {
            signal,
            follows_song,
        };
        let source = edit.add_processor("source", source).unwrap();
        let notes = edit.add_processor("notes", Notes { events: notes }).unwrap();
        let correction = PitchCorrection::new(state, meters.clone());
        let correction = edit.add_processor("correction", correction).unwrap();
        let connections = [
            Connection::new(
                source.id(),
                Source::OUTPUT,
                correction.id(),
                PitchCorrection::INPUT,
            ),
            Connection::new(
                notes.id(),
                Notes::OUTPUT,
                correction.id(),
                PitchCorrection::NOTES,
            ),
            Connection::to_device(correction.id(), PitchCorrection::OUTPUT, 0),
        ];
        for connection in connections {
            edit.connect(connection).unwrap();
        }
        edit.commit().unwrap();
        Self {
            control,
            engine,
            correction,
            meters,
        }
    }

    /// The left channel, in device buffers of `buffer` frames.
    pub(crate) fn render_in(&mut self, frames: usize, buffer: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames * 2];
        for chunk in output.chunks_mut(buffer * 2) {
            self.engine.process_block(chunk);
        }
        output.iter().step_by(2).copied().collect()
    }

    pub(crate) fn render(&mut self, frames: usize) -> Vec<f32> {
        self.render_in(frames, 480)
    }

    pub(crate) fn play_looped(&mut self, from: Ticks, to: Ticks) {
        self.control.set_loop(Some(from..to));
        self.control.play();
    }
}

/// A bright tone in both channels whose note, a MIDI note number with its fraction, goes with
/// the time in seconds, and that is silent while `note` gives `None`.
pub(crate) fn voice(note: impl Fn(f64) -> Option<f64> + Send + 'static) -> Signal {
    let mut phase = 0.0;
    Box::new(move |frame| {
        let Some(note) = note(frame as f64 / RATE) else {
            phase = 0.0;
            return [0.0; 2];
        };
        phase += TAU * hz(note) / RATE;
        let harmonics = [0.3, 0.15, 0.1, 0.06, 0.04];
        let sample: f64 = (harmonics.iter().enumerate())
            .map(|(index, amplitude)| amplitude * (phase * (index + 1) as f64).sin())
            .sum();
        [sample as f32; 2]
    })
}

/// Sines of these notes together, as a chord.
pub(crate) fn chord(notes: &'static [f64]) -> Signal {
    Box::new(move |frame| {
        let time = frame as f64 / RATE;
        let sample: f64 = notes.iter().map(|note| 0.15 * (TAU * hz(*note) * time).sin()).sum();
        [sample as f32; 2]
    })
}

/// White noise, the same every run.
pub(crate) fn noise() -> Signal {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    Box::new(move |_| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let sample = ((state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * 0.25;
        [sample; 2]
    })
}

pub(crate) fn hz(note: f64) -> f64 {
    440.0 * 2_f64.powf((note - 69.0) / 12.0)
}

/// What a tracker hears in `output`: each note read, with the time of the input it came from,
/// `latency` frames before.
pub(crate) fn heard(output: &[f32], latency: u32) -> Vec<(f64, f64)> {
    let settings = PitchSettings {
        lowest: 80.0,
        highest: 1_000.0,
        tolerance: 0.5,
    };
    let mut tracker = PitchTracker::new(RATE, settings);
    let mut notes = Vec::new();
    for (frame, sample) in output.iter().enumerate() {
        tracker.push([*sample]);
        if frame % 48 == 0
            && let Reading::Note(note) = tracker.pitch()
        {
            let time = (frame as f64 + 1.0 - tracker.age() - f64::from(latency)) / RATE;
            notes.push((time, note));
        }
    }
    notes
}

/// The notes of `heard` from `from` to `to` seconds.
pub(crate) fn between(heard: &[(f64, f64)], from: f64, to: f64) -> Vec<f64> {
    let inside = heard.iter().filter(|(time, _)| (from..to).contains(time));
    inside.map(|(_, note)| *note).collect()
}

/// The farthest any of `notes` is from `note`, in cents.
pub(crate) fn farthest_cents(notes: &[f64], note: f64) -> f64 {
    assert!(!notes.is_empty(), "no note heard");
    let far = notes.iter().map(|each| (each - note).abs()).fold(0.0, f64::max);
    100.0 * far
}

/// The frame of the first sample of each run of sound after at least 50 ms of silence.
pub(crate) fn onsets(sound: &[f32]) -> Vec<usize> {
    let mut onsets = Vec::new();
    let mut quiet = usize::MAX;
    for (frame, sample) in sound.iter().enumerate() {
        if sample.abs() > 0.05 {
            if quiet > 2_400 {
                onsets.push(frame);
            }
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    onsets
}
