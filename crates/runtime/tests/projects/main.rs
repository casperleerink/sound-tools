//! Whole projects with every bundled extension: the default project, live edits heard
//! through the real synth, reopening, the summary and the agent doc.

// Clippy allows unwrap inside `#[test]` functions only, not in the helpers next to them.
#![allow(clippy::unwrap_used)]

mod agent_doc;
mod live;
mod scale;
mod summary;
mod support;
