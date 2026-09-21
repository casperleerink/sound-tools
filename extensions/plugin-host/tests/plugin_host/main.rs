//! The plugin host on a real project folder, with the repository's own CLAP test plugin.
//! No third-party plugin is needed, so these run in CI.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod playing;
mod records;
mod scanning;
mod state;
mod support;
