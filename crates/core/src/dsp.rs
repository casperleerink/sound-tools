//! Small building blocks of the built-in effects that more than one of them needs: a read of a
//! [`DelayLine`] that fades from one length to another, a one-pole filter and the hold on what
//! goes into a loop. Next to [`Smoothed`](crate::Smoothed) and [`DelayLine`]. The core itself
//! uses none of them.

use std::f32::consts::PI;

use crate::DelayLine;

/// A cutoff stays under this part of the sample rate, below the Nyquist frequency.
const HIGHEST_PART: f32 = 0.45;

/// Input louder than this, or not a number, is held to it, so no sample of anyone else's can
/// make a loop infinite. +36 dBFS: nothing real comes near it.
const INPUT_LIMIT: f32 = 64.0;

/// A sample of the input as a loop takes it: held to +36 dBFS, and silence for anything that
/// is not a number.
#[inline]
pub fn held(sample: f32) -> f32 {
    if sample.is_nan() {
        return 0.0;
    }
    sample.clamp(-INPUT_LIMIT, INPUT_LIMIT)
}

/// Where a group of delay lines is read. A new length does not move a read position: the read
/// fades from the old tap to the new one. A length that comes during a fade waits for its end,
/// so a drag through many lengths is a row of fades, each from where the last one ended.
#[derive(Clone, Copy)]
pub struct Taps<const N: usize> {
    from: [usize; N],
    to: [usize; N],
    next: [usize; N],
    /// From 0 at `from` to 1 at `to`.
    fade: f32,
    step: f32,
}

impl<const N: usize> Taps<N> {
    pub fn new(taps: [usize; N]) -> Self {
        Self {
            from: taps,
            to: taps,
            next: taps,
            fade: 1.0,
            step: 1.0,
        }
    }

    /// Aims at other lengths, reached in a fade of `fade_frames` frames, at least one.
    pub fn aim(&mut self, next: [usize; N], fade_frames: f32) {
        self.next = next;
        // `max` first: it gives 1 for a length that is not a number.
        self.step = 1.0 / fade_frames.max(1.0);
        if !self.is_fading() {
            self.start();
        }
    }

    fn start(&mut self) {
        if self.next != self.to {
            self.from = self.to;
            self.to = self.next;
            self.fade = 0.0;
        }
    }

    pub fn is_fading(&self) -> bool {
        self.from != self.to
    }

    /// The part of the new tap in this frame.
    #[inline]
    pub fn weight(&self) -> f32 {
        if self.is_fading() { self.fade } else { 1.0 }
    }

    /// One frame along, after every read of the frame: a fade that ends here may start the
    /// next one, between other taps.
    #[inline]
    pub fn advance(&mut self) {
        if !self.is_fading() {
            return;
        }
        self.fade += self.step;
        if self.fade >= 1.0 {
            self.from = self.to;
            self.fade = 1.0;
            self.start();
        }
    }

    /// Takes the newest taps at once.
    pub fn snap(&mut self) {
        *self = Self::new(self.next);
    }

    /// The length of tap `index` now, between the two it fades between. 0 for a tap that
    /// does not exist.
    pub fn length(&self, index: usize) -> f32 {
        let (Some(&from), Some(&to)) = (self.from.get(index), self.to.get(index)) else {
            return 0.0;
        };
        let (from, to) = (from as f32, to as f32);
        from + (to - from) * self.fade
    }

    /// Reads tap `index` of `line` at `position`, with `fade` of the new tap. Silence for a
    /// tap that does not exist.
    #[inline]
    pub fn read(&self, line: &DelayLine, index: usize, position: usize, fade: f32) -> f32 {
        let (Some(&from), Some(&to)) = (self.from.get(index), self.to.get(index)) else {
            return 0.0;
        };
        let to = line.read(position, to);
        if fade >= 1.0 {
            return to;
        }
        let from = line.read(position, from);
        from + (to - from) * fade
    }
}

/// A one-pole filter in its trapezoidal form: stable at every cutoff, also while it moves, and
/// never louder than what goes in, at any frequency.
#[derive(Clone, Copy, Default)]
pub struct OnePole {
    memory: f32,
}

impl OnePole {
    /// The factor of a cutoff: `g / (1 + g)` with `g = tan(π cutoff / sample rate)`. The cutoff
    /// stays between 1 Hz and 0.45 of the sample rate. A `tan`: work it out when the cutoff
    /// moves, not per frame.
    pub fn factor(hz: f32, sample_rate: f32) -> f32 {
        let hz = hz.max(1.0).min(HIGHEST_PART * sample_rate);
        let g = (PI * hz / sample_rate).tan();
        g / (1.0 + g)
    }

    /// The low pass of one sample.
    #[inline]
    pub fn low(&mut self, factor: f32, input: f32) -> f32 {
        let step = (input - self.memory) * factor;
        let low = step + self.memory;
        self.memory = low + step;
        low
    }

    /// The high pass of one sample.
    #[inline]
    pub fn high(&mut self, factor: f32, input: f32) -> f32 {
        input - self.low(factor, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fade_of_no_frames_or_not_a_number_ends_after_one_frame() {
        for fade_frames in [0.0, -5.0, f32::NAN] {
            let mut taps = Taps::new([10]);
            taps.aim([20], fade_frames);
            assert!(taps.is_fading());
            taps.advance();
            assert!(!taps.is_fading(), "{fade_frames}");
            assert_eq!(taps.length(0), 20.0);
        }
    }

    #[test]
    fn a_tap_that_does_not_exist_is_silent() {
        let taps = Taps::new([1]);
        let line = DelayLine::new(8);
        assert_eq!(taps.length(1), 0.0);
        assert_eq!(taps.read(&line, 1, 0, 0.5), 0.0);
    }
}
