//! The wavetables: frames of one cycle each, the built-in tables made in code, and the levels a
//! voice reads so that no pitch aliases.
//!
//! A table is built on the control side, when a record picks it, and reaches the processor as
//! an `Arc`. Each frame is kept at eleven levels, one per octave: all its harmonics, then half
//! of them, down to the fundamental alone. A voice reads the fullest level whose highest
//! harmonic stays under half the sample rate at the pitch it plays, so nothing folds back. The
//! levels are made with an FFT: the harmonics above a level's limit are set to zero, which is
//! exact, where a filter would only lower them.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, OnceLock};

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use serde::{Deserialize, Serialize};

use crate::dsp::fold;

/// The samples of one frame at full resolution: one cycle.
pub const FRAME_LENGTH: usize = 2048;

/// The most frames a table has.
const MAX_FRAMES: usize = 256;

/// The harmonics a frame holds at full resolution: all under half its samples.
const HARMONICS: usize = FRAME_LENGTH / 2 - 1;

/// One level per octave, from every harmonic down to the fundamental alone.
const LEVELS: usize = 11;

/// A level keeps at least this many samples per cycle of its highest harmonic, up to the full
/// frame, so that reading between two samples on a straight line stays close to the curve.
const SAMPLES_PER_TOP_CYCLE: usize = 64;

/// Where a built-in table is finely sampled to find its spectrum: this many times the samples
/// of a frame, so what a sharp shape has above the harmonics of a frame folds back into them
/// only 90 dB down.
const SHAPE_OVERSAMPLING: usize = 16;

/// The highest harmonic of a level.
fn top_harmonic(level: usize) -> usize {
    if level == 0 {
        HARMONICS
    } else {
        (FRAME_LENGTH / 2) >> level
    }
}

/// The samples per frame of a level.
fn level_length(level: usize) -> usize {
    (top_harmonic(level) * SAMPLES_PER_TOP_CYCLE)
        .next_power_of_two()
        .min(FRAME_LENGTH)
}

/// One level of every frame, one after the other. A frame is `length + 1` samples: the last is
/// a copy of the first, so a read between two samples never wraps.
struct Level {
    length: usize,
    samples: Box<[f32]>,
}

/// A table: up to [`MAX_FRAMES`] frames of one cycle each, at every level. Every frame peaks
/// at 1 at full resolution and has no DC.
pub struct Wavetable {
    frames: usize,
    levels: Box<[Level]>,
}

/// One level of a table, as a voice reads it.
#[derive(Copy, Clone)]
pub(crate) struct LevelView<'a> {
    /// Frame after frame, each `length + 1` samples.
    pub samples: &'a [f32],
    pub length: usize,
    pub frames: usize,
}

impl Wavetable {
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Frame `index` at full resolution, one cycle of [`FRAME_LENGTH`] samples: what a view
    /// draws. An index past the last frame gives the last frame.
    pub fn frame(&self, index: usize) -> &[f32] {
        let index = index.min(self.frames - 1);
        let stride = FRAME_LENGTH + 1;
        &self.levels[0].samples[index * stride..index * stride + FRAME_LENGTH]
    }

    /// The level to read at a phase step, in cycles per frame: the fullest one whose highest
    /// harmonic stays at or under half the sample rate.
    pub(crate) fn level_for(phase_step: f32) -> usize {
        // Level `l` holds harmonics up to 1024 / 2^l, so it fits while 1024 / 2^l ≤ 0.5 / step,
        // which is 2^l ≥ 2048 step.
        let level = (FRAME_LENGTH as f32 * phase_step).log2().ceil();
        // Not `clamp`: a NaN comes out as 0, never as a panic.
        level.max(0.0).min((LEVELS - 1) as f32) as usize
    }

