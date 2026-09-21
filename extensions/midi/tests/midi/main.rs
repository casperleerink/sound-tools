//! The MIDI input with messages a test sends itself: no device, no project and no window.
//! CI has no MIDI keyboard, so the device layer is the only part no test here runs.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod messages;
mod playing;
mod recording;
mod support;
mod takes;
