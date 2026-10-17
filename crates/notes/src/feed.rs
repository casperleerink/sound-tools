//! Passes notes on to one instrument, for a sender that may go away or play elsewhere while a
//! note sounds: such a sender sends no off where it played, so the instrument hears
//! [`NoteEvent::AllOff`] from the feed instead.

use sound_core::{EventInput, EventOutput, Ports, PrepareConfig, ProcessContext, Processor};

use crate::NoteEvent;

/// What the control side sends to a [`NoteFeed`].
pub enum NoteFeedUpdate {
    /// What plays into the feed changed: the next block releases everything the instrument
    /// holds before it passes anything on.
    Release,
}

/// Connected to one instrument for as long as that instrument is there, with the senders of
/// notes connected to it instead of to the instrument.
#[derive(Default)]
pub struct NoteFeed {
    release: bool,
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
            NoteFeedUpdate::Release => self.release = true,
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        // The buffer is still empty here, so the event fits.
        if std::mem::take(&mut self.release) {
            context
                .event_outputs
                .push(Self::OUTPUT, 0, NoteEvent::AllOff);
        }
        for timed in context.event_inputs.get(Self::INPUT) {
            (context.event_outputs).push(Self::OUTPUT, timed.offset, timed.event);
        }
    }
}
