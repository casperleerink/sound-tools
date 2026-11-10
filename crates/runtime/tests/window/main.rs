//! The window driven by simulated keys and mouse events, on a project with an offline engine.
//! No display and no device.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them, and
// it does not know `#[gpui::test]`.
#![allow(clippy::unwrap_used)]

mod agent;
mod compressor;
mod delay;
mod drum_pad;
mod eq;
mod filter;
mod history;
mod limiter;
mod modulation;
mod other_apps;
mod plugin_card;
#[allow(dead_code)]
#[path = "../shared/plugin_hosts.rs"]
mod plugin_hosts;
#[path = "../shared/rack.rs"]
mod rack;
mod reverb;
mod sampler;
mod saturator;
mod shell;
#[path = "../shared/song.rs"]
mod song;
mod support;
mod tools;
mod utility;
mod wavetable;
