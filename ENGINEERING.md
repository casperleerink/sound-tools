# Engineering guide

How to work in this codebase: the rules for code, the audio thread, and testing. Product decisions are in [ARCHITECTURE.md](ARCHITECTURE.md). The engine API for extension authors is in [crates/core/README.md](crates/core/README.md), and the view API in [crates/ui/README.md](crates/ui/README.md). Commands to build and check are in [README.md](README.md), "Checks".

## Rules for writing code

Rules are traps to avoid, not general advice. Add a rule only when it is non-obvious, happened more than once and is actionable.

- Correctness and clarity first. Optimize only the audio path or measured hot spots.
- Comments explain why, never restate the code.
- No `unwrap()` or panicking indexing outside tests. Propagate with `?`.
- Never discard errors with `let _ =`. Use `?`, `.log_err()` or an explicit `match`. Errors from background work must reach the UI.
- Full-word variable names.
- Before an `async move`, shadow clones inside a block:
  ```rust
  executor.spawn({
      let state = state.clone();
      async move { state.update(); }
  });
  ```
- GPUI: no `cx.notify()` or entity updates inside `render`; no blocking file I/O or sleeps on the UI thread. See the gpui skills in [.agents/skills/](.agents/skills/).
- Audio thread: nothing from "What may not happen on the audio thread" below.
- Check a crate's current version and changelog before using it from memory. Agents tend to write code for older APIs.
- Every dependency is declared once in `[workspace.dependencies]`. Every crate uses the workspace lints.
- Extensions depend on the SDK and on small shared contract crates, never on each other. `tooling/workspace-rules` fails the tests if one does. This keeps the build wide and parallel.
- Warnings fail CI only (`.cargo/ci-config.toml`), never a local build: a warning in agent-written code must not break a composer's build.
- Never vary `rustflags` or environment variables between builds. Any change rebuilds everything. The sanitizer run below is the one exception.

## The audio engine

The engine is in `crates/core`. It follows what Pure Data, Elementary and similar engines do: edit a graph on a normal thread, compile it to a flat schedule there, and hand that to the audio thread through a lock-free queue.

### Threads

| Thread | Owns | May |
| --- | --- | --- |
| UI (GPUI) | Views | Edit project state through the editing service. |
| Control | Project graph, compiler, tempo map, assets | Allocate, lock, do I/O. Compiles schedules and builds processors. |
| Audio (device callback) | Live processors, current schedule, buffers | Only preallocated work. |

The engine is a value, not global state. The device callback, offline rendering and tests all call the same `Engine::process_block`. Pd needed years to add multiple instances because it started with globals.

### What may not happen on the audio thread

No allocation, no freeing, no locks, no I/O, no logging, no GPUI calls. So no `Mutex`, no `Vec::push` past capacity, no `Box::new`, no `Arc` drops, no `String` formatting.

How the engine keeps to that:

- Two fixed-size rings connect control and audio. One edit is one batch of commands. The audio thread applies a command by swapping: the new value goes in, the old one rides back in the same batch on the return ring, and the control side drops it. Nothing is ever freed on the audio thread.
- The audio thread takes a batch only when the return ring has room, so a full ring never forces a drop. The control side keeps batches that did not fit and sends them later, in order. Edits are never lost.
- Every `Arc` the audio thread might release must come back the same way.
- A whole edit is validated and compiled before anything is sent. A half-applied edit never reaches the engine.
- Status and counters (blocks, overflows, playhead) go out through a triple buffer and only grow, so reading the latest never misses a count. Meters use `Peaks`, an atomic maximum per channel, so a meter sees the loudest block since it last looked.
- Events are `Copy` types in preallocated per-port buffers. Overflow is counted, never allocated.
- Processors have two phases, from Pd: `prepare` on the control thread may allocate; `update` and `process` on the audio thread may not. `update` swaps values out of its message and never drops them.
- Processors live in a slot table. A routing change sends a new schedule and every surviving processor keeps its state.
- Device buffers are split into sub-blocks of at most 64 frames. Edits are taken at each sub-block start.
- Time is integers: engine time is a frame counter, and ticks convert to frames through one function in `clock.rs`, which rounds in one place. Every part then agrees on the frame of a tick.
- Timeline processors make their own events from the transport info of each block, instead of the control thread scheduling ahead. Timing then never depends on control thread latency.

### How the realtime sanitizer check runs

`Engine::process_block` is marked `#[nonblocking]` with `rtsan-standalone`. It does nothing unless the build sets `RTSAN_ENABLE=1`. Then it aborts on any allocation, lock or system call inside `process_block`, which covers every `update` and `process`. The input device callback (`CaptureWriter::write`) is marked the same way.

