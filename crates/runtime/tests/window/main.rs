//! The window driven by simulated keys and mouse events, on a project with an offline engine.
//! No display and no device.
//!
//! - `shell`: the keys of the window, the focus, the transport and the read-only timeline.
//! - `clips`: adding, moving, resizing, deleting and nudging clips in the arrangement.
//! - `notes`: the note editor.
//! - `several_notes`, `velocity`: several notes, copy and paste, and the velocity lane.
//! - `rack`: reordering the track rack.
//! - `track_order`: moving tracks up and down by their headers and with alt and the arrows.
//! - `track_panel`: the track panel and the view of the synth in it.
//! - `instruments`: picking the instrument of a track, and the plugin's own window.
//! - `drum_pad`: the card of the Drum pad: its pads, its keys and samples dropped on it.
//! - `effects`: adding and removing effects in the rack.
//! - `fit`: fitting the tempo to a take, and the steadiness in the transport.
//! - `transport`: the tempo, the click and the view following the playhead.
//! - `piece`: a short piece made by hand from the default project, closed and opened again.
//! - `audio`: audio clips moved, trimmed, faded and turned up or down, copied and pasted, the
//!   Clip card, files dropped from the Finder, and adding an audio track.
//! - `recording_audio`: arming audio tracks, the input select and recording them from a
//!   simulated input.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them, and
// it does not know `#[gpui::test]`.
#![allow(clippy::unwrap_used)]

mod audio;
mod clips;
mod compressor;
mod drum_pad;
mod editing;
mod effects;
mod eq;
mod filter;
mod fit;
mod instruments;
mod limiter;
mod modulation;
mod notes;
mod piece;
mod rack;
mod recording;
mod recording_audio;
mod reverb;
mod sampler;
mod saturator;
mod several_notes;
mod shell;
mod support;
mod track_order;
mod track_panel;
mod transport;
mod utility;
mod velocity;
