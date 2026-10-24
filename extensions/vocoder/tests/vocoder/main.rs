//! The vocoder as a processor, rendered offline through the engine: what the voice lets
//! through, its latency, and its speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod performance;
mod support;
mod vocoding;
