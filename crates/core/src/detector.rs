//! The level detector of a dynamics effect: the peak of the last 10 ms, and the factor of an
//! attack or release time. The Compressor and the Gate share them, so both hear a level the
//! same way. Next to [`Smoothed`](crate::Smoothed), a helper a processor uses per frame.
//!
//! A steady tone from 50 Hz up has the same peak in every stretch of 10 ms, so its level is
//! still and a gain worked out from it does not wobble with the wave.

/// The detector keeps the peak of this many stretches of 1 ms, and of the one it is in: the
/// peak of the last 10 to 11 ms. Every tone from 50 Hz up has a peak in each such stretch, so
/// its level is steady.
const STRETCHES: usize = 10;
const STRETCH_SECONDS: f32 = 0.001;

/// The peak of the last 10 to 11 ms: the largest sample of each finished stretch of 1 ms in a
/// ring, and of the stretch it is in now. Only the end of a stretch looks at the ring.
pub struct PeakDetector {
    stretches: [f32; STRETCHES],
    /// Where the next finished stretch goes in the ring.
    next: usize,
    /// The largest of the ring.
    held: f32,
    /// The stretch it is in now: its largest sample and its frames so far.
    current: f32,
    frames: usize,
    stretch_frames: usize,
}

impl PeakDetector {
    /// How long a peak stays in the level after it passed.
    pub const HOLD_SECONDS: f32 = STRETCHES as f32 * STRETCH_SECONDS;

    pub fn new(sample_rate: f32) -> Self {
        Self {
            stretches: [0.0; STRETCHES],
            next: 0,
            held: 0.0,
            current: 0.0,
            frames: 0,
            stretch_frames: ((STRETCH_SECONDS * sample_rate).round() as usize).max(1),
        }
    }

    /// Takes the next frame's peak of both channels, and gives the level now.
    #[inline]
    pub fn next(&mut self, peak: f32) -> f32 {
        self.current = self.current.max(peak);
        let level = self.held.max(self.current);
        self.frames += 1;
        if self.frames == self.stretch_frames {
            if let Some(slot) = self.stretches.get_mut(self.next) {
                *slot = self.current;
            }
            self.next = (self.next + 1) % STRETCHES;
            self.held = self.stretches.iter().copied().fold(0.0, f32::max);
            self.current = 0.0;
            self.frames = 0;
        }
        level
    }

    /// Starts a new stretch, for a detector that holds only silence. So where its stretches
    /// begin depends only on when the sound came back, not on what played before the silence.
    pub fn restart(&mut self) {
        self.current = 0.0;
        self.frames = 0;
    }

    /// The frames after which a silence has left the detector.
    pub fn window_frames(&self) -> usize {
        (STRETCHES + 1) * self.stretch_frames
    }
}

/// The one-pole factor for a time constant: after `seconds` a step is 63 % of the way. An
/// `exp`: work it out when the time changes, not per frame. In `f64`, as is what it moves: in
/// `f32` a slow pole stops short of where it goes, when the step it would take is smaller than
/// the precision of the value.
pub fn pole(seconds: f32, sample_rate: f32) -> f64 {
    (-1.0 / (f64::from(seconds) * f64::from(sample_rate))).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_level_is_the_peak_of_the_last_ten_milliseconds() {
        let mut detector = PeakDetector::new(48_000.0);
        assert_eq!(detector.next(0.5), 0.5);
        for _ in 0..480 {
            assert_eq!(detector.next(0.1), 0.5);
        }
        // After 11 ms the peak has left.
        for _ in 0..48 {
            detector.next(0.1);
        }
        assert_eq!(detector.next(0.1), 0.1);
    }
}
