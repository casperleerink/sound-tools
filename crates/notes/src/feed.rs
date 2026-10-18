//! Passes notes on to one instrument, for a sender that may go away or play elsewhere while a
//! note sounds: such a sender sends no off where it played, so the instrument hears
//! [`NoteEvent::AllOff`] from the feed instead. A performer plays the instrument through it
//! too, to the frame.

use sound_core::{
    EventInput, EventOutput, EventOutputs, Ports, PrepareConfig, ProcessContext, Processor,
};

use crate::{Amount, Bend, Controller, NoteEvent, Pedal, Performed, Pitch, Schedule};

/// The most performed events that wait for their frame: a control loop plays a little ahead.
const WAITING: usize = 256;

/// Connected to one instrument for as long as that instrument is there, with the senders of
/// notes connected to it instead of to the instrument.
pub struct NoteFeed {
    release: bool,
    performed: Schedule<NoteEvent>,
    late: Late,
}

impl Default for NoteFeed {
    fn default() -> Self {
        Self {
            release: false,
            performed: Schedule::new(WAITING),
            late: Late::default(),
        }
    }
}

pub enum NoteFeedUpdate {
    /// What plays into the feed changed: the next block releases everything the instrument
    /// holds before it passes anything on.
    Senders,
    Note(Performed),
}

/// What a performer sent that found the schedule full, to play at the start of the next block.
/// On one frame only where each controller ends up counts, so it keeps that and the offs: a
/// fixed size, whatever comes.
struct Late {
    all_off: bool,
    pedal: Option<Pedal>,
    bend: Option<Bend>,
    mod_wheel: Option<Amount>,
    pressure: Option<Amount>,
    controls: [Option<Amount>; 120],
    offs: u128,
}

impl Default for Late {
    fn default() -> Self {
        Self {
            all_off: false,
            pedal: None,
            bend: None,
            mod_wheel: None,
            pressure: None,
            controls: [None; 120],
            offs: 0,
        }
    }
}

impl Late {
    fn keep(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::AllOff => {
                *self = Self {
                    all_off: true,
                    ..Self::default()
                }
            }
            NoteEvent::Pedal(pedal) => self.pedal = Some(pedal),
            NoteEvent::Bend(bend) => self.bend = Some(bend),
            NoteEvent::ModWheel(amount) => self.mod_wheel = Some(amount),
            NoteEvent::Pressure(amount) => self.pressure = Some(amount),
            NoteEvent::Control { controller, value } => {
                if let Some(control) = self.controls.get_mut(usize::from(controller.number())) {
                    *control = Some(value);
                }
            }
            NoteEvent::Off { pitch } => self.offs |= 1 << pitch.number(),
            NoteEvent::On { .. } => {}
        }
    }

    /// Sends what it keeps at the start of the block, in the order of the note contract. What
    /// does not fit stays for the next block.
    fn send(&mut self, outputs: &mut EventOutputs<'_>) {
        let mut push = |event| outputs.push(NoteFeed::OUTPUT, 0, event);
        if self.all_off {
            if !push(NoteEvent::AllOff) {
                return;
            }
            self.all_off = false;
        }
        let standing = [
            self.pedal.take().map(NoteEvent::Pedal),
            self.bend.take().map(NoteEvent::Bend),
            self.mod_wheel.take().map(NoteEvent::ModWheel),
            self.pressure.take().map(NoteEvent::Pressure),
        ];
        let controls = std::mem::replace(&mut self.controls, [None; 120]);
        let controls = controls
            .into_iter()
            .enumerate()
            .filter_map(|(number, value)| {
                let controller = Controller::new(u8::try_from(number).ok()?).ok()?;
                let value = value?;
                Some(NoteEvent::Control { controller, value })
            });
        for event in standing.into_iter().flatten().chain(controls) {
            if !push(event) {
                self.keep(event);
            }
        }
        let mut offs = std::mem::take(&mut self.offs);
        while offs != 0 {
            let bit = offs & offs.wrapping_neg();
            let pitch = Pitch::nearest(i64::from(bit.trailing_zeros()));
            if !push(NoteEvent::Off { pitch }) {
                self.offs |= bit;
            }
            offs &= !bit;
        }
    }
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
                if let Some(event) = self.performed.perform(*performed) {
                    self.late.keep(event);
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
        self.late.send(outputs);
        // On one frame, the offs before the ons, as the note contract says, whoever sends them.
        let inputs = context.event_inputs.get(Self::INPUT);
        let is_on = |event: &NoteEvent| matches!(event, NoteEvent::On { .. });
        for timed in inputs.iter().filter(|timed| !is_on(&timed.event)) {
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
        for timed in inputs.iter().filter(|timed| is_on(&timed.event)) {
            outputs.push(Self::OUTPUT, timed.offset, timed.event);
        }
    }
}
