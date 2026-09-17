# Build scratchpad

## Goal

Build the headless audio core and bundled DAW extensions, then the GPUI workspace and an agent sidebar that edits ordinary project files. Work stays on `t3code/discuss-rust-dsp-architecture`.

## Constraints

- Existing architecture and design documents guide the work. Keep audio and persisted state independent of GPUI.
- Extensions own tracks, clips, notes, instruments, effects and mixing. The core owns execution, project services, timing and devices.
- Use Union Alpha only for delegated work and reviews.
- No promise of unattended operation beyond this active session. VST3 follows a verified working DAW.
- Do not commit or push without explicit authorization.

## Verification

Run Rust formatting, package tests and strict Clippy for each stage. Exercise offline rendering and project-file replacement before claiming playback works. Device audio and native UI need separate environment checks.

## 2026-09-17, initial inspection

- The working branch is already non-main and clean.
- The root workspace contains the UI SDK and gallery only. The lifecycle core is an isolated experiment with offline mono rendering, not a realtime engine.
- `.cargo/config.toml` hardcodes a macOS temporary target path. Linux verification uses `CARGO_TARGET_DIR=/tmp/opencode/sound-tools-target`.
- Rust 1.98.1 and OpenCode CLI are available. Full-workspace test builds exceed the 2 minute tool timeout once GPUI is involved; use package-scoped checks during development and accept the longer build for integration checkpoints.
- ALSA 1.2.11 development headers and Xvfb exist. Pi is not installed on PATH. The agent provider remains undecided, so no agent credentials should enter the code.
- Delegated Union Alpha sessions via `opencode run` work but returned nothing in the observed window. Built the core directly instead.

## Implemented

- `crates/core` (8 passing tests, strict Clippy clean):
  - `registry.rs`: typed tool registry; external JSON validation happens through one decode path.
  - `audio.rs`: block-based `Processor` trait, planar `AudioBuffer`, fixed event data, fan-in summing graph, topological order, cycle rejection, short-block support, no allocation in `process`.
  - `clock.rs`: piecewise tempo map with exact beat/frame round trips and transport state with a revision counter.
  - `project.rs`: JSON project folder (`project.json` + `state/*.json`), atomic writes, insertion/replacement/deletion with cascade ownership, undo/redo, gesture begin/finish/cancel, file polling with last-write-wins.
- `crates/daw` (10 passing tests, strict Clippy clean):
  - `arrangement.rs`: notes, clips, tracks with per-track FxChain (filter/delay/reverb settings); sorted non-overlapping validation; note events with offs before ons at equal frames.
  - `dsp.rs`: 32-voice polyphonic Synth (sine/saw/square, sample-offset note events, attack/release envelopes), Gain with smoothing, one-pole Filter, stereo Delay, Freeverb-style Reverb. All implement the core `Processor`; events carry note on/off (custom kinds 1/2/3) and parameter values.
  - `engine.rs`: maps an arrangement record + mixer record to a running core graph (synth → optional fx chain → track gain → master → device); `render_block_into` returns full block samples; `end_to_end_project_renders_non_silent_audio` proves project → graph → non-silent output.
- `crates/runtime` (headless CLI, strict Clippy clean):
  - `--inspect` prints records and connections; `--render out.wav --seconds N` writes a correct IEEE float32 stereo WAV (verified: 96000 frames = 2.0s, peak 0.358).
  - `--watch` polls project files every 50 ms, rebuilds the engine on valid edits, ignores invalid ones, prints per-window peaks.
  - Verified live agent-style edit: appended a Bass track to `state/arrangement.json` with Python while `--watch` ran; the runtime picked it up and rendered both tracks.

## Decisions and open issues

