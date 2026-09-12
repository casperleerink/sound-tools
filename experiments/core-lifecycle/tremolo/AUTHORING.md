# Tremolo authoring test

Built from `SDK.md`, `tone/src/lib.rs`, `tone/src/view.rs`, and the workspace/Tone manifests. No changes were needed in core, UI, Tone, or runtime.

The SDK was sufficient for state registration, processor updates, and the custom GPUI editor. The integration-test instructions omitted the exact open/poll/undo APIs. I searched the public declarations in `core/src/lib.rs` for `Project::open`, `poll_files`, `undo`, `redo`, `undo_len`, and `connections`, then read the connection accessor and `Connection` fields. No other extension or host source was needed. Adding those signatures to the SDK would make the test self-contained.

First compile: `CARGO_TARGET_DIR=/private/tmp/sound-tools-timing/target cargo check --offline -p tremolo` passed in 0.47 seconds. No compiler corrections were needed.

First test run: `CARGO_TARGET_DIR=/private/tmp/sound-tools-timing/target cargo test --offline -p tremolo` passed both integration tests; build finished in 1.36 seconds. No test corrections were needed. Rust formatting was applied afterward.

The tests use real Project instances and temporary project files. They cover two independent instances, only the connected instance reaching audio output, an edit muting output, a file replacement restoring audio, undo and redo, reopened values and routing, and stopped playback with empty undo history after reopening. A separate test checks that replacing every control preserves both oscillator and modulator phases.

The editor reads state from Session and sends edits through Project. Its four controls adjust voice frequency, gain, tremolo rate, and depth. There is no second saved-state store. Runtime phases live only in the processor. Depth zero leaves gain constant; depth one modulates amplitude between zero and the selected gain. Rate is limited to 0.1–20 Hz.

Host integration requires a `tremolo` path dependency, calling `tremolo::register` before opening Project, and calling `tremolo::register_view` with the returned tool. New projects may create `TremoloState::default()` instances and connect their `audio` output. The registered tool name is `example.tremolo`. Host interaction and two-view visual verification remain the host integrator's checks.

Host follow-up: strict workspace Clippy flagged the editor's explicit six-element tuple array type as too complex. The parent removed that type annotation and used one function-pointer cast so Rust infers the array. No SDK change was needed.
