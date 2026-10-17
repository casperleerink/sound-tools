//! A cable: carries a number one tool sends out into an automated number of another, block by
//! block, as an automation lane does.

use sound_core::{
    Automation, EventInput, EventOutput, Ports, PrepareConfig, ProcessContext, Processor,
};

/// A number a sound sends out: its value at the end of the block, sent every block. A number
/// that stops coming is gone, as a lane that goes away.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Number(pub f32);

/// Joins one sent number to the automation input of a device. The update is the place of the
/// number in what the device takes automation for, so the sender needs no place of its own.
#[derive(Default)]
pub struct Cable {
    parameter: u16,
}

impl Cable {
    pub const INPUT: EventInput<Number> = EventInput::new(0);
    pub const OUTPUT: EventOutput<Automation> = EventOutput::new(0);
}

impl Processor for Cable {
    type Update = u16;

    fn ports(&self) -> Ports {
        Ports::new()
            .event_input(Self::INPUT)
            .event_output(Self::OUTPUT)
    }

    fn prepare(&mut self, _: &PrepareConfig) {}

    fn update(&mut self, parameter: &mut u16) {
        self.parameter = *parameter;
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        for timed in context.event_inputs.get(Self::INPUT) {
            let automation = Automation {
                parameter: self.parameter,
                value: timed.event.0,
            };
            (context.event_outputs).push(Self::OUTPUT, timed.offset, automation);
        }
    }
}
