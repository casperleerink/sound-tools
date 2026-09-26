//! The reverb as a processor, rendered offline through the engine: its measured decay, freeze,
//! its glides, its stability and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod decay;
mod freeze;
mod glides;
mod performance;
mod stability;
mod support;
