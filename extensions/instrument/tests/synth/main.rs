//! The synth, played the way a track will play it: a test owner tool with a small sequencer
//! sends note events to its `instrument` child and routes the child's audio to the device.
//! Rendered offline. Asserts on samples.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod device;
mod editing;
mod pedal;
mod performance;
mod playing;
mod properties;
mod support;