    pub(crate) fn level(&self, level: usize) -> LevelView<'_> {
        let level = &self.levels[level.min(LEVELS - 1)];
        LevelView {
            samples: &level.samples,
            length: level.length,
            frames: self.frames,
        }
    }

    /// A table from the spectrum of each frame: the complex amplitude of each harmonic, at its
    /// index, as a real FFT of [`FRAME_LENGTH`] samples gives it. The DC and what is above
    /// [`HARMONICS`] are left out. Each frame is scaled to peak at 1.
    fn from_spectra(spectra: &[Spectrum]) -> Result<Self, String> {
        let spectra = &spectra[..spectra.len().min(MAX_FRAMES)];
        if spectra.is_empty() {
            return Err("a wavetable needs at least one frame".into());
        }
        let mut planner = RealFftPlanner::<f32>::new();
        let mut inverse = |level: usize, spectrum: &Spectrum| -> Result<Vec<f32>, String> {
            let transform = planner.plan_fft_inverse(level_length(level));
            let mut input = transform.make_input_vec();
            let top = top_harmonic(level);
            input[1..=top].copy_from_slice(&spectrum[1..=top]);
            let mut output = transform.make_output_vec();
            transform
                .process(&mut input, &mut output)
                .map_err(|error| error.to_string())?;
            Ok(output)
        };
        // Every level of a frame is the same sum of harmonics, only with fewer of them, so the
        // scale that makes the full level peak at 1 is the scale of all its levels.
        let mut scales = Vec::with_capacity(spectra.len());
        for spectrum in spectra {
            let full = inverse(0, spectrum)?;
            let peak = full
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
            scales.push(if peak > 0.0 { 1.0 / peak } else { 1.0 });
        }
        let mut levels = Vec::with_capacity(LEVELS);
        for level in 0..LEVELS {
            let length = level_length(level);
            let mut samples = Vec::with_capacity(spectra.len() * (length + 1));
            for (spectrum, scale) in spectra.iter().zip(&scales) {
                let frame = inverse(level, spectrum)?;
                samples.extend(frame.iter().map(|sample| sample * scale));
                samples.push(frame[0] * scale);
            }
            levels.push(Level {
                length,
                samples: samples.into_boxed_slice(),
            });
        }
        Ok(Self {
            frames: spectra.len(),
            levels: levels.into_boxed_slice(),
        })
    }
}

/// What a group of built-in tables has in common, for a picker to sort them by.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Category {
    /// The shapes of an analog synth.
    Basic,
    /// Sums of sine waves.
    Additive,
    /// Voices.
    Vocal,
    /// A spectrum that moves, such as a filter sweep.
    Spectral,
    /// Other kinds of synthesis caught one cycle at a time: sync, FM, folding.
    Synthesis,
    /// The sound of few bits.
    Digital,
}

/// A built-in table. Each is made in code, so nothing is loaded and every machine plays the
/// same. Saved in snake case, `"basic_shapes"`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Table {
    /// Sine, triangle, saw at the middle, then on to a square.
    BasicShapes,
    /// A square that narrows to a thin pulse.
    PulseWidth,
    /// A sine, then the harmonics of a saw added one by one.
    Harmonics,
    /// Drawbar settings of a tonewheel organ, from one flute to all drawbars out.
    Organ,
    /// The vowels a, e, i, o, u, one into the next.
    Vowels,
    /// A saw through a resonant low-pass filter that opens.
    ResonantSweep,
    /// A saw hard-synced to one that is up to three octaves higher.
    SyncSweep,
    /// A sine with another sine of the same pitch bending its phase more and more.
    FmSweep,
    /// A sine driven harder and harder into a wavefolder.
    FoldSweep,
    /// A sine at fewer and fewer levels, down to three steps.
    Digital,
}

impl Table {
    pub const ALL: [Self; 10] = [
        Self::BasicShapes,
        Self::PulseWidth,
        Self::Harmonics,
        Self::Organ,
        Self::Vowels,
        Self::ResonantSweep,
        Self::SyncSweep,
        Self::FmSweep,
        Self::FoldSweep,
        Self::Digital,
    ];

    /// The name a composer sees.
    pub fn name(self) -> &'static str {
        match self {
            Self::BasicShapes => "Basic Shapes",
            Self::PulseWidth => "Pulse Width",
            Self::Harmonics => "Harmonics",
            Self::Organ => "Organ",
            Self::Vowels => "Vowels",
            Self::ResonantSweep => "Resonant Sweep",
            Self::SyncSweep => "Sync Sweep",
            Self::FmSweep => "FM Sweep",
            Self::FoldSweep => "Fold Sweep",
            Self::Digital => "Digital",
        }
    }

    pub fn category(self) -> Category {
        match self {
            Self::BasicShapes | Self::PulseWidth => Category::Basic,
            Self::Harmonics | Self::Organ => Category::Additive,
            Self::Vowels => Category::Vocal,
            Self::ResonantSweep => Category::Spectral,
            Self::SyncSweep | Self::FmSweep | Self::FoldSweep => Category::Synthesis,
            Self::Digital => Category::Digital,
        }
    }

    /// The spectrum of every frame.
    fn spectra(self) -> Result<Vec<Spectrum>, String> {
        Ok(match self {
            Self::BasicShapes => basic_shapes(),
            Self::PulseWidth => sweep(32, pulse),
            Self::Harmonics => sweep(32, |at| {
                let count = 1.0 + at * 31.0;
                additive(|harmonic| {
                    if harmonic as f32 <= count + 0.5 {
                        1.0 / harmonic as f32
                    } else {
                        0.0
                    }
                })
            }),
            Self::Organ => organ(),
            Self::Vowels => vowels(),
            Self::ResonantSweep => sweep(32, resonant),
            Self::SyncSweep => shapes(32, |at, phase| {
                let ratio = 8_f32.powf(at);
                1.0 - 2.0 * (phase * ratio).fract()
            })?,
            Self::FmSweep => shapes(32, |at, phase| {
                let index = 6.0 * at;
                (TAU * phase + index * (TAU * phase).sin()).sin()
            })?,
            Self::FoldSweep => {
                shapes(32, |at, phase| fold((TAU * phase).sin() * (1.0 + 7.0 * at)))?
            }
            Self::Digital => shapes(16, |at, phase| {
                let steps = 64_f32.powf(1.0 - at);
                ((TAU * phase).sin() * steps).round() / steps
            })?,
        })
    }
}

