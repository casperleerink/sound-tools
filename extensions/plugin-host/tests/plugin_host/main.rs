//! The plugin host on a real project folder, with the repository's own CLAP test plugin.
//! No third-party plugin is needed, so these run in CI.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

#[global_allocator]
static ALLOCATOR: support::CountingAllocator = support::CountingAllocator;

mod background;
mod consistency;
mod editing;
mod effects;
mod lanes;
mod lifecycle;
mod listing;
mod parameters;
mod pins;
mod playing;
mod records;
mod remembered;
mod rendering;
mod resizing_and_keys;
mod restarts;
mod scanning;
mod state;
mod support;
mod window;