- Processor state belongs to the audio executor; parameter updates must not reset oscillator phases or delay buffers.
- The graph rejects feedback cycles for now; delayed feedback is deferred until the working DAW exists.
- Polling baseline: external file edits during an active gesture merge record-by-record against the gesture baseline; a repeated identical poll cannot resurrect a stale disk edit over a newer in-gesture update (covered by `repeated_polls_do_not_resurrect_stale_file_edits_over_newer_gesture_updates`).
- `edit_manifest` currently rejects index changes; instance index changes go through insert/delete only.
- DSP event contract: core `EventData` is fixed (`Parameter {id: u32, value: f32}` plus `Custom {kind: u32, data: [f32; 4]}`); the DAW layer maps notes to custom kinds 1/2/3 so the core never knows about notes (per ARCHITECTURE.md).
- Saved record shapes are one tool per record: arrangement holds `Arrangement`, mixer holds `MixerState`. Gain/pan/mute/solo/master edits use parameter events without rebuilding; topology, sample configuration and effect changes rebuild the graph.
- Fresh projects include a starter track with a short melody; the engine tolerates zero tracks (deleting the last track does not break playback).
- The WAV writer initially declared 16-bit PCM around a float32 payload; verified with a standalone Python reader and fixed to format tag 3.
- Lesson: verify numeric output with a standalone reader of the exact bytes, and double-check the reader itself before trusting its report (my first two probes misread stereo interleaving).
- Lesson: build and test after every structurally new module. A single orphaned brace in an uncompiled file cost more than three checkpoint runs would have.
- Next: GPUI workspace (timeline, transport, mixer), reusing the runtime engine; then the agent sidebar driven by the same project files + watch loop.

## Realtime playback diagnosis and fix

- Instrumentation found one 4800-frame callback before `start()`, followed by 2048-frame callbacks. All CLI losses happened before playback started. The callback now outputs silence without consuming the ring until `start()` succeeds.
- The app session also queued only 1024 frames against those 2048-frame callbacks. It now prefills and replenishes the full 8192-frame ring. This adds about 170 ms of queued latency at 48 kHz.
- Five separate five-second session probes reported zero underruns after the fix, versus 122048 before the session buffer fix. CLI probes also reported zero after the startup gate fix.
- Deterministic callback tests cover pre-start queue preservation and post-start accounting. The real-output session regression passed in the final workspace run and a separate explicit run.
- PipeWire exposes a dummy output here. The earlier claim that this alone caused the underruns was wrong. Audible output and low-latency performance still need a hardware-backed host.

## Connected workspace checkpoint

- `sound-app <project-directory>` opens the GPUI workspace with timeline, play/pause/stop/seek, clip selection, explicit clip editing, track gain/pan/mute/solo, master peak display and undo/redo.
- A background session owns project state, engine and audio output. Bounded command queues return receipts, including errors and no-op history. Arrangement revisions reject stale UI edits.
- Sampler import copies validated WAV files into project assets. Notes trigger pitched stereo sample playback; imported projects render after the original source is removed.
- MIDI input supports port discovery, one input routed to one track, live notes while transport is stopped, and overflow/disconnect voice cleanup. Hardware MIDI is not verified; this host denies access to `/dev/snd/seq`.
- Pi sidebar invokes a configured executable with project-directory context, bounded output and cancellation of its process group. Protocol and subprocess tests pass. Pi is missing from PATH, so provider authentication and an actual agent composition are unverified. Configure `SOUND_TOOLS_AGENT_BIN` with an installed Pi executable.
- Candidate validation rejects missing sample assets before local persistence or acceptance of external edits. Overflow-safe validation and note restoration on seek/resume/rebuild have regression tests.
- Final checkpoint: 93 tests pass across core, DAW, runtime and app; strict workspace all-target Clippy passes; app and CLI builds pass; formatting passes for changed packages. Existing UI/gallery formatting differences were left untouched.
- A bounded Xvfb launch produced no application panic in captured output. Visual interaction remains unverified; earlier Xvfb screenshots were black.
- VST3 remains deferred under the requirement to verify the working DAW first. No plugin-host support is claimed.
- Storage retains the documented per-file atomicity limit. Multi-record writes are not a project-wide transaction; partial I/O failure across files remains a recovery limitation.
- No commits or pushes were made.
