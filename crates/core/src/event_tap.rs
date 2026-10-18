//! EventTap: the events a processor heard, each with its frame of engine time, for the control
//! side to read. Like a [`Scope`], a ring of atomics one audio thread writes and a reader copies
//! from: no lock, no allocation and no message. A reader that fell behind by more than the ring
//! keeps learns how many events it missed.
//!
//! [`Scope`]: crate::Scope

use std::any::TypeId;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering, fence};

/// How many events a tap keeps: far more than a dense performance plays between two polls of
/// the interface, thirty a second.
const EVENTS: usize = 4096;

/// An event a tap can keep: one that fits in 64 bits and back.
pub trait Tappable: Copy {
    fn to_bits(self) -> u64;
    /// `None` for bits no event gives.
    fn from_bits(bits: u64) -> Option<Self>;
}

/// The last [`EventTap::EVENTS`] events a processor wrote. Clones share them. One writer.
pub struct EventTap<T> {
    ring: Arc<Ring>,
    kind: PhantomData<fn() -> T>,
}

struct Ring {
    /// The frame and the bits of each event.
    slots: Box<[[AtomicU64; 2]]>,
    /// Events written so far, counted when they are in the ring.
    written: AtomicU64,
    /// Events written so far, counted before they go in: a reader that finds this past what it
    /// read knows those slots may have been written over.
    writing: AtomicU64,
}

impl<T> Clone for EventTap<T> {
    fn clone(&self) -> Self {
        Self {
            ring: self.ring.clone(),
            kind: PhantomData,
        }
    }
}

impl<T> Default for EventTap<T> {
    fn default() -> Self {
        let slots = (0..EVENTS).map(|_| [0, 0].map(AtomicU64::new));
        let ring = Ring {
            slots: slots.collect(),
            written: AtomicU64::new(0),
            writing: AtomicU64::new(0),
        };
        Self {
            ring: Arc::new(ring),
            kind: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for EventTap<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let written = self.ring.written.load(Ordering::Relaxed);
        formatter
            .debug_struct("EventTap")
            .field("written", &written)
            .finish()
    }
}

/// What a read of a tap gave.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TapRead {
    /// The count to read from next time.
    pub next: u64,
    /// Events written over before they were read.
    pub missed: u64,
}

impl<T: Tappable> EventTap<T> {
    pub const EVENTS: usize = EVENTS;

    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `other` is this tap and not another one.
    pub fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ring, &other.ring)
    }

    /// Adds one event at `frame` of engine time. Realtime safe.
    pub fn write(&self, frame: u64, event: T) {
        let ring = &self.ring;
        let count = ring.written.load(Ordering::Relaxed);
        ring.writing.store(count + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        if let Some([at, bits]) = ring.slots.get(slot(count)) {
            at.store(frame, Ordering::Relaxed);
            bits.store(event.to_bits(), Ordering::Relaxed);
        }
        ring.written.store(count + 1, Ordering::Release);
    }

    /// Appends to `into` the events written since `from`, a count a read gave before or 0,
    /// oldest first, each with its frame.
    pub fn read(&self, from: u64, into: &mut Vec<(u64, T)>) -> TapRead {
        let ring = &self.ring;
        let written = ring.written.load(Ordering::Acquire);
        let oldest = |count: u64| count.saturating_sub(Self::EVENTS as u64);
        let start = from.clamp(oldest(written), written);
        let slots = (start..written).filter_map(|count| {
            let [at, bits] = ring.slots.get(slot(count))?;
            Some((
                count,
                at.load(Ordering::Relaxed),
                bits.load(Ordering::Relaxed),
            ))
        });
        let copied: Vec<(u64, u64, u64)> = slots.collect();
        // A slot the writer reached while it was read may hold a newer event.
        fence(Ordering::Acquire);
        let intact = oldest(ring.writing.load(Ordering::Relaxed));
        let first = into.len();
        let kept = copied.into_iter().filter(|(count, ..)| *count >= intact);
        into.extend(kept.filter_map(|(_, at, bits)| Some((at, T::from_bits(bits)?))));
        let since = written - from.min(written);
        TapRead {
            next: written,
            missed: since - (into.len() - first) as u64,
        }
    }
}

/// A tap of any event type, as an instance keeps it.
#[derive(Clone)]
pub(crate) struct AnyTap {
    ring: Arc<Ring>,
    kind: TypeId,
}

impl AnyTap {
    pub(crate) fn of<T: Tappable + 'static>(tap: &EventTap<T>) -> Self {
        Self {
            ring: tap.ring.clone(),
            kind: TypeId::of::<T>(),
        }
    }

    /// The tap, when it keeps events of `T`.
    pub(crate) fn typed<T: Tappable + 'static>(&self) -> Option<EventTap<T>> {
        (self.kind == TypeId::of::<T>()).then(|| EventTap {
            ring: self.ring.clone(),
            kind: PhantomData,
        })
    }
}

fn slot(count: u64) -> usize {
    (count % EVENTS as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Tappable for u32 {
        fn to_bits(self) -> u64 {
            u64::from(self)
        }

        fn from_bits(bits: u64) -> Option<Self> {
            u32::try_from(bits).ok()
        }
    }

    #[test]
    fn a_read_gives_what_came_since_the_last_and_counts_what_was_written_over() {
        let tap = EventTap::<u32>::new();
        let mut events = Vec::new();
        tap.write(10, 1);
        tap.write(12, 2);
        let read = tap.read(0, &mut events);
        assert_eq!(events, [(10, 1), (12, 2)]);
        assert_eq!(read, TapRead { next: 2, missed: 0 });

        let more = EventTap::<u32>::EVENTS as u32 + 5;
        for event in 0..more {
            tap.write(u64::from(event), event);
        }
        events.clear();
        let read = tap.read(read.next, &mut events);
        // The newest the ring keeps, and the count of the rest.
        assert_eq!(events.len(), EventTap::<u32>::EVENTS);
        assert_eq!(events.first(), Some(&(5, 5)));
        assert_eq!(events.last(), Some(&(u64::from(more - 1), more - 1)));
        assert_eq!(read.missed, 5);
        assert_eq!(read.next, 2 + u64::from(more));
    }
}
