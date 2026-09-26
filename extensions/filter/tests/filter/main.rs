//! The filter as a processor, rendered offline through the engine: its measured response, its
//! glides, its stability and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod glides;
mod performance;
mod response;
mod stability;
mod support;
