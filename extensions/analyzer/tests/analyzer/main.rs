//! The analyzer as a processor, rendered offline through the engine: the sound passes bit for
//! bit, the scope gets it, and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod passing;
mod performance;
mod support;
