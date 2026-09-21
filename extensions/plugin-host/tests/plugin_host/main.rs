//! The plugin host on a real project folder, with the repository's own CLAP test plugin.
//! No third-party plugin is needed, so these run in CI.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

/// So that a test can say a block of audio allocated nothing, including inside the plugin's
/// own call, where the realtime sanitizer is switched off.
#[global_allocator]
static ALLOCATOR: support::CountingAllocator = support::CountingAllocator;

mod background;
mod consistency;
mod lifecycle;
mod playing;
mod records;
mod rendering;
mod scanning;
mod state;
mod support;
mod window;
