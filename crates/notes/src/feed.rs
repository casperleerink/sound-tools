//! Passes notes on to one instrument, for a sender that may go away or play elsewhere while a
//! note sounds: such a sender sends no off where it played, so the instrument hears
//! [`NoteEvent::AllOff`] from the feed instead. A performer plays the instrument through it
//! too, to the frame.

use sound_core::{
    EventInput, EventOutput, EventOutputs, Ports, PrepareConfig, ProcessContext, Processor,
};

use crate::{Amount, Bend, Controller, NoteEvent, Performed, Pitch, Schedule};

/// The most performed events that wait for their frame: a control loop plays a little ahead.
const WAITING: usize = 256;

/// Connected to one instrument for as long as that instrument is there, with the senders of
/// notes connected to it instead of to the instrument.
pub struct NoteFeed {
    performed: Schedule<NoteEvent>,
    now: Now,
}

impl Default for NoteFeed {
    fn default() -> Self {
        Self {
            performed: Schedule::new(WAITING),
            now: Now::default(),
        }
    }
}

pub enum NoteFeedUpdate {
    /// What plays into the feed changed: the next block releases everything the instrument
    /// holds before it passes anything on.
    Senders,
    Note(Performed),
}

/// What plays at the start of the next block: the release of a change of senders, and what a
/// performer sent that found the schedule full. On one frame only where each controller ends up
/// counts, so it keeps that and the offs: a fixed size, whatever comes.
struct Now {
    waits: bool,
    all_off: bool,
    bend: Option<Bend>,
    pressure: Option<Amount>,
    /// By MIDI controller number, the pedal and the mod wheel too.
    controls: [Option<Amount>; 128],
    offs: u128,
}

impl Default for Now {
    fn default() -> Self {
        Self {
            waits: false,
            all_off: false,
            bend: None,
            pressure: None,
            controls: [None; 128],
            offs: 0,
        }
    }
}

impl Now {
    fn keep(&mut self, event: NoteEvent) {
        self.waits = true;
        match event {
            // It puts the pedal, the wheels and the pressure at rest, and leaves the other
            // controllers where they are.
            NoteEvent::AllOff => {
                self.all_off = true;
                (self.bend, self.pressure, self.offs) = (None, None, 0);
                for number in [Controller::SUSTAIN_PEDAL, Controller::MOD_WHEEL] {
                    self.controls[usize::from(number)] = None;
                }
            }
            NoteEvent::Bend(bend) => self.bend = Some(bend),
            NoteEvent::Pressure(amount) => self.pressure = Some(amount),
            NoteEvent::Off { pitch } => self.offs |= 1 << pitch.number(),
            NoteEvent::On { .. } => {}
            NoteEvent::Pedal(_) | NoteEvent::ModWheel(_) | NoteEvent::Control { .. } => {
                if let Some((number, value)) = event.as_controller() {
                    self.controls[usize::from(number)] = Some(value);
                }
            }
        }
    }

    /// Sends what it keeps at the start of the block, in the order of the note contract. What
    /// does not fit stays for the next block.
    fn send(&mut self, outputs: &mut EventOutputs<'_>) {
        if !self.waits {
            return;
        }
        let mut push = |event| outputs.push(NoteFeed::OUTPUT, 0, event);
        if self.all_off && !push(NoteEvent::AllOff) {
            return;
        }
        let Self {
            bend,
            pressure,
            controls,
            offs,
            ..
        } = std::mem::take(self);
        let controls = (controls.into_iter().zip(0..))
            .filter_map(|(value, number)| NoteEvent::controller(number, value?));
        let standing = [bend.map(NoteEvent::Bend), pressure.map(NoteEvent::Pressure)];
        let pitches = (0..128).filter(|pitch| offs & (1 << pitch) != 0);
        let offs = pitches.map(|pitch| NoteEvent::Off {
            pitch: Pitch::nearest(pitch),
        });
        for event in controls.chain(standing.into_iter().flatten()).chain(offs) {
            if !push(event) {
                self.keep(event);
            }
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
            NoteFeedUpdate::Senders => self.now.keep(NoteEvent::AllOff),
            NoteFeedUpdate::Note(performed) => {
                if let Some(event) = self.performed.perform(*performed) {
                    self.now.keep(event);
                }
            }
        }
    }

    fn process(&mut self, context: &mut ProcessContext<'_>) {
        for _ in 0..self.performed.take_dropped() {
            context.event_outputs.count_dropped();
        }
        let outputs = &mut context.event_outputs;
        self.now.send(outputs);
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
