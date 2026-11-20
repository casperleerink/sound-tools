//! What brings sound and notes into a project, on the offline engine with Bun and a simulated
//! audio input: the keyboard, the takes of recorders, other apps and files brought in.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod import;
mod keys;
mod other_apps;
mod recorder;
mod support;
