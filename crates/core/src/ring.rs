//! A ring of atomics one thread writes and another copies from: no lock, no allocation and no
//! message. A [`Scope`] keeps the sound in one, an [`EventTap`] events.
//!
//! [`Scope`]: crate::Scope
//! [`EventTap`]: crate::EventTap

use std::sync::atomic::{AtomicU64, Ordering, fence};

#[derive(Debug)]
pub(crate) struct Ring<const N: usize> {
    slots: Box<[[AtomicU64; N]]>,
    /// Items written so far, counted when they are in the ring.
    written: AtomicU64,
    /// Items written so far, counted before they go in: a reader that finds this past what it
    /// read knows those slots may have been written over.
    writing: AtomicU64,
}

impl<const N: usize> Ring<N> {
    pub(crate) fn new(length: usize) -> Self {
        let slots = (0..length).map(|_| std::array::from_fn(|_| AtomicU64::new(0)));
        Self {
            slots: slots.collect(),
            written: AtomicU64::new(0),
            writing: AtomicU64::new(0),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    /// Adds `items`. Realtime safe. One writer.
    pub(crate) fn write(&self, items: impl ExactSizeIterator<Item = [u64; N]>) {
        let start = self.written.load(Ordering::Relaxed);
        let end = start + items.len() as u64;
        self.writing.store(end, Ordering::Relaxed);
        fence(Ordering::Release);
        for (count, item) in (start..).zip(items) {
            if let Some(slot) = self.slots.get(self.slot(count)) {
                for (atomic, value) in slot.iter().zip(item) {
                    atomic.store(value, Ordering::Relaxed);
                }
            }
        }
        self.written.store(end, Ordering::Release);
    }

    /// Appends to `into` what `decode` makes of the items written since `from`, a count a read
    /// gave before, 0, or `u64::MAX` for none, oldest first. Gives the count to read from next
    /// time. Items written over before they were read are left out.
    pub(crate) fn read<T>(
        &self,
        from: u64,
        into: &mut Vec<T>,
        decode: impl Fn([u64; N]) -> T,
    ) -> u64 {
        let written = self.written.load(Ordering::Acquire);
        let start = from.clamp(self.oldest(written), written);
        let first = into.len();
        for count in start..written {
            if let Some(slot) = self.slots.get(self.slot(count)) {
                into.push(decode(
                    slot.each_ref().map(|atomic| atomic.load(Ordering::Relaxed)),
                ));
            }
        }
        // A slot the writer reached while it was read may hold a newer item.
        fence(Ordering::Acquire);
        let overwritten = self.oldest(self.writing.load(Ordering::Relaxed));
        let overwritten = (overwritten.saturating_sub(start) as usize).min(into.len() - first);
        into.drain(first..first + overwritten);
        written
    }

    fn oldest(&self, count: u64) -> u64 {
        count.saturating_sub(self.len() as u64)
    }

    fn slot(&self, count: u64) -> usize {
        (count % self.len() as u64) as usize
    }
}
