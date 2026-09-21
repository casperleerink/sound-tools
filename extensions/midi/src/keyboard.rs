//! The MIDI input in a running engine: where it plays, what it records, and how long it takes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sound_core::{
    EngineControl, GraphError, InputEndpoint, Node, OutputEndpoint, StreamTiming, Ticks,
    monotonic_nanos,
};

use sound_notes::Pedal;

use crate::keys::{Input, Keys, Played, Sounded};
use crate::take::{Take, TakeEvent};

/// The name of the processor in the engine graph. Instance processors are named
/// `<instance id>#<name>`, so a name without `#` can never collide with one.
const PROCESSOR: &str = "midi-input";

/// Messages that were played but did not reach the take or the latency, and messages that were
/// never played at all. Both are zero in an ordinary session.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Lost {
    /// Messages that did not fit in the input ring, so they never sounded.
    pub input: u64,
    /// Messages that sounded but whose report did not fit, so they are missing from a take.
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

/// A recording that is going on.
struct Recording {
    /// The playhead where it began.
    start: Ticks,
    /// The monotonic time where it began, so the take's times start at zero.
    started_nanos: u64,
    /// Where the pedal stood when it began, so a take under a held pedal plays back as it
    /// sounded.
    pedal_at_start: Pedal,
    events: Vec<TakeEvent>,
}

/// The MIDI input in the engine: one processor, where its notes go, and what it records.
///
/// It is not project state. Attaching it writes nothing and adds no undo step, like the click.
/// A finished recording is an edit of the project, which the caller makes.
pub struct Keyboard {
    node: Node<Keys>,
    input: Input,
    reports: rtrb::Consumer<Sounded>,
    lost_reports: Arc<AtomicU64>,
    destination: Option<InputEndpoint>,
    /// Where the live input should go next. The change waits for the next [`Self::poll`], so
    /// the release of what was held is out of the processor before the connection goes.
    wanted: Option<Option<InputEndpoint>>,
    recording: Option<Recording>,
    latency: Latency,
    /// Where the pedal stands, as the reports have it. A recording that begins under a held
    /// pedal starts with this value.
    live_pedal: Pedal,
    /// The pitches the live input holds, one bit each, as the reports have it. It is what the
    /// processor holds, so this side can tell whether a release is needed at all.
    live_notes: u128,
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
            destination: None,
            wanted: None,
            recording: None,
            latency: Latency::default(),
            live_pedal: Pedal::UP,
            live_notes: 0,
            seen_lost_reports: 0,
        })
    }

    /// Where a device layer, or a test, puts the messages it read.
    pub fn input(&self) -> Input {
        self.input.clone()
    }

    /// Sends the live input to this port, or to nowhere. It is the `notes` input of the
    /// instrument of the selected track, which the window looks up and passes in.
    ///
    /// The change itself happens at the next [`Self::poll`]. Everything the live input holds
    /// is released first, into the instrument it plays into now: a disconnect in this block
    /// would take the connection away before the offs could go out, and the notes of the old
    /// instrument would sound for ever.
    ///
    /// Call it after every change of the project: the endpoint moves when the instrument is
    /// built again. The same endpoint twice costs nothing.
    pub fn play_into(
        &mut self,
        _engine: &mut EngineControl,
        destination: Option<InputEndpoint>,
    ) -> Result<(), GraphError> {
        let settled = self.wanted.unwrap_or(self.destination);
        if settled == destination {
            return Ok(());
        }
        self.wanted = Some(destination);
        // Only when something is held. Else the change happens at the next poll with nothing
        // to wait for, which is what a track selected between two phrases does.
        if self.holds_anything() {
            self.release_held();
        }
        Ok(())
    }

    /// Whether the live input may be holding a note or the pedal. It is what the reports say,
    /// and "yes" whenever a report was lost, because then this side does not know.
    fn holds_anything(&self) -> bool {
        self.live_notes != 0
            || self.live_pedal.is_down()
            || self.lost_reports.load(Ordering::Relaxed) != self.seen_lost_reports
    }

    /// Lets go of every note and the pedal the live input holds, at the start of the next
    /// audio block. See [`Input::release_held`]: this is the same one path.
    pub fn release_held(&self) {
        self.input.release_held();
    }

    /// The port the live input reaches now. It is still the old one until the next poll after
    /// a [`Self::play_into`].
    pub fn destination(&self) -> Option<InputEndpoint> {
        self.destination
    }

    /// Takes every report the engine sent since the last call: a recording grows by what was
    /// played, and the latency is measured against `timing` when there is a device. It also
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
        let Some(destination) = self.wanted.take() else {
            return Ok(());
        };
        let notes = OutputEndpoint::new(self.node, Keys::NOTES);
        let mut edit = engine.edit();
        if let Some(old) = self.destination {
            // The instrument it played into may be gone with its processor, which takes its
            // connections with it. Then there is nothing left to disconnect.
            match edit.disconnect(&notes.to(old)) {
                Ok(()) | Err(GraphError::UnknownConnection(_)) => {}
                Err(error) => return Err(error),
            }
        }
        if let Some(new) = destination {
            edit.connect(notes.to(new))?;
        }
        edit.commit()?;
        self.destination = destination;
        Ok(())
    }

    fn take_reports(&mut self, timing: Option<&StreamTiming>) {
        self.seen_lost_reports = self.lost_reports.load(Ordering::Relaxed);
        while let Ok(sounded) = self.reports.pop() {
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
            }
            let Some(recording) = &mut self.recording else {
                continue;
            };
            // Only what the project played from the start of the recording on. A key that was
            // pressed before, while the project was stopped, is not part of the take.
            if !sounded.playing || sounded.tick < recording.start {
                continue;
            }
            let since = sounded
                .arrived
                .at_nanos
                .saturating_sub(recording.started_nanos);
            recording.events.push(TakeEvent {
                time_us: since / 1_000,
                tick: sounded.tick,
                played: sounded.arrived.played,
            });
        }
    }

    /// Starts recording from the playhead. Whatever the engine sounds from here on is in the
    /// take. A recording that was going on is dropped.
    pub fn start_recording(&mut self, at: Ticks) {
        self.recording = Some(Recording {
            start: at,
            started_nanos: monotonic_nanos(),
            pedal_at_start: self.live_pedal,
            events: Vec::new(),
        });
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Ends the recording at the playhead and gives the take. `None` when nothing was
    /// recording. Poll once before this, so the last block is in the take.
    pub fn finish_recording(&mut self, until: Ticks) -> Option<Take> {
        let recording = self.recording.take()?;
        Some(Take {
            start: recording.start,
            end: until.max(recording.start),
            pedal_at_start: recording.pedal_at_start,
            events: recording.events,
        })
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
