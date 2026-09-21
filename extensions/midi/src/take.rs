//! A recording while it runs, and the saved take it becomes.
//!
//! The messages arrive with the moment they reached this process and with the tick the engine
//! sounded them on. Both become microseconds when the take is written, because the saved form
//! ([`sound_notes::RawTake`]) must keep its meaning when the tempo map changes under it, which
//! is exactly what step 7 does to it.

use sound_core::{Clock, Ticks};
use sound_notes::{Clip, Pedal, RawEvent, RawTake};

use crate::keys::Played;

/// One message of a take: what arrived and when, plus where the engine sounded it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TakeEvent {
    /// Microseconds from the moment recording began.
    pub time_us: u64,
    /// The project tick the engine sounded it on.
    pub tick: Ticks,
    pub played: Played,
}

/// Everything that arrived between the start and the end of one recording.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Take {
    /// The playhead where recording began.
    pub start: Ticks,
    /// The playhead where it ended.
    pub end: Ticks,
    /// Where the sustain pedal stood when recording began. A take that begins under a held
    /// pedal plays back as it sounded, though the keys that were already down are left out.
    pub pedal_at_start: Pedal,
    pub events: Vec<TakeEvent>,
}

impl Take {
    /// Whether anything was played. An empty take makes no clip and no file.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The last tick the clip has to cover: its own end, or one tick past the last message
    /// when the recording was stopped before the engine had passed it.
    fn last_tick(&self) -> Ticks {
        let last = self.events.iter().map(|event| event.tick).max();
        let past_last = last.map_or(Ticks(0), |tick| Ticks(tick.0 + 1));
        self.end.max(past_last).max(Ticks(self.start.0 + 1))
    }

    /// The take in its saved form, with every time in microseconds on `clock`, which is the
    /// clock the engine played it by. From here on nothing depends on the tempo map any more.
    pub fn raw(&self, clock: &Clock) -> RawTake {
        let start_us = clock.micros_of(self.start);
        let last = self.last_tick();
        RawTake {
            start_us,
            end_us: clock.micros_of(last),
            start_tick: self.start.0,
            end_tick: last.0,
            pedal_at_start: self.pedal_at_start.value(),
            events: self
                .events
                .iter()
                .map(|event| {
                    let time_us = event.time_us;
                    let sounded_us = clock.micros_of(event.tick).saturating_sub(start_us);
                    match event.played {
                        Played::On { pitch, velocity } => RawEvent::On {
                            time_us,
                            sounded_us,
                            pitch: pitch.number(),
                            velocity: velocity.value(),
                        },
                        Played::Off { pitch, velocity } => RawEvent::Off {
                            time_us,
                            sounded_us,
                            pitch: pitch.number(),
                            velocity,
                        },
                        Played::Pedal(value) => RawEvent::Pedal {
                            time_us,
                            sounded_us,
                            value: value.value(),
                        },
                    }
                })
                .collect(),
        }
    }

    /// The clip of this take, at the place it was played: every message on the tick the engine
    /// sounded it, so playing the clip renders what was heard.
    ///
    /// The rules are [`RawTake::clip`]'s, and the tick of a microsecond time is the one the
    /// engine used, because a tick turns into microseconds and back without loss.
    pub fn clip(&self, clock: &Clock) -> Option<Clip> {
        self.raw(clock)
            .clip(|time_us| clock.tick_at_micros(time_us))
    }
}
