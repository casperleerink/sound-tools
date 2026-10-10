//! The gate as a processor, rendered offline through the engine: its measured gain, its hold,
//! its sidechain, its transient shaper and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod gating;
mod performance;
mod shaper;
mod support;
