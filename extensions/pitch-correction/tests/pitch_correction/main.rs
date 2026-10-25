//! The pitch correction as a processor, rendered through the engine: where it moves a voice,
//! how fast, what it leaves alone, its timing, and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod correction;
mod performance;
mod playing;
mod realtime;
mod support;
