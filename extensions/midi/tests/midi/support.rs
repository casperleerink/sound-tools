//! An engine with the MIDI input and a listening instrument, and nothing else: no project, no
//! device and no MIDI hardware. Every message is one a test sends itself.

use midi::{Input, Keyboard, Played};
use sound_core::{
    Engine, EngineConfig, EngineControl, EventInput, InputEndpoint, Node, Ports, PrepareConfig,
    ProcessContext, Processor, Ticks,
};
use sound_notes::{NoteEvent, Pedal, Pitch, Velocity};

pub const SAMPLE_RATE: u32 = 48_000;
/// Frames per tick at 120 bpm and 48 kHz.
pub const TICK: usize = 25;

/// An event that reached the instrument, with the engine frame it landed on.
pub type Heard = (u64, NoteEvent);

/// An instrument that makes no sound and only says what it was sent and when. It reports
/// through a ring, like everything else that leaves the audio thread here.
pub struct Ears(rtrb::Producer<Heard>);

impl Ears {
    pub const NOTES: EventInput<NoteEvent> = EventInput::new(0);
}

impl Processor for Ears {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_input(Self::NOTES)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        for timed in context.event_inputs.get(Self::NOTES) {
            let frame = context.start_frame + timed.offset as u64;
            if self.0.push((frame, timed.event)).is_err() {
                return;
            }
        }
    }
}

/// The engine with a keyboard playing into one instrument.
pub struct Harness {
    pub control: EngineControl,
    pub engine: Engine,
    pub keyboard: Keyboard,
    pub input: Input,
    ears: Node<Ears>,
    heard: rtrb::Consumer<Heard>,
}

impl Harness {
    pub fn new() -> Self {
        let (mut control, engine) = Engine::new(EngineConfig::new(SAMPLE_RATE, 2));
        let keyboard = Keyboard::attach(&mut control).unwrap();
        let (producer, heard) = rtrb::RingBuffer::new(4096);
        let mut edit = control.edit();
        let ears = edit.add_processor("ears", Ears(producer)).unwrap();
        edit.commit().unwrap();
        let input = keyboard.input();
        let mut harness = Self {
            control,
            engine,
            keyboard,
            input,
            ears,
            heard,
        };
        let notes = harness.notes_input();
        harness
            .keyboard
            .play_into(&mut harness.control, Some(notes))
            .unwrap();
        harness
    }

    /// The `notes` input of the instrument.
    pub fn notes_input(&self) -> InputEndpoint {
        InputEndpoint::new(self.ears, Ears::NOTES)
    }

    /// Runs the engine for `frames` frames in device buffers of `block` frames, and polls the
    /// keyboard after each of them, as the interface does.
    pub fn run(&mut self, frames: usize, block: usize) {
        let mut buffer = vec![0.0_f32; block * 2];
        let mut left = frames;
        while left > 0 {
            let now = left.min(block);
            self.engine.process_block(&mut buffer[..now * 2]);
            self.control.poll().unwrap();
            self.keyboard.poll(None);
            left -= now;
        }
    }

    /// Everything the instrument heard since the last call.
    pub fn heard(&mut self) -> Vec<Heard> {
        let mut heard = Vec::new();
        while let Ok(event) = self.heard.pop() {
            heard.push(event);
        }
        heard
    }

    /// Where the project position is, as the interface reads it. Recording starts here.
    pub fn playhead(&mut self) -> Ticks {
        self.control.poll().unwrap().playhead_tick
    }
}

pub fn pitch(number: u8) -> Pitch {
    Pitch::new(number).unwrap()
}

pub fn on(number: u8, velocity: u8) -> Played {
    Played::On {
        pitch: pitch(number),
        velocity: Velocity::new(velocity).unwrap(),
    }
}

pub fn off(number: u8) -> Played {
    Played::Off {
        pitch: pitch(number),
        velocity: 64,
    }
}

pub fn pedal(value: u8) -> Played {
    Played::Pedal(Pedal::new(value).unwrap())
}
