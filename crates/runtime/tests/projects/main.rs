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
mod feeds;
mod filter;
mod latency;
mod launcher;
mod limiter;
mod live;
mod metronome;
mod modulation;
mod own_ports;
mod owners;
mod plugin_hosts;
mod plugins;
mod rack;
mod render_cost;
mod reverb;
mod sampler;
mod saturator;
mod shared_song;
mod song;
mod song_mixer;
mod song_sound;
mod sound_ports;
mod summary;
mod support;
mod template_sound;
mod typescript;
mod utility;
