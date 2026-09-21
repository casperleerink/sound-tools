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
    /// Other instruments of the same engine, see [`Harness::add_ears`].
    others: Vec<(InputEndpoint, rtrb::Consumer<Heard>)>,
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
            others: Vec::new(),
        };
        let notes = harness.notes_input();
        harness.wire(Some(notes));
        harness
    }

    /// The `notes` input of the instrument.
    pub fn notes_input(&self) -> InputEndpoint {
        InputEndpoint::new(self.ears, Ears::NOTES)
    }

    /// A second instrument in the same engine, as another track's synth is. Gives its `notes`
    /// port, and [`Self::heard_by`] says what reached it.
    pub fn add_ears(&mut self) -> InputEndpoint {
        let (producer, heard) = rtrb::RingBuffer::new(4096);
        let mut edit = self.control.edit();
        let name = format!("ears-{}", self.others.len() + 2);
        let ears = edit.add_processor(&name, Ears(producer)).unwrap();
        edit.commit().unwrap();
        let port = InputEndpoint::new(ears, Ears::NOTES);
        self.others.push((port, heard));
        port
    }

    /// Everything a second instrument heard since the last call.
    pub fn heard_by(&mut self, port: InputEndpoint) -> Vec<Heard> {
        let found = self.others.iter_mut().find(|(other, _)| *other == port);
        let (_, ring) = found.expect("no such instrument");
        let mut heard = Vec::new();
        while let Ok(event) = ring.pop() {
            heard.push(event);
        }
        heard
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
            self.keyboard.poll(&mut self.control, None).unwrap();
            left -= now;
        }
    }

    /// One poll of the control side, as the interface does it.
    pub fn poll(&mut self) {
        self.keyboard.poll(&mut self.control, None).unwrap();
    }

    /// Sends the live input to a port and runs until the change has happened, as the window
    /// does over its polls: the release of what was held goes out into the port it played
    /// into, a block before the connection changes.
    pub fn wire(&mut self, destination: Option<InputEndpoint>) {
        self.keyboard
            .play_into(&mut self.control, destination)
            .unwrap();
        for _ in 0..4 {
            self.poll();
            if self.keyboard.destination() == destination {
                return;
            }
            self.run(64, 64);
        }
        panic!("the live input never reached {destination:?}");
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
