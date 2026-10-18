//! Scope: the last frames of a sound, for the interface to look at. [`Peaks`] give a level; a
//! scope gives the sound itself, for what the interface works out of it, such as a spectrum,
//! so that work stays off the audio thread. The audio thread writes each sample into a ring of
//! atomics and counts the frames; the interface copies what came since it last looked. No
//! lock, no allocation and no message.
//!
//! [`Peaks`]: crate::Peaks

use std::sync::Arc;

use crate::processor::CHANNELS;
use crate::ring::Ring;

/// The last [`Scope::FRAMES`] frames a processor wrote. Clones share them. One writer.
#[derive(Clone, Debug)]
pub struct Scope(Arc<Ring<1>>);

impl Default for Scope {
    fn default() -> Self {
        Self(Arc::new(Ring::new(Self::FRAMES)))
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
        let [left, right] = channels;
        let frames = left.iter().zip(right);
        let both = |(left, right): (&f32, &f32)| {
            u64::from(left.to_bits()) << 32 | u64::from(right.to_bits())
        };
        self.0.write(frames.map(|frame| [both(frame)]));
    }

    /// Appends to `into` the frames written since `from`, a count [`Self::read`] gave before, 0,
    /// or `u64::MAX` for none, oldest first. Gives the count to read from next time. Frames
    /// written over before they were read are left out.
    pub fn read(&self, from: u64, into: &mut Vec<[f32; CHANNELS]>) -> u64 {
        let channels = |bits: u64| [(bits >> 32) as u32, bits as u32].map(f32::from_bits);
        self.0.read(from, into, |[bits]| channels(bits))
    }
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
