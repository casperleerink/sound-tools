//! Notes a performer plays into a feed, rendered offline into a probe whose level is the sum of
//! the pitches it holds: each lands on its frame, also inside a block, and lets go on time.

#![allow(clippy::unwrap_used)]

use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, EventInput, Node, Ports,
    PrepareConfig, ProcessContext, Processor,
};
use sound_notes::{NoteEvent, NoteFeed, NoteFeedUpdate, PerformedNote, Pitch, Velocity};

struct Probe {
    held: [bool; 128],
}

impl Probe {
    const NOTES: EventInput<NoteEvent> = EventInput::new(0);
    const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for Probe {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::NOTES)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, (): &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let mut events = context.event_inputs.get(Self::NOTES).iter().peekable();
        let [left, right] = context.audio_outputs.get(Self::OUTPUT);
        for (frame, sample) in left.iter_mut().enumerate() {
            while let Some(timed) = events.next_if(|timed| timed.offset <= frame) {
                match timed.event {
                    NoteEvent::On { pitch, .. } => self.held[usize::from(pitch.number())] = true,
                    NoteEvent::Off { pitch } => self.held[usize::from(pitch.number())] = false,
                    NoteEvent::AllOff => self.held = [false; 128],
                    _ => {}
                }
            }
            let pitches = self.held.iter().enumerate().filter(|(_, held)| **held);
            *sample = pitches.map(|(pitch, _)| pitch as f32).sum();
        }
        right.copy_from_slice(left);
    }
}

struct Harness {
    control: EngineControl,
    engine: Engine,
    feed: Node<NoteFeed>,
}

impl Harness {
    fn new() -> Self {
        let (mut control, engine) = Engine::new(EngineConfig::new(48_000, 1));
        let mut edit = control.edit();
        let feed = edit.add_processor("feed", NoteFeed::default()).unwrap();
        let probe = Probe { held: [false; 128] };
        let probe = edit.add_processor("probe", probe).unwrap();
        let notes = Connection::new(feed.id(), NoteFeed::OUTPUT, probe.id(), Probe::NOTES);
        edit.connect(notes).unwrap();
        edit.connect(Connection::to_device(probe.id(), Probe::OUTPUT, 0))
            .unwrap();
        edit.commit().unwrap();
        Self {
            control,
            engine,
            feed,
        }
    }

    fn send(&mut self, update: NoteFeedUpdate) {
        self.control.update(self.feed, update).unwrap();
    }

    /// In device buffers of 100 frames, so a frame lands inside a block as often as on its edge.
    fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut output = vec![0.0; frames];
        for buffer in output.chunks_mut(100) {
            // Sends what did not fit in the ring to the engine yet.
            self.control.poll().unwrap();
            self.engine.process_block(buffer);
        }
        output
    }
}

fn note(pitch: u8, at: Option<u64>, frames: Option<u64>) -> NoteFeedUpdate {
    NoteFeedUpdate::Note(PerformedNote {
        pitch: Pitch::new(pitch).unwrap(),
        velocity: Velocity::new(100).unwrap(),
        at,
        frames,
    })
}

fn release(pitch: u8, at: Option<u64>) -> NoteFeedUpdate {
    let pitch = Pitch::new(pitch).unwrap();
    NoteFeedUpdate::Release { pitch, at }
}

/// The frames where the level changes, with the new level.
fn level_changes(samples: &[f32]) -> Vec<(usize, f32)> {
    let mut last = 0.0;
    let changed = samples.iter().enumerate().filter(|(_, sample)| {
        let changed = **sample != last;
        last = **sample;
        changed
    });
    changed.map(|(frame, sample)| (frame, *sample)).collect()
}

#[test]
fn a_performed_note_lands_on_its_frame_and_ends_after_its_length() {
    let mut harness = Harness::new();
    harness.send(note(60, Some(1_234), Some(500)));
    harness.send(note(62, Some(1_500), None));
    harness.send(release(62, Some(2_345)));
    let changes = level_changes(&harness.render(4_000));
    let expected = [(1_234, 60.0), (1_500, 122.0), (1_734, 62.0), (2_345, 0.0)];
    assert_eq!(changes, expected);
}

#[test]
fn a_note_whose_frame_has_passed_plays_in_the_next_block() {
    let mut harness = Harness::new();
    harness.render(1_000);
    harness.send(note(60, Some(10), Some(50)));
    assert_eq!(level_changes(&harness.render(200)), [(0, 60.0), (50, 0.0)]);
}

#[test]
fn a_release_when_the_feed_is_full_lets_go_of_its_key_at_once() {
    let mut harness = Harness::new();
    harness.send(note(60, None, None));
    assert_eq!(harness.render(100).last(), Some(&60.0));
    // Room for 256 waiting: 128 notes far ahead, an on and an off each.
    for pitch in 0..128 {
        harness.send(note(pitch, Some(1_000_000), Some(1)));
    }
    harness.send(release(60, None));
    // The engine takes a few updates a block, so the release lands a few blocks later.
    assert_eq!(harness.render(4_800).last(), Some(&0.0));
}