- The CI job `sanitizer` in `.github/workflows/ci.yml` runs the tests of every crate with a processor, and then `runtime --test projects`, which plays real projects with file edits and recording.
- Add every new crate with a processor to that list.
- A test in `crates/core/tests/engine.rs` starts a child that allocates inside `process` and expects the abort, so a sanitizer that is silently off fails CI.
- `HostedPlugin` wraps each call into a third-party plugin in a `ScopedDisabler`: what a plugin does inside itself is not ours to check. Because that could hide our own buffers growing, the plugin host is also tested with a counting global allocator.

`no_denormals` wraps the body of `process_block`, so offline renders and the device compute the same.

### DSP speed

- Measure a DSP crate as a whole project: 100 instances in one engine, rendered offline in the dev profile, as a realtime ratio. `extensions/instrument/tests/synth/performance.rs` is the pattern. An idle processor returns before it touches its output.
- A recursive filter is bound by the chain of operations from one frame to the next, not by their count. Shorten the chain.
- Do not use `f32::mul_add`: one instruction on Apple Silicon, a slow library call on x86 without FMA.
- Work out filter factors (`tan`, `powf`) once per block and only while a parameter moves, never per frame. Smooth the parameter, not the factors.
- Every crate with DSP code gets `opt-level = 3` in the dev profile. Unoptimized audio glitches and hides real performance problems.

## Testing

- DSP, graph, clock and project state tests are plain `#[test]` with no GPUI. Offline rendering through `process_block` makes audio testable: render frames, then assert on samples or snapshot a summary with `insta`.
- Project folder tests call `Project::apply_outside_changes` with explicit paths, the function the file watcher calls, so they do not depend on timing.
- Property tests (`proptest`) cover the graph compiler, the clock round trips, transport events over random tempo maps and buffer sizes, and state application.
- `#[gpui::test]` only for views and entities. In GPUI tests use `cx.background_executor().timer(..)`, never `smol::Timer::after`, or `run_until_parked()` fails.
- Window behaviour is tested with a simulated mouse and keys on an offline engine run by hand (`crates/runtime/tests/window/`). The tests check the project, the undo history and the files after every gesture.
- Recording is tested with a simulated input device and output timing. No test needs real hardware.
- The two snapshot tests render the component gallery and the window to PNGs without a display. Look at the PNGs after a UI change. `WINDOW_SNAPSHOT_ONLY` picks a subset of window states.
- A file under `tests/` with helper functions starts with `#![allow(clippy::unwrap_used)]`, because `allow-unwrap-in-tests` covers only `#[test]` functions.

CI runs the commands of [README.md](README.md), "Checks", plus the sanitizer job. The jobs (lint, tests, snapshots, sanitizer, Linux) run side by side. Lint, snapshots and the sanitizer run on macOS; the Linux job builds and runs the tests on Ubuntu. Add Miri for new unsafe code.

To try Linux from a Mac: an `ubuntu:24.04` Docker container with the README packages, `CARGO_TARGET_DIR` on a volume, run as a normal user (root can write into the read-only folders some tests make). For the window add `xvfb` and `mesa-vulkan-drivers` and a null sound card. Set `CARGO_BUILD_JOBS=4` on Docker Desktop's default memory.

## GPUI version

`gpui` and `gpui_platform` come from Zed git, pinned to the commit of a stable Zed release in the root `Cargo.toml`, because the crates.io release is old and lacks what we use (offscreen rendering, arcs, the split platform crate). `rust-toolchain.toml` matches the toolchain Zed pins at that commit. To upgrade: move both together, compare the gallery snapshots with the previous PNGs, and update the gpui skills in the same change.

## Reference repos

Clone them next to this repo and read them before designing something they already solved.

- [Zed](https://github.com/zed-industries/zed): GPUI itself and how a large GPUI app is built. Its `Cargo.toml` profiles and lints, `.rules`, `clippy.toml`, `tooling/xtask/src/workspace.rs` (dependency rules), `crates/gpui_util` (`log_err` and friends).
- [Pure Data](https://github.com/pure-data/pure-data): the classic audio graph. `src/d_ugen.c` (graph sort, buffers), `src/m_sched.c` (scheduler, clocks), `src/g_canvas.c` (rebuilding the graph), `src/d_delay.c` (feedback delay). Also what not to copy: globals, silently dropped cycles, sort-dependent delays.
- [Elementary](https://github.com/elemaudio/elementary): compiled schedules swapped into a realtime runtime. `runtime/elem/Runtime.h` (schedule swap, garbage collection), `runtime/elem/GraphRenderSequence.h`, `runtime/elem/builtins/Feedback.h`. Also what not to copy: dropping a schedule when its queue is full, freeing on the audio thread, crossfading the whole output on every edit.