/// A built-in table, made the first time something asks for it and shared from then on.
pub fn wavetable(table: Table) -> Result<Arc<Wavetable>, String> {
    static BUILT: [OnceLock<Result<Arc<Wavetable>, String>>; Table::ALL.len()] =
        [const { OnceLock::new() }; Table::ALL.len()];
    BUILT[table as usize]
        .get_or_init(|| {
            let spectra = table.spectra()?;
            Wavetable::from_spectra(&spectra).map(Arc::new)
        })
        .clone()
}

/// The complex amplitude of each harmonic at its index, as a real FFT of [`FRAME_LENGTH`]
/// samples gives it, from the DC to half the samples.
type Spectrum = Vec<Complex<f32>>;

/// Sine waves: harmonic `h` at `amplitude(h)`, each starting at 0 and rising. Every frame made
/// of these starts its cycle in the same place, so a morph from one to the next never cancels
/// its fundamental.
fn additive(amplitude: impl Fn(usize) -> f32) -> Spectrum {
    let mut spectrum = vec![Complex::default(); FRAME_LENGTH / 2 + 1];
    for (harmonic, bin) in spectrum.iter_mut().enumerate().take(HARMONICS + 1).skip(1) {
        // A sine is `-i` in the spectrum.
        *bin = Complex::new(0.0, -amplitude(harmonic));
    }
    spectrum
}

/// `count` frames, from `frame(0)` to `frame(1)`.
fn sweep(count: usize, frame: impl Fn(f32) -> Spectrum) -> Vec<Spectrum> {
    (0..count)
        .map(|index| frame(index as f32 / (count - 1) as f32))
        .collect()
}

/// `count` frames of a shape given by its value at a phase from 0 to 1, from `shape(0, _)` to
/// `shape(1, _)`. Each is sampled finely and its spectrum found with an FFT.
fn shapes(count: usize, shape: impl Fn(f32, f32) -> f32) -> Result<Vec<Spectrum>, String> {
    let length = FRAME_LENGTH * SHAPE_OVERSAMPLING;
    let transform = RealFftPlanner::<f32>::new().plan_fft_forward(length);
    let mut samples = transform.make_input_vec();
    let mut spectrum = transform.make_output_vec();
    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let at = index as f32 / (count - 1) as f32;
        for (sample_index, sample) in samples.iter_mut().enumerate() {
            *sample = shape(at, sample_index as f32 / length as f32);
        }
        transform
            .process(&mut samples, &mut spectrum)
            .map_err(|error| error.to_string())?;
        frames.push(spectrum[..=FRAME_LENGTH / 2].to_vec());
    }
    Ok(frames)
}

fn basic_shapes() -> Vec<Spectrum> {
    let sine = additive(|harmonic| if harmonic == 1 { 1.0 } else { 0.0 });
    let odd = |harmonic: usize| harmonic % 2 == 1;
    let triangle = additive(|harmonic| {
        let sign = if (harmonic / 2) % 2 == 0 { 1.0 } else { -1.0 };
        if odd(harmonic) {
            sign / (harmonic * harmonic) as f32
        } else {
            0.0
        }
    });
    let saw = additive(|harmonic| 1.0 / harmonic as f32);
    let square = additive(|harmonic| {
        if odd(harmonic) {
            1.0 / harmonic as f32
        } else {
            0.0
        }
    });
    // The saw sits at the middle of the table, the default position: a frame half way to the
    // square makes the morph from the saw to the square take the second half.
    let between = saw
        .iter()
        .zip(&square)
        .map(|(saw, square)| (saw + square) * 0.5)
        .collect();
    vec![sine, triangle, saw, between, square]
}

