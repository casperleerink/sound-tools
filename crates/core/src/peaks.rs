//! Peaks: the highest values the audio thread saw since the interface last looked, per channel.
//!
//! This is how a level leaves the audio thread. The audio side keeps the largest value with an
//! atomic maximum and the interface takes it and puts zero back, so every peak between two
//! frames of the interface is seen once, however often either side runs. No lock, no
//! allocation and no message: two atomics shared through an `Arc`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::processor::CHANNELS;

/// The largest value per channel since the last [`Peaks::take`]. Clones share the values.
///
/// A processor that shows a level keeps one and records every block; the interface gets the
/// same one through [`Project::peaks`](crate::Project::peaks) and takes from it once per frame.
/// The engine keeps one for the device output, [`EngineControl::output_peaks`](crate::EngineControl::output_peaks).
#[derive(Clone, Debug, Default)]
pub struct Peaks(Arc<[AtomicU32; CHANNELS]>);

impl Peaks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps `value` for `channel` when it is the largest since the last take. Realtime safe.
    ///
    /// For a value of 0 or more: the bits of such floats sort as the floats do, so the maximum
    /// of the bits is the maximum of the values. Anything else, and a channel the peaks do not
    /// have, is ignored, so a sample that is not a number never shows as the loudest.
    pub fn record(&self, channel: usize, value: f32) {
        let Some(peak) = self.0.get(channel) else {
            return;
        };
        if value >= 0.0 {
            peak.fetch_max(value.to_bits(), Ordering::Relaxed);
        }
    }

    /// Keeps the largest absolute sample of each channel of one block. Realtime safe.
    pub fn record_block(&self, channels: [&[f32]; CHANNELS]) {
        for (channel, samples) in channels.into_iter().enumerate() {
            let peak = samples
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            self.record(channel, peak);
        }
    }

    /// The largest value of each channel since the last take, and zero from now on.
    pub fn take(&self) -> [f32; CHANNELS] {
        self.0
            .each_ref()
            .map(|peak| f32::from_bits(peak.swap(0, Ordering::Relaxed)))
    }
}

#[cfg(test)]
mod tests {
    use super::Peaks;

    #[test]
    fn a_take_gives_the_largest_since_the_last_one_and_starts_again() {
        let peaks = Peaks::new();
        let other = peaks.clone();
        peaks.record_block([&[0.1, -0.7, 0.2], &[0.0, 0.3, -0.25]]);
        peaks.record_block([&[0.5], &[0.1]]);
        assert_eq!(other.take(), [0.7, 0.3]);
        assert_eq!(other.take(), [0.0, 0.0]);
    }

    #[test]
    fn not_a_number_and_a_wrong_channel_are_ignored() {
        let peaks = Peaks::new();
        peaks.record(0, f32::NAN);
        peaks.record(0, -1.0);
        peaks.record(5, 1.0);
        peaks.record(1, f32::INFINITY);
        assert_eq!(peaks.take(), [0.0, f32::INFINITY]);
    }
}
