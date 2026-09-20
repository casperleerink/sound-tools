//! The arrangement on a real project folder with an offline engine. The instrument of every
//! track is a probe whose output level says exactly which notes are held on which frame.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod held_notes;
mod live_edits;
mod preview;
mod records;
mod support;
mod timing;
