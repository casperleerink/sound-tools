use std::marker::PhantomData;

use sound_core::{
    Automation, Event, EventInput, EventOutput, Ports, PrepareConfig, ProcessContext, Processor,
};

/// A number a sound sends out: its value at the end of the block, sent every block. A number
/// that stops coming is gone, as a lane that goes away.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Number(pub f32);

/// A bang a sound sends out, on the frame its signal goes above 0.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Bang;

/// What reaches a live control or a trigger of a [`Signals`](crate::Signals) by a cable, by its
/// index among those of its kind.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Control {
    Live { index: u16, value: f32 },
    Trigger { index: u16 },
}

/// An event `From` that a cable delivers as one of these, to the number of a device at `place`.
pub trait Placed<From>: Event {
    fn placed(from: From, place: u16) -> Self;
}

impl Placed<Number> for Automation {
    fn placed(Number(value): Number, parameter: u16) -> Self {
        Self { parameter, value }
    }
}

impl Placed<Number> for Control {
    fn placed(Number(value): Number, index: u16) -> Self {
        Self::Live { index, value }
    }
}

impl Placed<Bang> for Control {
    fn placed(_: Bang, index: u16) -> Self {
        Self::Trigger { index }
    }
}

/// Joins one sender to a device that takes many numbers on one input, such as its automation.
/// The update is the place of the number there, so the sender needs no place of its own.
pub struct Cable<From, To> {
    place: u16,
    events: PhantomData<fn(From) -> To>,
}

impl<From, To> Default for Cable<From, To> {
    fn default() -> Self {
        Self {
            place: 0,
            events: PhantomData,
        }
    }
}

impl<From: Event, To: Placed<From>> Cable<From, To> {
    pub const INPUT: EventInput<From> = EventInput::new(0);
    pub const OUTPUT: EventOutput<To> = EventOutput::new(0);
}

impl<From: Event, To: Placed<From>> Processor for Cable<From, To> {
    type Update = u16;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::INPUT)
            .event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, place: &mut u16) {
        self.place = *place;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        for timed in context.event_inputs.get(Self::INPUT) {
            let event = To::placed(timed.event, self.place);
            (context.event_outputs).push(Self::OUTPUT, timed.offset, event);
        }
    }
}
