//! Whole projects with every bundled extension: the default project, live edits heard
//! through the real synth, reopening, the summary and the agent doc.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod agent_doc;
mod audio;
mod compressor;
mod drums;
mod effects;
mod eq;
mod export;
mod filter;
mod fit;
mod generated_take;
mod latency;
mod live;
mod metronome;
mod mixer;
mod plugins;
mod rack_order;
mod recording;
mod recording_audio;
mod reverb;
mod sampler;
mod scale;
mod summary;
mod support;
