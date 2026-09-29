//! The modulation as a processor, rendered offline through the engine: its measured response,
//! what each mode does to a tone, the stereo spread, the mix, its glides, its stability and its
//! speed.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod glides;
mod modes;
mod performance;
mod response;
mod stability;
mod stereo;
mod support;
