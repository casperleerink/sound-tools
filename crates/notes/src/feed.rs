//! Passes notes on to one instrument, for a sender that may go away or play elsewhere while a
//! note sounds: such a sender sends no off where it played, so the instrument hears
//! [`NoteEvent::AllOff`] from the feed instead. A performer plays the instrument through it
//! too, to the frame.
//!
//! An off releases every note of its pitch, and several senders may hold one pitch, such as two
//! clips that overlap, or a clip and a key. So the feed counts the ons of each pitch and passes
//! an off on only when it ends the last of them.
//!
//! It counts the performer's ons apart too: a performer that goes away, such as code that
//! restarts, sends no off for what it holds.

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
    /// How many ons of each pitch reached the instrument and have not ended.
    held: [u8; 128],
    /// How many of `held` the performer played.
    performer_held: [u8; 128],
}

impl Default for NoteFeed {
    fn default() -> Self {
        Self {
            performed: Schedule::new(WAITING),
            now: Now::default(),
            held: [0; 128],
            performer_held: [0; 128],
        }
    }
}

pub enum NoteFeedUpdate {
    /// What plays into the feed changed: the next block releases everything the instrument
    /// holds before it passes anything on.
    Senders,
    Note(Performed),
    /// The performer is gone: what it holds ends now, and what it sent to play later does not
    /// play.
    PerformerGone,
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

    /// Passes `event`, from the performer or not, on unless it is an off another on of its
    /// pitch still holds. False when it did not fit.
    fn pass(
        &mut self,
        outputs: &mut EventOutputs<'_>,
        offset: usize,
        event: NoteEvent,
        performed: bool,
    ) -> bool {
        match event {
            NoteEvent::Off { pitch } => {
                let index = usize::from(pitch.number());
                let (held, performer) = (&mut self.held[index], &mut self.performer_held[index]);
                if performed {
                    *performer = performer.saturating_sub(1);
                }
                if *held > 1 {
                    *held -= 1;
                    *performer = (*performer).min(*held);
                    return true;
                }
                (*held, *performer) = (0, 0);
            }
            NoteEvent::AllOff => (self.held, self.performer_held) = ([0; 128], [0; 128]),
            _ => {}
        }
        let sent = outputs.push(Self::OUTPUT, offset, event);
        if sent && let NoteEvent::On { pitch, .. } = event {
            let index = usize::from(pitch.number());
            self.held[index] = self.held[index].saturating_add(1);
            if performed {
                self.performer_held[index] = self.performer_held[index].saturating_add(1);
            }
        }
        sent
    }
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
            NoteFeedUpdate::Senders => {
                (self.held, self.performer_held) = ([0; 128], [0; 128]);
                self.now.keep(NoteEvent::AllOff);
            }
            NoteFeedUpdate::PerformerGone => {
                self.performed.clear();
                let held = self.held.iter_mut().zip(&mut self.performer_held);
                for (number, (held, performer)) in (0..).zip(held) {
                    if *performer == 0 {
                        continue;
                    }
                    *held = held.saturating_sub(std::mem::take(performer));
                    if *held == 0 {
                        self.now.keep(NoteEvent::Off {
                            pitch: Pitch::nearest(number),
                        });
                    }
                }
            }
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
        let start = context.start_frame;
        let outputs = &mut context.event_outputs;
        self.now.send(outputs);
        self.performed.playing(start, context.frames);
        // In the order they play, so an off finds the ons before it counted. On one frame the
        // offs before the ons, as the note contract says, whoever sends them.
        let inputs = context.event_inputs.get(Self::INPUT);
        let is_on = |event: &NoteEvent| matches!(event, NoteEvent::On { .. });
        let mut performing = true;
        let mut rest = inputs;
        while let Some(offset) = rest.first().map(|timed| timed.offset) {
            let (frame, after) =
                rest.split_at(rest.partition_point(|timed| timed.offset == offset));
            rest = after;
            performing = performing && self.perform(outputs, start, start + offset as u64);
            for timed in frame.iter().filter(|timed| !is_on(&timed.event)) {
                self.pass(outputs, offset, timed.event, false);
            }
            for timed in frame.iter().filter(|timed| is_on(&timed.event)) {
                self.pass(outputs, offset, timed.event, false);
            }
        }
        if performing {
            self.perform(outputs, start, start + context.frames as u64);
        }
    }
}

impl NoteFeed {
    /// Passes on what a performer played before frame `before`. False once one did not fit:
    /// it waits for the next block, and what comes after it too.
    fn perform(&mut self, outputs: &mut EventOutputs<'_>, start: u64, before: u64) -> bool {
        let Some(last) = before.checked_sub(1) else {
            return true;
        };
        while let Some((at, event)) = self.performed.take(last) {
            let offset = usize::try_from(at.saturating_sub(start)).unwrap_or(0);
            if !self.pass(outputs, offset, event, true) {
                self.performed.put_back((at, event));
                return false;
            }
        }
        true
    }
}
