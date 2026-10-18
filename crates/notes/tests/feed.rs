//! Notes a performer plays into a feed, rendered offline into a probe whose level is the sum of
//! the pitches it holds, 1000 while the pedal is down and the value of controller 74: each lands
//! on its frame, also inside a block, and lets go on time, also when its schedule or the output
//! is full.

#![allow(clippy::unwrap_used)]

use sound_core::{
    AudioOutput, Connection, Engine, EngineConfig, EngineControl, EventInput, EventOutput, Node,
    Ports, PrepareConfig, ProcessContext, Processor,
};
use sound_notes::{
    Amount, Controller, NoteEvent, NoteFeed, NoteFeedUpdate, Pedal, Performed, Pitch, Velocity,
};

struct Probe {
    held: [bool; 128],
    pedal: bool,
    control: f32,
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
                    NoteEvent::Pedal(pedal) => self.pedal = pedal.is_down(),
                    NoteEvent::Control { controller, value } if controller.number() == 74 => {
                        self.control = f32::from(value.value());
                    }
                    _ => {}
                }
            }
            let pitches = self.held.iter().enumerate().filter(|(_, held)| **held);
            let pedal = if self.pedal { 1_000.0 } else { 0.0 };
            *sample = pitches.map(|(pitch, _)| pitch as f32).sum::<f32>() + pedal + self.control;
        }
        right.copy_from_slice(left);
    }
}

/// Another sender into the feed, such as a player.
#[derive(Default)]
struct Sender {
    /// How many more blocks it fills its output with offs of pitch 0, which the probe does
    /// not count.
    flood: u32,
    /// What it sends in its next block, at that offset.
    once: Option<(usize, NoteEvent)>,
}

enum Send {
    Flood(u32),
    Once(usize, NoteEvent),
}

impl Sender {
    const OUTPUT: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Processor for Sender {
    type Update = Send;

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, send: &mut Send) {
        match send {
            Send::Flood(blocks) => self.flood = *blocks,
            Send::Once(offset, event) => self.once = Some((*offset, *event)),
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        if let Some((offset, event)) = self.once.take() {
            context.event_outputs.push(Self::OUTPUT, offset, event);
        }
        if self.flood > 0 {
            self.flood -= 1;
            let pitch = Pitch::new(0).unwrap();
            while context
                .event_outputs
                .push(Self::OUTPUT, 0, NoteEvent::Off { pitch })
            {}
        }
    }
}

struct Harness {
    control: EngineControl,
    engine: Engine,
    feed: Node<NoteFeed>,
    sender: Node<Sender>,
}

impl Harness {
    fn new() -> Self {
        let (mut control, engine) = Engine::new(EngineConfig::new(48_000, 1));
        let mut edit = control.edit();
        let feed = edit.add_processor("feed", NoteFeed::default()).unwrap();
        let sender = edit.add_processor("sender", Sender::default()).unwrap();
        let flooded = Connection::new(sender.id(), Sender::OUTPUT, feed.id(), NoteFeed::INPUT);
        edit.connect(flooded).unwrap();
        let probe = Probe {
            held: [false; 128],
            pedal: false,
            control: 0.0,
        };
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
            sender,
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

/// A note of `frames`, or held until its off with none.
fn note(pitch: u8, at: u64, frames: Option<u64>) -> NoteFeedUpdate {
    let (pitch, velocity) = (Pitch::new(pitch).unwrap(), Velocity::new(100).unwrap());
    NoteFeedUpdate::Note(match frames {
        Some(frames) => Performed::Play {
            pitch,
            velocity,
            at,
            frames,
        },
        None => Performed::Send {
            event: NoteEvent::On { pitch, velocity },
            at,
        },
    })
}

fn send(event: NoteEvent, at: u64) -> NoteFeedUpdate {
    NoteFeedUpdate::Note(Performed::Send { event, at })
}

fn release(pitch: u8, at: u64) -> NoteFeedUpdate {
    let pitch = Pitch::new(pitch).unwrap();
    send(NoteEvent::Off { pitch }, at)
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
    harness.send(note(60, 1_234, Some(500)));
    harness.send(note(62, 1_500, None));
    harness.send(release(62, 2_345));
    let changes = level_changes(&harness.render(4_000));
    let expected = [(1_234, 60.0), (1_500, 122.0), (1_734, 62.0), (2_345, 0.0)];
    assert_eq!(changes, expected);
}

#[test]
fn a_note_whose_frame_has_passed_plays_in_the_next_block() {
    let mut harness = Harness::new();
    harness.render(1_000);
    harness.send(note(60, 10, Some(50)));
    assert_eq!(level_changes(&harness.render(200)), [(0, 60.0), (50, 0.0)]);
}

#[test]
fn a_release_when_the_feed_is_full_lets_go_of_its_key_at_once() {
    let mut harness = Harness::new();
    harness.send(note(60, 0, None));
    assert_eq!(harness.render(100).last(), Some(&60.0));
    // Room for 256 waiting: 128 notes far ahead, an on and an off each.
    for pitch in 0..128 {
        harness.send(note(pitch, 1_000_000, Some(1)));
    }
    harness.send(release(60, 0));
    // The engine takes a few updates a block, so the release lands a few blocks later.
    assert_eq!(harness.render(4_800).last(), Some(&0.0));
}

#[test]
fn a_pedal_and_a_controller_that_find_the_feed_full_play_at_once_and_a_note_on_does_not() {
    let mut harness = Harness::new();
    let control = |value| NoteEvent::Control {
        controller: Controller::new(74).unwrap(),
        value: Amount::new(value).unwrap(),
    };
    harness.send(send(NoteEvent::Pedal(Pedal::new(127).unwrap()), 0));
    harness.send(send(control(10), 0));
    assert_eq!(harness.render(100).last(), Some(&1_010.0));
    for pitch in 0..128 {
        harness.send(note(pitch, 1_000_000, Some(1)));
    }
    // Where each controller ends up counts: the 20 never sounds, the 30 does.
    for event in [NoteEvent::Pedal(Pedal::UP), control(20), control(30)] {
        harness.send(send(event, 500_000));
    }
    harness.send(note(70, 0, None));
    assert_eq!(harness.render(4_800).last(), Some(&30.0));
}

#[test]
fn an_end_that_finds_the_output_full_comes_in_the_next_block_that_has_room() {
    let mut harness = Harness::new();
    harness.send(note(60, 0, Some(150)));
    assert_eq!(harness.render(100).last(), Some(&60.0));
    harness
        .control
        .update(harness.sender, Send::Flood(3))
        .unwrap();
    assert_eq!(harness.render(1_000).last(), Some(&0.0));
}

#[test]
fn a_performed_end_on_the_frame_another_sender_starts_the_key_comes_first() {
    let mut harness = Harness::new();
    harness.render(800);
    harness.send(note(60, 900, Some(110)));
    assert_eq!(harness.render(200).last(), Some(&60.0));
    let pitch = Pitch::new(60).unwrap();
    let velocity = Velocity::new(100).unwrap();
    let on = Send::Once(10, NoteEvent::On { pitch, velocity });
    harness.control.update(harness.sender, on).unwrap();
    // The end of the performed note at frame 1010, then the other's start: it holds.
    assert_eq!(harness.render(100).last(), Some(&60.0));
}
