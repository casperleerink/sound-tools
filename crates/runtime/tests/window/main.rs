//! The window driven by simulated keys and mouse events, on a project with an offline engine.
//! No display and no device.
//!
//! - `shell`: the keys of the window, the focus, the transport and the read-only timeline.
//! - `clips`: adding, moving, resizing, deleting and nudging clips in the arrangement.
//! - `notes`: the note editor.
//! - `track_panel`: the track panel and the view of the synth in it.
//! - `transport`: the tempo, the click and the view following the playhead.
//! - `piece`: a short piece made by hand from the default project, closed and opened again.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them, and
// it does not know `#[gpui::test]`.
#![allow(clippy::unwrap_used)]

mod clips;
mod notes;
mod piece;
mod recording;
mod shell;
mod support;
mod track_panel;
mod transport;
