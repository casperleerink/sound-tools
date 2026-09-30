//! The Wavetable synth, played the way a track plays it: a test owner tool with a small
//! sequencer sends note events to its `instrument` child and routes the child's audio to the
//! device. Rendered offline. Asserts on samples and spectra.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod effects;
mod filters;
mod matrix;
mod performance;
mod record;
mod sound;
mod support;
mod unison;
mod voicing;
