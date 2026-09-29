//! The gain computer and lookahead of a peak limiter, one frame at a time: the part the master
//! of the arrangement and the Limiter effect share. Next to [`Smoothed`](crate::Smoothed) and
//! [`Envelope`](crate::Envelope), a helper a processor uses per frame. The core itself limits
//! nothing.
//!
//! The gain goes down at once to what a peak needs, so nothing goes over the ceiling, and comes
//! back with the release. With a lookahead the sound is held back by that time and the gain
//! comes down along a straight line over it, so the gain is already down when the peak arrives
//! and the top of the wave keeps its shape. The lookahead is the latency of whoever uses it.
//!
//! Under the ceiling the gain is exactly 1, so the output is the input, sample for sample, one
//! lookahead later. [`PeakLimiter::limit`] clamps at the ceiling last: that catches what
//! rounding might leave over it, and what was planned for a higher ceiling while the ceiling
//! came down. With no lookahead, the rising edge of the first peak over the ceiling is flattened
//! there: that is a hard clip of the edge, and the release then turns the next peaks down whole.

use crate::CHANNELS;

/// Within this of its target the gain goes the rest of the way at once: 0.001 dB, far under
/// what anyone hears, and it makes the gain exactly 1 again after a peak.
const CLOSE_ENOUGH: f64 = 1e-4;

/// A limiter at one sample rate. [`Self::prepare`] allocates; everything else runs on the audio
/// thread. Samples that are not a number are the caller's to turn into silence first: a
/// comparison drops them, so they would pass the ceiling.
#[derive(Debug)]
pub struct PeakLimiter {
    sample_rate: f32,
    /// Frames of lookahead, which is the latency.
    lookahead: usize,
    /// The sound of the last `lookahead` frames, per channel.
    delay: [Vec<f32>; CHANNELS],
    /// The lowest gain any frame of the window asks for, oldest first: a queue of (frame,
    /// gain) in which the gains rise, so its front is the lowest in the window.
    lowest: Vec<(u64, f32)>,
    lowest_start: usize,
    lowest_len: usize,
    /// The gain after the release, over the last `lookahead` frames, and their sum. Their mean
    /// is the gain applied, which turns each drop into a straight line over the lookahead.
    smoothing: Vec<f32>,
    smoothing_sum: f64,
    /// Where the next frame goes in `delay` and in `smoothing`.
    position: usize,
    /// Frames seen, to age the queue.
    frame: u64,
    /// Frames in a row whose gain after the release was exactly 1. Once there are a lookahead of
    /// them, the mean is exactly 1 and the sum is set to what it is, so no drift of it can stay.
    unity_run: usize,
    /// In f64: near 1 a release step of an f32 would be less than half of its last bit, and the
    /// gain would stop just under 1 for good.
    envelope: f64,
    release_factor: f64,
}

impl Default for PeakLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl PeakLimiter {
    /// No room for a lookahead yet, and a release of nothing. Allocates nothing.
    pub fn new() -> Self {
        Self {
            sample_rate: 0.0,
            lookahead: 0,
            delay: [Vec::new(), Vec::new()],
            lowest: Vec::new(),
            lowest_start: 0,
            lowest_len: 0,
            smoothing: Vec::new(),
            smoothing_sum: 0.0,
            position: 0,
            frame: 0,
            unity_run: 0,
            envelope: 1.0,
            release_factor: 1.0,
        }
    }

    /// Room for a lookahead of up to `longest_lookahead_seconds` at this sample rate, silence in
    /// it and no reduction. The lookahead is none until [`Self::set_lookahead`]. Allocates: the
    /// control thread only.
    pub fn prepare(&mut self, sample_rate: f32, longest_lookahead_seconds: f32) {
        self.sample_rate = sample_rate;
        let capacity = (longest_lookahead_seconds * sample_rate).ceil() as usize + 1;
        self.delay = [vec![0.0; capacity], vec![0.0; capacity]];
        self.lowest = vec![(0, 1.0); capacity + 1];
        self.smoothing = vec![1.0; capacity];
        self.lookahead = 0;
        self.position = 0;
        self.reset();
    }

    /// How long the gain takes to come back after a peak: the time constant.
    pub fn set_release(&mut self, seconds: f32) {
        let frames = f64::from(seconds) * f64::from(self.sample_rate);
        self.release_factor = match frames <= 0.0 {
            true => 1.0,
            false => 1.0 - (-1.0 / frames).exp(),
        };
    }

    /// How far ahead to look, up to what [`Self::prepare`] made room for. Another lookahead is
    /// another latency: the engine leads the sound by the new one from this block on, so the
    /// delay starts again from silence and with no reduction.
    pub fn set_lookahead(&mut self, seconds: f32) {
        let frames = (seconds * self.sample_rate).round() as usize;
        // One frame under the room: the queue of lowest gains holds a lookahead and two more.
        let lookahead = frames.min(self.delay[0].len().saturating_sub(1));
        if lookahead != self.lookahead {
            self.lookahead = lookahead;
            self.position = 0;
            self.reset();
        }
    }

