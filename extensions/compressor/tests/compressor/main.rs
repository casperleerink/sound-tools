//! The compressor as a processor, rendered offline through the engine: its measured gain, the
//! times of its attack and release, its lookahead, its glides, its stability and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod glides;
mod lookahead;
mod meters;
mod performance;
mod stability;
mod static_gain;
mod support;
mod timing;
