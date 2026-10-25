//! The MIDI input in a running engine: where it plays, and how long it takes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sound_core::{EngineControl, GraphError, InputEndpoint, Node, OutputEndpoint, StreamTiming};

use sound_notes::{Expression, Pedal};

use crate::keys::{Input, Keys, Played, Report};

/// The name of the processor in the engine graph. Instance processors are named
/// `<instance id>#<name>`, so a name without `#` can never collide with one.
const PROCESSOR: &str = "midi-input";

/// Messages that were played but did not reach the latency, and messages that were never played
/// at all. Both are zero in an ordinary session.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lost {
    /// Messages that did not fit in the input ring, so they never sounded.
    pub input: u64,
    /// Messages that sounded but whose report did not fit, so they are missing from the latency.
    pub reports: u64,
}

impl Lost {
    pub fn any(self) -> bool {
        self.input > 0 || self.reports > 0
    }
}

/// How long it took from a MIDI message arriving to the start of its sound at the device.
///
/// It covers the wait for the next audio block and the output latency the device reports. It
/// does not cover the keyboard's own scan and the USB or DIN transfer, which happen before the
/// message reaches this process and which nothing here can see.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Latency {
    count: u64,
    total_nanos: u64,
    shortest_nanos: u64,
    longest_nanos: u64,
}

impl Latency {
    fn add(&mut self, nanos: u64) {
        if self.count == 0 {
            self.shortest_nanos = nanos;
        }
        self.count += 1;
        self.total_nanos = self.total_nanos.saturating_add(nanos);
        self.shortest_nanos = self.shortest_nanos.min(nanos);
        self.longest_nanos = self.longest_nanos.max(nanos);
    }

    /// How many messages were measured.
    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn mean(&self) -> Duration {
        Duration::from_nanos(self.total_nanos / self.count.max(1))
    }

    pub fn shortest(&self) -> Duration {
        Duration::from_nanos(self.shortest_nanos)
    }

    pub fn longest(&self) -> Duration {
        Duration::from_nanos(self.longest_nanos)
    }

    /// The jitter: how much the slowest message waited longer than the fastest one.
    pub fn spread(&self) -> Duration {
        Duration::from_nanos(self.longest_nanos.saturating_sub(self.shortest_nanos))
    }
}

/// The MIDI input in the engine: one processor, and where its notes go.
///
/// It is not project state. Attaching it writes nothing and adds no undo step, like the click.
pub struct Keyboard {
    node: Node<Keys>,
    input: Input,
    reports: rtrb::Consumer<Report>,
    lost_reports: Arc<AtomicU64>,
    destinations: Vec<InputEndpoint>,
    /// Where the live input should go next. The change waits for the next [`Self::poll`], so
    /// the release of what was held is out of the processor before the connections go.
    wanted: Option<Vec<InputEndpoint>>,
    latency: Latency,
    /// Where the pedal stands, as the reports have it.
    live_pedal: Pedal,
    /// The pitches the live input holds, one bit each, as the reports have it. It is what the
    /// processor holds, so this side can tell whether a release is needed at all.
    live_notes: u128,
    /// Where the wheels stand, as the reports have it. Away from rest is held like a key.
    live_expression: Expression,
    /// Reports that were lost. Then this side does not know what is held, and a release is
    /// sent whether the mirror says so or not.
    seen_lost_reports: u64,
}

impl Keyboard {
    /// Adds the MIDI input to the engine. It plays nowhere until [`Self::play_into`].
    pub fn attach(engine: &mut EngineControl) -> Result<Self, GraphError> {
        let (keys, input, reports, lost_reports) = Keys::new();
        let mut edit = engine.edit();
        let node = edit.add_processor(PROCESSOR, keys)?;
        edit.commit()?;
        Ok(Self {
            node,
            input,
            reports,
            lost_reports,
            destinations: Vec::new(),
            wanted: None,
            latency: Latency::default(),
            live_pedal: Pedal::UP,
            live_notes: 0,
            live_expression: Expression::REST,
            seen_lost_reports: 0,
        })
    }

    /// Where a device layer, or a test, puts the messages it read.
    pub fn input(&self) -> Input {
        self.input.clone()
    }

