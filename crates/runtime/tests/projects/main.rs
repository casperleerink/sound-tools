//! Whole projects with every bundled extension: the default project, live edits heard
//! through the real synth, reopening, the summary and the agent doc.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod agent;
mod agent_doc;
mod audio;
mod automation;
mod blocks;
mod cables;
mod compressor;
mod delay;
mod devices;
mod drums;
mod effects;
mod eq;
mod events;
mod export;
mod filter;
mod fit;
mod generated_take;
mod latency;
mod launcher;
mod limiter;
mod live;
mod metronome;
mod mixer;
mod modulation;
mod own_ports;
mod owners;
// The sizes of its page are for the window tests.
#[path = "../shared/plugin_hosts.rs"]
mod plugin_hosts;
mod plugin_lanes;
mod plugins;
mod rack_order;
mod recording;
mod recording_audio;
mod render_cost;
mod reverb;
mod sampler;
mod saturator;
mod scale;
#[allow(dead_code)]
#[path = "../shared/song.rs"]
mod shared_song;
mod solo;
mod song;
mod song_mixer;
mod summary;
mod support;
mod typescript;
mod utility;
