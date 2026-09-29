//! A delay line, for a processor that hears what came in a while ago: the lines of a reverb, the
//! swept delay of a chorus. Next to [`Smoothed`](crate::Smoothed) and
//! [`Envelope`](crate::Envelope), a helper a processor uses per frame.

/// A delay line whose length is a power of two, so a position wraps with a mask and a read can
/// never be out of it. The caller keeps the position where the next frame is written, so lines
/// of one processor can share one.
pub struct DelayLine {
    buffer: Vec<f32>,
    mask: usize,
}

impl DelayLine {
    /// Holds at least `frames` frames of delay. Allocates: make it on the control thread, or in
    /// `prepare`.
    pub fn new(frames: usize) -> Self {
        let length = (frames + 1).next_power_of_two();
        Self {
            buffer: vec![0.0; length],
            mask: length - 1,
        }
    }

    /// The frames it holds: a read further back than this gives what was written after.
    pub fn frames(&self) -> usize {
        self.buffer.len()
    }

    /// What was written `delay` frames before `position`.
    pub fn read(&self, position: usize, delay: usize) -> f32 {
        // The mask keeps the index inside the buffer, whose length is the mask plus one.
        self.buffer[position.wrapping_sub(delay) & self.mask]
    }

    /// What was written `delay` frames before `position`, between two frames: a cubic through
    /// the four frames around it (Catmull-Rom). A delay that moves every frame, as a chorus
    /// sweeps it, then bends the pitch smoothly: a straight line between two frames takes a
    /// part of the highs that changes with the fraction, and that is heard as a buzz at the
    /// rate of the sweep. It is exact on a whole frame.
    ///
    /// It reads one frame newer than the delay, so a delay under 2 frames reads 2: the frame at
    /// `position` is not written yet.
    pub fn read_between(&self, position: usize, delay: f32) -> f32 {
        let delay = delay.max(2.0);
        let whole = delay as usize;
        let fraction = delay - whole as f32;
        let [newer, at, older, oldest] =
            [whole - 1, whole, whole + 1, whole + 2].map(|delay| self.read(position, delay));
        let slope_at = 0.5 * (older - newer);
        let slope_older = 0.5 * (oldest - at);
        let difference = older - at;
        let c2 = 3.0 * difference - 2.0 * slope_at - slope_older;
        let c3 = slope_at + slope_older - 2.0 * difference;
        ((c3 * fraction + c2) * fraction + slope_at) * fraction + at
    }

    pub fn write(&mut self, position: usize, value: f32) {
        self.buffer[position & self.mask] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::DelayLine;

    fn line_of(samples: impl IntoIterator<Item = f32>) -> (DelayLine, usize) {
        let mut line = DelayLine::new(64);
        let mut position = 0;
        for sample in samples {
            line.write(position, sample);
            position += 1;
        }
        (line, position)
    }

    #[test]
    fn a_whole_delay_reads_the_frame_itself() {
        let (line, position) = line_of((0..20).map(|index| (index * index) as f32));
        for delay in 2..16 {
            let expected = line.read(position, delay);
            assert_eq!(line.read_between(position, delay as f32), expected);
        }
    }

    /// A cubic through four points is exact on anything of degree two or less, and a line
    /// between two frames is not.
    #[test]
    fn between_two_frames_it_follows_a_curve() {
        let curve = |time: f32| 0.5 * time * time - 3.0 * time + 1.0;
        let (line, position) = line_of((0..20).map(|index| curve(index as f32)));
        for delay in [2.25, 3.5, 7.75, 10.1] {
            let time = position as f32 - delay;
            let read = line.read_between(position, delay);
            assert!((read - curve(time)).abs() < 1e-3, "{delay}: {read}");
        }
    }

    #[test]
    fn a_delay_under_two_frames_reads_two() {
        let (line, position) = line_of((0..8).map(|index| index as f32));
        assert_eq!(line.read_between(position, 0.0), line.read(position, 2));
        assert_eq!(line.read_between(position, 1.5), line.read(position, 2));
    }

    #[test]
    fn a_line_holds_a_power_of_two_past_its_frames() {
        assert_eq!(DelayLine::new(1).frames(), 2);
        assert_eq!(DelayLine::new(1000).frames(), 1024);
        assert_eq!(DelayLine::new(1024).frames(), 2048);
    }
}
