//! EventTap: the events a processor heard, each with its frame of engine time, for the control
//! side to read, as a [`Scope`] keeps a sound. A reader that fell behind by more than the ring
//! keeps learns how many events it missed.
//!
//! [`Scope`]: crate::Scope

use std::marker::PhantomData;
use std::sync::Arc;

use crate::ring::Ring;

/// An event a tap can keep: one that fits in 64 bits and back.
pub trait Tappable: Copy {
    fn to_bits(self) -> u64;
    /// `None` for bits no event gives.
    fn from_bits(bits: u64) -> Option<Self>;
}

/// The last [`EventTap::EVENTS`] events a processor wrote, each with its frame. Clones share
/// them. One writer.
#[derive(Clone)]
pub struct EventTap<T> {
    ring: Arc<Ring<2>>,
    kind: PhantomData<fn() -> T>,
}

impl<T> Default for EventTap<T> {
    fn default() -> Self {
        Self {
            ring: Arc::new(Ring::new(Self::EVENTS)),
            kind: PhantomData,
        }
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

impl<T> EventTap<T> {
    /// Far more than a dense performance plays between two polls of the interface, thirty a
    /// second.
    pub const EVENTS: usize = 4096;
}

impl<T: Tappable> EventTap<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `other` is this tap and not another one.
    pub fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.ring, &other.ring)
    }

    /// Adds the events of a block, each at its frame of engine time. Realtime safe.
    pub fn write(&self, events: impl ExactSizeIterator<Item = (u64, T)>) {
        (self.ring).write(events.map(|(frame, event)| [frame, event.to_bits()]));
    }

    /// Appends to `into` the events written since `from`, a count a read of this tap gave
    /// before or 0, oldest first, each with its frame.
    pub fn read(&self, from: u64, into: &mut Vec<(u64, T)>) -> TapRead {
        let mut read = Vec::new();
        let next = (self.ring).read(from, &mut read, |[frame, bits]| (frame, T::from_bits(bits)));
        let first = into.len();
        into.extend(
            read.into_iter()
                .filter_map(|(frame, event)| Some((frame, event?))),
        );
        let since = next - from.min(next);
        TapRead {
            next,
            missed: since - (into.len() - first) as u64,
        }
    }
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
        tap.write([(10, 1), (12, 2)].into_iter());
        let read = tap.read(0, &mut events);
        assert_eq!(events, [(10, 1), (12, 2)]);
        assert_eq!(read, TapRead { next: 2, missed: 0 });

        let more = EventTap::<u32>::EVENTS as u32 + 5;
        tap.write((0..more).map(|event| (u64::from(event), event)));
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
