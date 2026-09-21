//! The path from a MIDI message to sound, and the report that comes back.
//!
//! Three threads meet here and none of them waits for another:
//!
//! - A device thread reads a message and puts it in the input ring ([`Input`]). It is the only
//!   place with a lock, and it is never the audio thread: several ports share one ring, and the
//!   lock is only ever taken by other device threads.
//! - The audio thread takes what is in the ring at the start of a block and sends it to the
//!   instrument at offset 0 of that block. So a message sounds at the start of the next block
//!   and never goes through the interface or its poll.
//! - It puts what it sent in the report ring, with where the project was. The control side
//!   reads that for the take and for the latency, and nothing it does can delay sound.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sound_core::{
    EventOutput, Ports, PrepareConfig, ProcessContext, Processor, Ticks, monotonic_nanos,
};
use sound_notes::{NoteEvent, Pedal, Pitch, Velocity};

/// Messages the input ring holds. A keyboard sends a few hundred a second at most, and the
/// audio thread empties the ring every block, so this is far more than a burst needs.
pub const INPUT_CAPACITY: usize = 1024;

/// Reports the audio thread sends back. The control side reads them every poll.
pub const REPORT_CAPACITY: usize = 4096;

/// One message from a keyboard, as far as this application cares.
///
/// Everything else is left out on purpose: no pitch bend, no mod wheel, no aftertouch, no
/// other controller, no channel. All inputs and all channels are merged.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Played {
    On {
        pitch: Pitch,
        velocity: Velocity,
    },
    /// The release velocity is kept for the raw take. The note contract has no room for it,
    /// and no instrument here uses it yet.
    Off {
        pitch: Pitch,
        velocity: u8,
    },
    Pedal(Pedal),
}

impl Played {
    /// Reads one MIDI message. `None` for anything this application does not use, and for
    /// bytes that are not a message.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        use wmidi::{ControlFunction, MidiMessage};
        match MidiMessage::try_from(bytes).ok()? {
            // A note on of velocity 0 is a note off, which every keyboard may send.
            MidiMessage::NoteOn(_, note, velocity) if u8::from(velocity) == 0 => Some(Self::Off {
                pitch: Pitch::new(u8::from(note)).ok()?,
                velocity: 0,
            }),
            MidiMessage::NoteOn(_, note, velocity) => Some(Self::On {
                pitch: Pitch::new(u8::from(note)).ok()?,
                velocity: Velocity::new(u8::from(velocity)).ok()?,
            }),
            MidiMessage::NoteOff(_, note, velocity) => Some(Self::Off {
                pitch: Pitch::new(u8::from(note)).ok()?,
                velocity: u8::from(velocity),
            }),
            MidiMessage::ControlChange(_, ControlFunction::DAMPER_PEDAL, value) => {
                Some(Self::Pedal(Pedal::new(u8::from(value)).ok()?))
            }
            _ => None,
        }
    }

    /// What goes to the instrument.
    pub fn event(self) -> NoteEvent {
        match self {
            Self::On { pitch, velocity } => NoteEvent::On { pitch, velocity },
            Self::Off { pitch, .. } => NoteEvent::Off { pitch },
            Self::Pedal(value) => NoteEvent::Pedal(value),
        }
    }
}

/// A message with the moment it reached this process, on the clock of
/// [`sound_core::monotonic_nanos`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Arrived {
    pub at_nanos: u64,
    pub played: Played,
}

/// What the audio thread sent to the instrument, and where the project was when it did.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Sounded {
    pub arrived: Arrived,
    /// The engine frame this block began on. The sound of the message starts there.
    pub frame: u64,
    /// The first project tick of that block: where a recording writes the message.
    pub tick: Ticks,
    pub playing: bool,
}

struct Shared {
    /// The lock is taken by device threads only. Several MIDI ports share one ring, and one
    /// ring with a lock on the writing side is simpler than one ring per port with a list the
    /// audio thread has to keep in step. The audio thread never waits for it.
    producer: Mutex<rtrb::Producer<Arrived>>,
    dropped: AtomicU64,
}

/// Where a device layer puts the messages it reads. One per open port, all into one ring.
#[derive(Clone)]
pub struct Input(Arc<Shared>);

impl Input {
    /// Reads and sends one MIDI message, stamped with the time it arrived. Anything this
    /// application does not use is left out. Returns whether it was sent on.
    pub fn read(&self, bytes: &[u8]) -> bool {
        match Played::from_bytes(bytes) {
            Some(played) => self.send(played),
            None => false,
        }
    }

    /// Sends one message as if it arrived now. Returns whether it fitted in the ring.
    pub fn send(&self, played: Played) -> bool {
        self.send_at(monotonic_nanos(), played)
    }

    /// Sends one message with the time it arrived. Tests give the time themselves.
    pub fn send_at(&self, at_nanos: u64, played: Played) -> bool {
        let arrived = Arrived { at_nanos, played };
        // A device thread that panicked while holding the lock leaves the ring as it was: the
        // producer is a plain value with no invariant of its own to break.
        let mut producer = match self.0.producer.lock() {
            Ok(producer) => producer,
            Err(poisoned) => poisoned.into_inner(),
        };
        if producer.push(arrived).is_err() {
            self.0.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Messages that did not fit in the ring, so they were never played.
    pub fn dropped(&self) -> u64 {
        self.0.dropped.load(Ordering::Relaxed)
    }
}

/// Sends what arrived from the keyboards to an instrument, and reports what it sent.
///
/// It has no state to save and nothing to say to the control side but the reports, so its
/// update type is `()`.
pub struct Keys {
    input: rtrb::Consumer<Arrived>,
    reports: rtrb::Producer<Sounded>,
    lost_reports: Arc<AtomicU64>,
}

impl Keys {
    pub const NOTES: EventOutput<NoteEvent> = EventOutput::new(0);

    /// The processor, the handle a device layer writes into, the reports and the counter of
    /// reports that did not fit.
    pub(crate) fn new() -> (Self, Input, rtrb::Consumer<Sounded>, Arc<AtomicU64>) {
        let (producer, input) = rtrb::RingBuffer::new(INPUT_CAPACITY);
        let (reports, read_reports) = rtrb::RingBuffer::new(REPORT_CAPACITY);
        let lost_reports = Arc::new(AtomicU64::new(0));
        let keys = Self {
            input,
            reports,
            lost_reports: lost_reports.clone(),
        };
        let shared = Shared {
            producer: Mutex::new(producer),
            dropped: AtomicU64::new(0),
        };
        (keys, Input(Arc::new(shared)), read_reports, lost_reports)
    }
}

impl Processor for Keys {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new().event_output(Self::NOTES)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let ProcessContext {
            transport,
            event_outputs,
            start_frame,
            ..
        } = context;
        // A stop or a seek says nothing about a keyboard: what is held stays held, and the
        // player lifts it. The sequencer of the track sends its own `AllOff`, which does
        // release these notes too. That is the known limit of one `notes` port per instrument.
        while let Ok(arrived) = self.input.peek().copied() {
            // The event buffer is full: the message waits in the ring for the next block, so
            // nothing a keyboard sent is ever lost on the way to the instrument.
            if !event_outputs.push(Self::NOTES, 0, arrived.played.event()) {
                break;
            }
            if self.input.pop().is_err() {
                break;
            }
            let sounded = Sounded {
                arrived,
                frame: *start_frame,
                tick: transport.tick_range.start,
                playing: transport.playing,
            };
            if self.reports.push(sounded).is_err() {
                // Only the take and the latency lose this. The sound went out.
                self.lost_reports.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
