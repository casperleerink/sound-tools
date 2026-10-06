#![allow(clippy::unwrap_used)]
//! The Sampler in an engine, on a project folder with its sample files: pitch, envelope,
//! velocity, the part of the file that plays, clicks, edits while it plays, loading, and SFZ
//! instruments.

mod clicks;
mod envelope;
mod library;
mod loading;
mod looping;
mod performance;
mod pitch;
mod sfz;
mod support;