/// A pulse that is up for `at` of the way from half of its cycle to 3 %, exact: the spectrum
/// of a pulse of width `w` is `sin(π h w) / h`, delayed by half the width.
fn pulse(at: f32) -> Spectrum {
    let width = 0.5 - 0.47 * at;
    let mut spectrum = additive(|_| 0.0);
    for (harmonic, bin) in spectrum.iter_mut().enumerate().take(HARMONICS + 1).skip(1) {
        let angle = PI * harmonic as f32 * width;
        *bin = Complex::from_polar(angle.sin() / harmonic as f32, -angle);
    }
    spectrum
}

/// Drawbar settings from 0 to 8 for the 8', 4', 2⅔', 2', 1⅗', 1⅓' and 1' drawbars of a
/// tonewheel organ, which play these harmonics. The 16' and 5⅓' drawbars are under the
/// fundamental, which a table cannot hold, so they are left out.
const DRAWBAR_HARMONICS: [usize; 7] = [1, 2, 3, 4, 5, 6, 8];
const REGISTRATIONS: [[u8; 7]; 8] = [
    [8, 0, 0, 0, 0, 0, 0],
    [8, 4, 0, 0, 0, 0, 0],
    [8, 8, 4, 0, 0, 0, 0],
    [8, 8, 8, 0, 0, 0, 0],
    [8, 8, 8, 6, 0, 0, 0],
    [8, 8, 8, 8, 4, 0, 0],
    [8, 8, 8, 8, 6, 6, 4],
    [8, 8, 8, 8, 8, 8, 8],
];

fn organ() -> Vec<Spectrum> {
    REGISTRATIONS
        .iter()
        .map(|drawbars| {
            additive(|harmonic| {
                let drawbar = DRAWBAR_HARMONICS.iter().position(|&of| of == harmonic);
                match drawbar.map(|index| drawbars[index]) {
                    // Each step of a drawbar is 3 dB.
                    Some(level) if level > 0 => 10_f32.powf(-3.0 * f32::from(8 - level) / 20.0),
                    _ => 0.0,
                }
            })
        })
        .collect()
}

/// The first three formants of the vowels a, e, i, o, u of a man's voice in Hz, from Peterson
/// and Barney (1952).
const FORMANTS: [[f32; 3]; 5] = [
    [730.0, 1090.0, 2440.0],
    [530.0, 1840.0, 2480.0],
    [270.0, 2290.0, 3010.0],
    [570.0, 840.0, 2410.0],
    [300.0, 870.0, 2240.0],
];
const FORMANT_GAINS: [f32; 3] = [1.0, 0.5, 0.3];
const FORMANT_WIDTHS_HZ: [f32; 3] = [80.0, 100.0, 120.0];
/// The pitch a vowel frame is made for, about C3. A table plays its harmonics at any pitch,
/// so the vowels move with the note, and are most like themselves near this one.
const VOWEL_PITCH_HZ: f32 = 130.0;
/// Frames from one vowel to the next.
const VOWEL_STEPS: usize = 8;

/// A buzz falling at 6 dB per octave, as the voice makes it, with peaks at the formants.
/// Between two vowels the formants glide in octaves, so the morph moves the peaks and does not
/// fade one set out and the other in.
fn vowels() -> Vec<Spectrum> {
    let count = (FORMANTS.len() - 1) * VOWEL_STEPS + 1;
    (0..count)
        .map(|index| {
            let (vowel, step) = (index / VOWEL_STEPS, index % VOWEL_STEPS);
            let next = (vowel + 1).min(FORMANTS.len() - 1);
            let at = step as f32 / VOWEL_STEPS as f32;
            let formants: [f32; 3] = std::array::from_fn(|formant| {
                let (from, to) = (FORMANTS[vowel][formant], FORMANTS[next][formant]);
                from * (to / from).powf(at)
            });
            additive(|harmonic| {
                let hz = harmonic as f32 * VOWEL_PITCH_HZ;
                let peaks: f32 = (0..3)
                    .map(|formant| {
                        let off = (hz - formants[formant]) / FORMANT_WIDTHS_HZ[formant];
                        FORMANT_GAINS[formant] / (1.0 + off * off)
                    })
                    .sum();
                (0.02 + peaks) / harmonic as f32
            })
        })
        .collect()
}

