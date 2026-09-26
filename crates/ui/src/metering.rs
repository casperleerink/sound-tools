//! Feeding a meter from the audio thread: what a view keeps for each meter it shows.
//!
//! A processor records its [`Peaks`] every block. The view takes them once per poll of the
//! session, gives the [`Ballistics`] one reading and draws again only when what the meter shows
//! changed. So a meter at rest costs no frame, and one that falls after the sound draws until it
//! is at rest.

use gpui::{Context, Task};
use sound_core::Peaks;

use crate::components::meter::{Ballistics, Level};
use crate::session::POLL_INTERVAL;

/// The meter of one level: its ballistics and what it shows now.
#[derive(Debug, Default)]
pub struct Metering {
    ballistics: Ballistics,
    level: Level,
}

impl Metering {
    /// What the meter shows.
    pub fn level(&self) -> Level {
        self.level
    }

    /// One reading, one poll after the last: the peaks since then, or silence when there are
    /// none, as for an instance that is gone. Whether what the meter shows changed.
    pub fn read(&mut self, peaks: Option<&Peaks>) -> bool {
        self.read_amplitudes(peaks.map_or([0.0; 2], Peaks::take))
    }

    /// The same, for peaks that were taken already because something else shows them too.
    pub fn read_amplitudes(&mut self, amplitudes: [f32; 2]) -> bool {
        let seconds = POLL_INTERVAL.as_secs_f32();
        let level = self.ballistics.read(amplitudes.map(decibels), seconds);
        let changed = level != self.level;
        self.level = level;
        changed
    }

    /// The composer clicked the clip light.
    pub fn clear_clip(&mut self) {
        self.ballistics.clear_clip();
        self.level.clipped = false;
    }

    /// At rest, for a meter that shows another level now, such as the panel of another track.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// An amplitude in dBFS: 1 is 0 dB and 0 is `-inf`.
pub fn decibels(amplitude: f32) -> f32 {
    20.0 * amplitude.log10()
}

/// Runs `tick` on the view once per poll of the session, for as long as the view lives: the
/// clock of the meters of a view. Keep the task in the view.
pub fn every_poll<V: 'static>(
    cx: &mut Context<V>,
    tick: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Task<()> {
    cx.spawn(async move |view, cx| {
        loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            if view.update(cx, |view, cx| tick(view, cx)).is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use sound_core::Peaks;

    use super::{Metering, decibels};

    #[test]
    fn a_reading_takes_the_peaks_and_shows_them_in_decibels() {
        assert_eq!(decibels(1.0), 0.0);
        assert_eq!(decibels(0.0), f32::NEG_INFINITY);
        let peaks = Peaks::new();
        peaks.record_block([&[0.5], &[1.0]]);
        let mut metering = Metering::default();
        assert!(metering.read(Some(&peaks)));
        let level = metering.level();
        assert!((level.now[0] + 6.0206).abs() < 1e-3, "{:?}", level.now);
        assert_eq!(level.now[1], 0.0);
        // Taken: the next reading starts from nothing and the bars fall.
        assert!(metering.read(Some(&peaks)));
        assert!(metering.level().now[1] < 0.0);
        // A meter at rest does not change, and asks for no frame.
        let mut quiet = Metering::default();
        assert!(!quiet.read(None));
    }
}
