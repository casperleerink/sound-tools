//! The Drum pad, played the way a track plays it: a test owner tool with a small sequencer
//! sends note events to its `instrument` child and routes the child's audio to the device.
//! Rendered offline. Asserts on samples.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod choke;
mod listen;
mod performance;
mod playing;
mod samples;
mod sounds;
mod support;