    /// Sends the live input to these ports, each the same, or to none. They are `notes` inputs
    /// the window looks up and passes in, such as that of the selected track.
    ///
    /// The change itself happens at the next [`Self::poll`]. Everything the live input holds
    /// is released first, into the ports it plays into now: a disconnect in this block would
    /// take a connection away before the offs could go out, and the notes there would sound
    /// for ever.
    ///
    /// Call it after every change of the project: an endpoint moves when its instance is built
    /// again. The same endpoints twice cost nothing.
    pub fn play_into(
        &mut self,
        _engine: &mut EngineControl,
        destinations: &[InputEndpoint],
    ) -> Result<(), GraphError> {
        let settled = self.wanted.as_ref().unwrap_or(&self.destinations);
        if settled == destinations {
            return Ok(());
        }
        self.wanted = Some(destinations.to_vec());
        // Only when something is held. Else the change happens at the next poll with nothing
        // to wait for, which is what a track selected between two phrases does.
        if self.holds_anything() {
            self.release_held();
        }
        Ok(())
    }

    /// Whether the live input may be holding a note, the pedal or a wheel. It is what the
    /// reports say, and "yes" whenever a report was lost, because then this side does not know.
    fn holds_anything(&self) -> bool {
        self.live_notes != 0
            || self.live_pedal.is_down()
            || !self.live_expression.is_at_rest()
            || self.lost_reports.load(Ordering::Relaxed) != self.seen_lost_reports
    }

    /// Lets go of every note and the pedal the live input holds. See [`Input::release_held`]:
    /// this is the same one path.
    pub fn release_held(&self) {
        self.input.release_held();
    }

    /// The ports the live input reaches now. They are still the old ones until the next poll
    /// after a [`Self::play_into`].
    pub fn destinations(&self) -> &[InputEndpoint] {
        &self.destinations
    }

    /// Takes every report the engine sent since the last call: the latency is measured against `timing` when there is a device. It also
    /// makes the port of [`Self::play_into`] the one the live input reaches.
    ///
    /// Call it regularly. Nothing here is on the way to sound.
    pub fn poll(
        &mut self,
        engine: &mut EngineControl,
        timing: Option<&StreamTiming>,
    ) -> Result<(), GraphError> {
        self.take_reports(timing);
        // The offs of what was held must reach the instrument that holds the notes, so the
        // connection stays until the processor says they are out of it.
        if self.input.release_is_pending() {
            return Ok(());
        }
        if self.wanted.is_none() {
            return Ok(());
        }
        // A key pressed since `play_into` went to the old instrument. It is released there
        // first, or it would sound there for ever.
        if self.holds_anything() {
            self.release_held();
            return Ok(());
        }
        let destinations = self.wanted.take().unwrap_or_default();
        let notes = OutputEndpoint::new(self.node, Keys::NOTES);
        let mut edit = engine.edit();
        for old in &self.destinations {
            // The instance it played into may be gone with its processor, which takes its
            // connections with it. Then there is nothing left to disconnect.
            match edit.disconnect(&notes.to(*old)) {
                Ok(()) | Err(GraphError::UnknownConnection(_)) => {}
                Err(error) => return Err(error),
            }
        }
        for new in &destinations {
            edit.connect(notes.to(*new))?;
        }
        edit.commit()?;
        self.destinations = destinations;
        Ok(())
    }

    fn take_reports(&mut self, timing: Option<&StreamTiming>) {
        self.seen_lost_reports = self.lost_reports.load(Ordering::Relaxed);
        while let Ok(report) = self.reports.pop() {
            let sounded = match report {
                Report::Sounded(sounded) => sounded,
                Report::Released => {
                    self.live_notes = 0;
                    self.live_pedal = Pedal::UP;
                    self.live_expression = Expression::REST;
                    continue;
                }
            };
            if let Some(timing) = timing
                && let Some(sound_nanos) = timing.sound_time_nanos(sounded.frame)
            {
                self.latency
                    .add(sound_nanos.saturating_sub(sounded.arrived.at_nanos));
            }
            match sounded.arrived.played {
                Played::On { pitch, .. } => self.live_notes |= 1 << pitch.number(),
                Played::Off { pitch, .. } => self.live_notes &= !(1 << pitch.number()),
                Played::Pedal(value) => self.live_pedal = value,
                Played::Bend(_)
                | Played::ModWheel(_)
                | Played::Pressure(_)
                | Played::Control { .. } => {
                    self.live_expression.follow(sounded.arrived.played.event());
                }
            }
        }
    }

    pub fn latency(&self) -> Latency {
        self.latency
    }

    pub fn lost(&self) -> Lost {
        Lost {
            input: self.input.dropped(),
            reports: self.lost_reports.load(Ordering::Relaxed),
        }
    }
}
