//! Passes notes on to one instrument, for a sender that may go away or play elsewhere while a
//! note sounds: such a sender sends no off where it played, so the instrument hears
//! [`NoteEvent::AllOff`] from the feed instead. A performer plays the instrument through it
//! too, to the frame.

use sound_core::{EventInput, EventOutput, Ports, PrepareConfig, ProcessContext, Processor};

use crate::{NoteEvent, Performed, Pitch, Schedule};

/// The most performed notes that wait for their frame: a control loop plays a little ahead.
const WAITING: usize = 256;

/// Connected to one instrument for as long as that instrument is there, with the senders of
/// notes connected to it instead of to the instrument.
pub struct NoteFeed {
    release: bool,
    performed: Schedule<NoteEvent>,
    /// The pitches whose release found the schedule full, which let go in the next block, or
    /// the one after when the output is full.
    let_go: u128,
}

impl Default for NoteFeed {
    fn default() -> Self {
        Self {
            release: false,
            performed: Schedule::new(WAITING),
            let_go: 0,
        }
    }
}

pub enum NoteFeedUpdate {
    /// What plays into the feed changed: the next block releases everything the instrument
    /// holds before it passes anything on.
    Senders,
    Note(Performed),
}

impl NoteFeed {
    pub const INPUT: EventInput<NoteEvent> = EventInput::new(0);
    pub const OUTPUT: EventOutput<NoteEvent> = EventOutput::new(0);
}

impl Processor for NoteFeed {
    type Update = NoteFeedUpdate;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::INPUT)
            .event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, update: &mut NoteFeedUpdate) {
        match update {
            NoteFeedUpdate::Senders => self.release = true,
            NoteFeedUpdate::Note(performed) => {
                if let Some(NoteEvent::Off { pitch }) = self.performed.perform(*performed) {
                    self.let_go |= 1 << pitch.number();
                }
            }
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let outputs = &mut context.event_outputs;
        // The buffer is still empty here, so the event fits.
        if std::mem::take(&mut self.release) {
            outputs.push(Self::OUTPUT, 0, NoteEvent::AllOff);
        }
        let mut let_go = std::mem::take(&mut self.let_go);
        while let_go != 0 {
            let bit = let_go & let_go.wrapping_neg();
            let pitch = Pitch::nearest(i64::from(bit.trailing_zeros()));
            if !outputs.push(Self::OUTPUT, 0, NoteEvent::Off { pitch }) {
                self.let_go |= bit;
            }
            let_go &= !bit;
        }
        for timed in context.event_inputs.get(Self::INPUT) {
            outputs.push(Self::OUTPUT, timed.offset, timed.event);
        }
        self.performed.playing(context.start_frame, context.frames);
        let last = context.start_frame + context.frames.saturating_sub(1) as u64;
        while let Some((at, event)) = self.performed.take(last) {
            let offset = usize::try_from(at.saturating_sub(context.start_frame)).unwrap_or(0);
            // A dropped off would hold its note for ever: what does not fit waits, in order.
            if !outputs.push(Self::OUTPUT, offset, event) {
                self.performed.put_back((at, event));
                break;
            }
        }
    }
}