/// A saw through a resonant two-pole low pass whose cutoff rises from the 2nd harmonic to the
/// 128th, in even steps of pitch.
fn resonant(at: f32) -> Spectrum {
    const Q: f32 = 5.0;
    let cutoff = 2.0 * 64_f32.powf(at);
    additive(|harmonic| {
        let ratio = harmonic as f32 / cutoff;
        let below = 1.0 - ratio * ratio;
        let gain = 1.0 / (below * below + (ratio / Q) * (ratio / Q)).sqrt();
        gain / harmonic as f32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_of_every_table_peaks_at_one_and_has_no_dc() {
        for table in Table::ALL {
            let wavetable = wavetable(table).unwrap();
            assert!((2..=MAX_FRAMES).contains(&wavetable.frames()), "{table:?}");
            for index in 0..wavetable.frames() {
                let frame = wavetable.frame(index);
                assert_eq!(frame.len(), FRAME_LENGTH);
                let peak = frame
                    .iter()
                    .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
                assert!((peak - 1.0).abs() < 1e-5, "{table:?} {index}: {peak}");
                let mean = frame.iter().sum::<f32>() / FRAME_LENGTH as f32;
                assert!(mean.abs() < 1e-5, "{table:?} {index}: {mean}");
            }
        }
    }

    /// Each level of each frame, back through an FFT: nothing above its highest harmonic, and
    /// the copy of its first sample at its end.
    #[test]
    fn no_level_holds_a_harmonic_above_its_limit() {
        let mut planner = RealFftPlanner::<f32>::new();
        for table in Table::ALL {
            let wavetable = wavetable(table).unwrap();
            for level in 0..LEVELS {
                let view = wavetable.level(level);
                let transform = planner.plan_fft_forward(view.length);
                let mut spectrum = transform.make_output_vec();
                for frame in 0..view.frames {
                    let start = frame * (view.length + 1);
                    let samples = &view.samples[start..start + view.length + 1];
                    assert_eq!(samples[view.length], samples[0]);
                    let mut input = samples[..view.length].to_vec();
                    transform.process(&mut input, &mut spectrum).unwrap();
                    let loudest = spectrum.iter().map(|bin| bin.norm()).fold(0.0, f32::max);
                    let above = spectrum[top_harmonic(level) + 1..]
                        .iter()
                        .map(|bin| bin.norm())
                        .fold(0.0, f32::max);
                    // A frame can be silent at a level: a saw synced at 8 times has nothing
                    // under its 8th harmonic.
                    assert!(
                        above <= 1e-5 * loudest,
                        "{table:?} level {level} frame {frame}: {above} of {loudest}"
                    );
                }
            }
        }
    }

    /// The level for a pitch is the fullest one whose highest harmonic stays at or under half
    /// the sample rate.
    #[test]
    fn the_level_for_a_pitch_keeps_its_harmonics_under_half_the_sample_rate() {
        for step in [
            1e-5,
            1.0 / 4096.0,
            1.0 / 2048.0,
            0.001,
            0.0123,
            0.1,
            0.24,
            0.45,
        ] {
            let level = Wavetable::level_for(step);
            assert!(top_harmonic(level) as f32 * step <= 0.5, "{step}: {level}");
            if level > 0 {
                assert!(
                    top_harmonic(level - 1) as f32 * step > 0.5,
                    "{step}: {level}"
                );
            }
        }
        assert_eq!(Wavetable::level_for(f32::NAN), 0);
    }

    /// The morph between two frames is a mix of the two, so a position that moves a little
    /// changes the sound a little: next to every frame of a table the level between it and the
    /// next is half of each.
    #[test]
    fn basic_shapes_morphs_from_a_sine_through_a_saw_at_the_middle_to_a_square() {
        let table = wavetable(Table::BasicShapes).unwrap();
        assert_eq!(table.frames(), 5);
        // A quarter into the cycle a sine is at its top, and a square is at its top the whole
        // first half.
        let quarter = FRAME_LENGTH / 4;
        assert!((table.frame(0)[quarter] - 1.0).abs() < 1e-5);
        let square = table.frame(4);
        assert!(
            square[FRAME_LENGTH / 8..3 * FRAME_LENGTH / 8]
                .iter()
                .all(|sample| *sample > 0.8)
        );
        // The saw at the middle falls through the cycle.
        let saw = table.frame(2);
        assert!(saw[FRAME_LENGTH / 8] > 0.5 && saw[7 * FRAME_LENGTH / 8] < -0.5);
    }
}
