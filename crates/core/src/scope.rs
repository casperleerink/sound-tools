//! Scope: the last frames of a sound, for the interface to look at. [`Peaks`] give a level; a
//! scope gives the sound itself, for what the interface works out of it, such as a spectrum,
//! so that work stays off the audio thread. The audio thread writes each sample into a ring of
//! atomics and counts the frames; the interface copies what came since it last looked. No
//! lock, no allocation and no message.
//!
//! [`Peaks`]: crate::Peaks

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

use crate::processor::CHANNELS;

/// The last [`Scope::FRAMES`] frames a processor wrote. Clones share them. One writer.
#[derive(Clone, Debug)]
pub struct Scope(Arc<Ring>);

#[derive(Debug)]
struct Ring {
    frames: Box<[[AtomicU32; CHANNELS]]>,
    /// Frames written so far, counted when they are in the ring.
    written: AtomicU64,
    /// Frames written so far, counted before they go in: a reader that finds this past what it
    /// read knows those frames may have been written over.
    writing: AtomicU64,
}

impl Default for Scope {
    fn default() -> Self {
        let frames = (0..Self::FRAMES).map(|_| [0, 0].map(AtomicU32::new));
        Self(Arc::new(Ring {
            frames: frames.collect(),
            written: AtomicU64::new(0),
            writing: AtomicU64::new(0),
        }))
    }
}

impl Scope {
    /// How many frames it keeps: 170 ms at 48 kHz, ten polls of the interface.
    pub const FRAMES: usize = 8192;

    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the frames of one block. Realtime safe.
    pub fn write(&self, channels: [&[f32]; CHANNELS]) {
        let ring = &self.0;
        let [left, right] = channels;
        let frames = left.len().min(right.len()) as u64;
        let start = ring.written.load(Ordering::Relaxed);
        ring.writing.store(start + frames, Ordering::Relaxed);
        fence(Ordering::Release);
        for (frame, (left, right)) in (start..).zip(left.iter().zip(right)) {
            if let Some([left_slot, right_slot]) = ring.frames.get(slot(frame)) {
                left_slot.store(left.to_bits(), Ordering::Relaxed);
                right_slot.store(right.to_bits(), Ordering::Relaxed);
            }
        }
        ring.written.store(start + frames, Ordering::Release);
    }

    /// Appends to `into` the frames written since `from`, a count [`Self::read`] gave before, 0,
    /// or `u64::MAX` for none, oldest first. Gives the count to read from next time. Frames
    /// written over before they were read are left out.
    pub fn read(&self, from: u64, into: &mut Vec<[f32; CHANNELS]>) -> u64 {
        let ring = &self.0;
        let written = ring.written.load(Ordering::Acquire);
        let oldest = |count: u64| count.saturating_sub(Self::FRAMES as u64);
        let start = from.clamp(oldest(written), written);
        let first = into.len();
        for frame in start..written {
            if let Some(slot) = ring.frames.get(slot(frame)) {
                into.push(
                    slot.each_ref()
                        .map(|sample| f32::from_bits(sample.load(Ordering::Relaxed))),
                );
            }
        }
        // A frame the writer reached while it was read may be a newer one.
        fence(Ordering::Acquire);
        let overwritten = oldest(ring.writing.load(Ordering::Relaxed)).saturating_sub(start);
        let overwritten = (overwritten as usize).min(into.len() - first);
        into.drain(first..first + overwritten);
        written
    }
}

fn slot(frame: u64) -> usize {
    (frame % Scope::FRAMES as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::Scope;

    #[test]
    fn a_read_gives_what_came_since_the_last_and_at_most_what_the_ring_keeps() {
        let scope = Scope::new();
        let mut frames = Vec::new();
        scope.write([&[0.1, 0.2], &[-0.1, -0.2]]);
        let next = scope.read(0, &mut frames);
        assert_eq!(frames, [[0.1, -0.1], [0.2, -0.2]]);
        scope.write([&[0.3], &[-0.3]]);
        frames.clear();
        assert_eq!(scope.read(next, &mut frames), 3);
        assert_eq!(frames, [[0.3, -0.3]]);
        // A reader that fell behind gets the newest frames the ring still has.
        let long: Vec<f32> = (0..Scope::FRAMES + 10).map(|index| index as f32).collect();
        scope.write([&long, &long]);
        frames.clear();
        scope.read(3, &mut frames);
        assert_eq!(frames.len(), Scope::FRAMES);
        assert_eq!(frames.last(), Some(&[long[long.len() - 1]; 2]));
        assert_eq!(frames.first(), Some(&[10.0; 2]));
    }
}
