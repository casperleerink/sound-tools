//! A short-time Fourier transform, for an effect that works on the spectrum of a sound: it
//! takes one sample in and gives one out, and every hop hands the spectrum of the last
//! `size` samples to the caller to change, in magnitude and phase. One made
//! [`with_side`](Stft::with_side) also hands over the spectrum of a second input, such as the
//! voice of a vocoder.
//!
//! A Hann window of `size` samples, moved on by a quarter of it, both before the transform and
//! after the inverse: the windows then add up to the same in every sample, so a spectrum left
//! as it is gives back the input, [`Stft::latency`] samples late.

use std::f32::consts::TAU;
use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// One bin of a spectrum. A sine of amplitude 1 at the frequency of a bin has a magnitude of 1
/// there.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SpectralBin {
    pub magnitude: f32,
    /// In radians, from -π to π as it comes in; any value as it goes out.
    pub phase: f32,
}

#[derive(Clone)]
pub struct Stft {
    hop: usize,
    window: Box<[f32]>,
    /// The window after the inverse, with the scale that gives the input back.
    synthesis: Box<[f32]>,
    /// The last `size` samples in, a ring that `write` goes round.
    input: Box<[f32]>,
    /// The same of the side input, empty without one.
    side: Box<[f32]>,
    side_bins: Box<[SpectralBin]>,
    write: usize,
    /// Samples in since the last hop.
    count: usize,
    /// What the frames so far add up to, from the next sample out on.
    output: Box<[f32]>,
    read: usize,
    frame: Box<[f32]>,
    spectrum: Box<[Complex<f32>]>,
    bins: Box<[SpectralBin]>,
    scratch: Box<[Complex<f32>]>,
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
}

impl Stft {
    /// `size` is a power of two of at least 4. Allocates: make it on the control thread.
    pub fn new(size: usize) -> Self {
        Self::make(size, false)
    }

    /// As [`Stft::new`], with a side input for [`Stft::next_with_side`].
    pub fn with_side(size: usize) -> Self {
        Self::make(size, true)
    }

    fn make(size: usize, side: bool) -> Self {
        let size = size.max(4).next_power_of_two();
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let window: Box<[f32]> = (0..size)
            .map(|index| 0.5 - 0.5 * (TAU * index as f32 / size as f32).cos())
            .collect();
        // A magnitude is the sum times 2 over the sum of the window, `size / 2`. The inverse
        // gives `size` times the input, and four windows squared add up to 1.5.
        let synthesis = window.iter().map(|window| window / 6.0).collect();
        let scratch = forward.get_scratch_len().max(inverse.get_scratch_len());
        Self {
            hop: size / 4,
            synthesis,
            input: vec![0.0; size].into(),
            side: vec![0.0; if side { size } else { 0 }].into(),
            side_bins: vec![SpectralBin::default(); if side { size / 2 + 1 } else { 0 }].into(),
            write: 0,
            count: 0,
            output: vec![0.0; size].into(),
            read: 0,
            frame: vec![0.0; size].into(),
            spectrum: forward.make_output_vec().into(),
            bins: vec![SpectralBin::default(); size / 2 + 1].into(),
            scratch: vec![Complex::default(); scratch].into(),
            forward,
            inverse,
            window,
        }
    }

    /// How many samples late a sample comes out.
    pub fn latency(&self) -> usize {
        Self::latency_of(self.window.len())
    }

    /// The latency of an `Stft` of `size`.
    pub fn latency_of(size: usize) -> usize {
        size.saturating_sub(1)
    }

    /// How many bins a spectrum has: from 0 Hz to half the sample rate.
    pub fn bins(&self) -> usize {
        self.bins.len()
    }

    /// Samples between two spectra.
    pub fn hop(&self) -> usize {
        self.hop
    }

    /// Takes `sample` in and gives one out. On the last sample of a hop, `change` changes the
    /// spectrum of the last `size` samples first.
    #[inline]
    pub fn next(&mut self, sample: f32, change: impl FnOnce(&mut [SpectralBin])) -> f32 {
        self.next_with_side(sample, 0.0, |bins, _| change(bins))
    }

    /// As [`Stft::next`], with `side` into the side input: `change` also gets the spectrum of
    /// the side input, which is empty in an `Stft` made without one.
    #[inline]
    pub fn next_with_side(
        &mut self,
        sample: f32,
        side: f32,
        change: impl FnOnce(&mut [SpectralBin], &[SpectralBin]),
    ) -> f32 {
        let mask = self.input.len() - 1;
        if let Some(slot) = self.input.get_mut(self.write) {
            *slot = sample;
        }
        if let Some(slot) = self.side.get_mut(self.write) {
            *slot = side;
        }
        self.write = (self.write + 1) & mask;
        self.count += 1;
        if self.count == self.hop {
            self.count = 0;
            self.read = 0;
            self.transform(change);
        }
        let out = self.output.get(self.read).copied().unwrap_or(0.0);
        self.read += 1;
        out
    }

