//! Watch: the latest value the audio thread gave, for the interface to read. A [`Peaks`] keeps
//! the largest since the last look, which is what a meter shows; a watch keeps the last, which
//! is what a step light or a number shows. One atomic shared through an `Arc`: no lock, no
//! allocation and no message.
//!
//! [`Peaks`]: crate::Peaks

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The value last set. Clones share it. Starts at 0.
#[derive(Clone, Debug, Default)]
pub struct Watch(Arc<AtomicU32>);

impl Watch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Realtime safe.
    pub fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}
