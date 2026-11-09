//! Building blocks of sound for processors: filters, envelopes, LFOs, oversampling, a limiter,
//! a level detector, spectra, gain and pan. More than one extension needs each; the core needs
//! none of them.

mod detector;
mod dsp;
mod envelope;
mod gain;
mod lfo;
mod limiter;
mod oversampling;
mod saturation;
mod stft;
mod svf;
pub mod testing;

pub use detector::PeakDetector;
pub use dsp::{
    HIGHEST_PHASE_STEP, OnePole, Taps, all_held_silent, all_positive_zero, all_zero, bessel_i0,
    held, pole, poly_blep, xorshift32,
};
pub use envelope::{ENVELOPE_FLOOR, Envelope, EnvelopeCurves, EnvelopeStage, EnvelopeState};
pub use gain::{amplitude, decibels, pan_gains};
pub use lfo::{Lfo, LfoShape};
pub use limiter::PeakLimiter;
pub use oversampling::{Oversampler, OversamplerDelay, OversamplingFilters};
pub use saturation::soft_clip;
pub use stft::{SpectralBin, Stft};
pub use svf::{
    FilterCharacter, FilterSlope, FilterType, SHELF_MAX_Q, SVF_MAX_Q, ShapedSvf, SvfFactors,
    SvfSection, SvfShape, svf_response,
};