    /// The lookahead in frames: how much later a frame comes out than it went in.
    pub fn latency(&self) -> usize {
        self.lookahead
    }

    /// Whether the gain is exactly 1 and has been for a whole lookahead, so the next frame of
    /// silence changes nothing but the delay.
    pub fn is_resting(&self) -> bool {
        self.envelope == 1.0 && self.unity_run >= self.lookahead
    }

    /// Forgets what the limiter was doing: no reduction, and silence in the delay.
    pub fn reset(&mut self) {
        for channel in &mut self.delay {
            channel.fill(0.0);
        }
        let lookahead = self.lookahead;
        self.smoothing
            .iter_mut()
            .take(lookahead)
            .for_each(|gain| *gain = 1.0);
        self.smoothing_sum = lookahead as f64;
        self.lowest_start = 0;
        self.lowest_len = 0;
        self.unity_run = lookahead;
        self.envelope = 1.0;
    }

    /// One frame. `frame` goes into the delay; `peak` is the loudest channel of the frame the
    /// gain is worked out for, which is `frame` unless the caller lets something else through.
    /// Gives the frame one lookahead back and the gain that frame needs: 1, or less so that it
    /// comes out at the ceiling. [`Self::limit`] puts the two together.
    pub fn next(
        &mut self,
        peak: f32,
        frame: [f32; CHANNELS],
        ceiling: f32,
    ) -> ([f32; CHANNELS], f32) {
        let gain = self.gain_for(peak, ceiling);
        let position = self.position;
        let mut delayed = frame;
        if self.lookahead > 0 {
            for (channel, sample) in self.delay.iter_mut().zip(&mut delayed) {
                if let Some(slot) = channel.get_mut(position) {
                    *sample = std::mem::replace(slot, *sample);
                }
            }
        }
        self.position = wrap(position + 1, self.lookahead.max(1));
        self.frame += 1;
        (delayed, gain)
    }

    /// A frame at a gain, held to the ceiling.
    pub fn limit(frame: [f32; CHANNELS], gain: f32, ceiling: f32) -> [f32; CHANNELS] {
        frame.map(|sample| (sample * gain).max(-ceiling).min(ceiling))
    }

    /// One frame of the gain computer: the gain that the frame one lookahead back gets, for a
    /// frame whose loudest channel is `peak` now.
    fn gain_for(&mut self, peak: f32, ceiling: f32) -> f32 {
        let wanted = if peak > ceiling { ceiling / peak } else { 1.0 };
        let lowest = f64::from(self.lowest_with(self.frame, wanted));
        // The release: down at once, back up along the time constant.
        self.envelope = match lowest < self.envelope {
            true => lowest,
            false => {
                let next = self.envelope + (lowest - self.envelope) * self.release_factor;
                match lowest - next < CLOSE_ENOUGH || next <= self.envelope {
                    true => lowest,
                    false => next,
                }
            }
        };
        let envelope = self.envelope as f32;
        let lookahead = self.lookahead;
        if lookahead == 0 {
            return envelope;
        }
        if let Some(oldest) = self.smoothing.get_mut(self.position) {
            self.smoothing_sum += f64::from(envelope) - f64::from(*oldest);
            *oldest = envelope;
        }
        self.unity_run = match envelope == 1.0 {
            true => self.unity_run.saturating_add(1),
            false => 0,
        };
        if self.unity_run >= lookahead {
            self.smoothing_sum = lookahead as f64;
            return 1.0;
        }
        ((self.smoothing_sum / lookahead as f64) as f32).min(1.0)
    }

    /// The lowest gain the last `lookahead + 1` frames ask for, with this frame's.
    fn lowest_with(&mut self, frame: u64, gain: f32) -> f32 {
        let capacity = self.lowest.len();
        if capacity == 0 {
            return gain;
        }
        // Gains at the back that are not lower than this one can never be the lowest again.
        while self.lowest_len > 0 {
            let back = wrap(self.lowest_start + self.lowest_len - 1, capacity);
            if self
                .lowest
                .get(back)
                .is_some_and(|(_, lowest)| *lowest >= gain)
            {
                self.lowest_len -= 1;
            } else {
                break;
            }
        }
        let back = wrap(self.lowest_start + self.lowest_len, capacity);
        if let Some(entry) = self.lowest.get_mut(back) {
            *entry = (frame, gain);
            self.lowest_len += 1;
        }
        // Out of the window at the front.
        let oldest = frame.saturating_sub(self.lookahead as u64);
        while self.lowest_len > 1
            && self
                .lowest
                .get(self.lowest_start)
                .is_some_and(|(at, _)| *at < oldest)
        {
            self.lowest_start = wrap(self.lowest_start + 1, capacity);
            self.lowest_len -= 1;
        }
        self.lowest
            .get(self.lowest_start)
            .map_or(gain, |(_, lowest)| *lowest)
    }
}

/// `index % length` for an index under twice the length, without a division: a division per
/// frame was a large part of the cost of the limiter.
fn wrap(index: usize, length: usize) -> usize {
    match index >= length {
        true => index - length,
        false => index,
    }
}
