//! The delay as a processor, rendered offline through the engine: when its repeats come, synced
//! and free, how loud each one is, its glides, its stability and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod glides;
mod performance;
mod repeats;
mod stability;
mod support;
mod timing;
