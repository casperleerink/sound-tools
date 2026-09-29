//! The limiter as a processor, rendered offline through the engine: the ceiling, the sound
//! under it, the release, the lookahead, the glide of the gain, the meters and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod ceiling;
mod glides;
mod lookahead;
mod meters;
mod performance;
mod release;
mod support;
mod transparency;
