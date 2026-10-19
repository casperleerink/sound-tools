//! The window driven by simulated keys and mouse events, on a project with an offline engine.
//! No display and no device.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them, and
// it does not know `#[gpui::test]`.
#![allow(clippy::unwrap_used)]

mod agent;
mod audio;
mod automation_lanes;
mod built_in_effects;
mod clip_automation;
mod clips;
mod compressor;
mod computer_keys;
mod delay;
mod drum_pad;
mod editing;
mod effects;
mod eq;
mod filter;
mod fit;
mod instruments;
mod lanes;
mod limiter;
mod modulation;
mod notes;
mod other_apps;
mod piece;
mod plugin_card;
#[path = "../shared/plugin_hosts.rs"]
mod plugin_hosts;
mod rack;
mod recording;
mod recording_audio;
mod returns;
mod reverb;
mod sampler;
mod saturator;
mod several_notes;
mod shell;
#[path = "../shared/song.rs"]
mod song;
mod support;
mod tools;
mod track_order;
mod track_panel;
mod transport;
mod utility;
mod velocity;
mod wavetable;