    fn transform(&mut self, change: impl FnOnce(&mut [SpectralBin], &[SpectralBin])) {
        let size = self.input.len();
        if !self.side.is_empty() && !self.analyse(Ring::Side) {
            return;
        }
        if !self.analyse(Ring::Input) {
            return;
        }
        change(&mut self.bins, &self.side_bins);
        for (complex, bin) in self.spectrum.iter_mut().zip(self.bins.iter()) {
            *complex = Complex::from_polar(finite(bin.magnitude), finite(bin.phase));
        }
        // The first and the last bin of a real signal have no imaginary part.
        if let Some(first) = self.spectrum.first_mut() {
            first.im = 0.0;
        }
        if let Some(last) = self.spectrum.last_mut() {
            last.im = 0.0;
        }
        let inverse = (self.inverse).process_with_scratch(
            &mut self.spectrum,
            &mut self.frame,
            &mut self.scratch,
        );
        if inverse.is_err() {
            return;
        }
        self.output.copy_within(self.hop.., 0);
        if let Some(rest) = self.output.get_mut(size - self.hop..) {
            rest.fill(0.0);
        }
        let added = self.frame.iter().zip(self.synthesis.iter());
        for (output, (frame, window)) in self.output.iter_mut().zip(added) {
            *output += finite(frame * window);
        }
    }

    /// Puts the spectrum of the last `size` samples of `ring` in its bins. False when the
    /// transform fails, which it does only for buffers of the wrong length.
    fn analyse(&mut self, ring: Ring) -> bool {
        let (input, bins) = match ring {
            Ring::Input => (&self.input, &mut self.bins),
            Ring::Side => (&self.side, &mut self.side_bins),
        };
        let oldest = input.iter().cycle().skip(self.write);
        for ((frame, input), window) in self.frame.iter_mut().zip(oldest).zip(&self.window) {
            *frame = input * window;
        }
        let forward = (self.forward).process_with_scratch(
            &mut self.frame,
            &mut self.spectrum,
            &mut self.scratch,
        );
        if forward.is_err() {
            return false;
        }
        let scale = 4.0 / input.len() as f32;
        for (bin, complex) in bins.iter_mut().zip(self.spectrum.iter()) {
            let (magnitude, phase) = complex.to_polar();
            *bin = SpectralBin {
                magnitude: magnitude * scale,
                phase,
            };
        }
        true
    }
}

#[derive(Copy, Clone)]
enum Ring {
    Input,
    Side,
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::Stft;

    #[test]
    fn a_spectrum_left_as_it_is_gives_the_input_back_its_latency_late() {
        let mut stft = Stft::new(256);
        let input = |frame: usize| (TAU * 440.0 * frame as f32 / 48_000.0).sin() * 0.5;
        let output: Vec<f32> = (0..4096)
            .map(|frame| stft.next(input(frame), |_| {}))
            .collect();
        let latency = stft.latency();
        assert_eq!(latency, 255);
        for (frame, sample) in output.iter().enumerate().skip(latency) {
            let expected = input(frame - latency);
            assert!(
                (sample - expected).abs() < 1e-5,
                "frame {frame}: {sample} for {expected}"
            );
        }
    }

    #[test]
    fn a_sine_at_a_bin_has_its_amplitude_as_its_magnitude() {
        let mut stft = Stft::new(1024);
        // Bin 32 of 1024 at 48 kHz.
        let hz = 32.0 * 48_000.0 / 1024.0;
        let mut magnitudes = Vec::new();
        for frame in 0..2048 {
            let sample = 0.25 * (TAU * hz * frame as f32 / 48_000.0).sin();
            stft.next(sample, |bins| {
                magnitudes = bins.iter().map(|bin| bin.magnitude).collect()
            });
        }
        assert!((magnitudes[32] - 0.25).abs() < 1e-4, "{}", magnitudes[32]);
        assert!(magnitudes[40] < 1e-4, "{}", magnitudes[40]);
    }

    #[test]
    fn the_side_spectrum_is_of_the_side_input_and_shapes_the_input() {
        let mut stft = Stft::with_side(1024);
        let hz = |bin: f32| bin * 48_000.0 / 1024.0;
        let sine = |hz: f32, frame: usize| (TAU * hz * frame as f32 / 48_000.0).sin();
        let mut output = Vec::new();
        for frame in 0..4096 {
            let input = sine(hz(32.0), frame) + sine(hz(64.0), frame);
            let side = 0.5 * sine(hz(64.0), frame);
            output.push(stft.next_with_side(input, side, |bins, side| {
                for (bin, side) in bins.iter_mut().zip(side) {
                    if side.magnitude < 0.1 {
                        bin.magnitude = 0.0;
                    }
                }
            }));
        }
        // Only the sine the side has is left.
        let late = stft.latency();
        for (frame, sample) in output.iter().enumerate().skip(2048) {
            let expected = sine(hz(64.0), frame - late);
            assert!((sample - expected).abs() < 1e-3, "frame {frame}: {sample} for {expected}");
        }
    }
}
