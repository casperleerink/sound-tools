//! The live project folder, driven through the same function the watcher calls, with explicit
//! paths. Only `watching.rs` uses the real watcher.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod binding;
mod composite;
mod editing;
mod outside;
mod properties;
mod scale;
mod tools;
mod watching;
