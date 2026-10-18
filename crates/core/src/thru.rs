//! Processors that pass on what reaches them unchanged: a port many connections meet at, such
//! as an input of an instance that several of its processors hear.

use std::marker::PhantomData;

use crate::processor::{
    AudioInput, AudioOutput, Event, EventInput, EventOutput, Ports, PrepareConfig, ProcessContext,
    Processor,
};

/// Passes on its sound.
#[derive(Default)]
pub struct AudioThru;

impl AudioThru {
    pub const INPUT: AudioInput = AudioInput::new(0);
    pub const OUTPUT: AudioOutput = AudioOutput::new(0);
}

impl Processor for AudioThru {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new()
            .audio_input(Self::INPUT)
            .audio_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        let input = context.audio_inputs.get(Self::INPUT);
        for (output, input) in context
            .audio_outputs
            .get(Self::OUTPUT)
            .into_iter()
            .zip(input)
        {
            for (sample, from) in output.iter_mut().zip(input) {
                *sample = *from;
            }
        }
    }
}

/// Passes on its events, each on its frame.
pub struct EventThru<E> {
    events: PhantomData<fn(E)>,
}

impl<E> Default for EventThru<E> {
    fn default() -> Self {
        Self {
            events: PhantomData,
        }
    }
}

impl<E: Event> EventThru<E> {
    pub const INPUT: EventInput<E> = EventInput::new(0);
    pub const OUTPUT: EventOutput<E> = EventOutput::new(0);
}

impl<E: Event> Processor for EventThru<E> {
    type Update = ();

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::INPUT)
            .event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, _: &mut ()) {}

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        for timed in context.event_inputs.get(Self::INPUT) {
            (context.event_outputs).push(Self::OUTPUT, timed.offset, timed.event);
        }
    }
}
