//! Whole projects with every bundled extension: the default project, live edits heard
//! through the real synth, reopening, the summary and the agent doc.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod agent;
mod agent_doc;
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
mod latency;
mod launcher;
mod limiter;
mod live;
mod metronome;
mod modulation;
mod own_ports;
mod owners;
// The sizes of its page are for the window tests.
#[path = "../shared/plugin_hosts.rs"]
mod plugin_hosts;
mod plugins;
#[allow(dead_code)]
#[path = "../shared/rack.rs"]
mod rack;
mod render_cost;
mod reverb;
mod sampler;
mod saturator;
#[allow(dead_code)]
#[path = "../shared/song.rs"]
mod shared_song;
mod song;
mod song_mixer;
mod summary;
mod support;
mod templates;
mod typescript;
mod utility;
