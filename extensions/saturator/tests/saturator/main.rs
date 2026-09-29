//! The saturator as a processor, rendered offline through the engine: its measured response,
//! its harmonics and how little folds back, its level, its glides, its stability and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod glides;
mod harmonics;
mod level;
mod performance;
mod response;
mod stability;
mod support;
