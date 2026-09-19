# Engineering guide

How to build Sound Tools: tooling, dependencies and the audio engine design. Researched September 19, 2026 from Zed (`916fc2b`), Pure Data (`d9639d2`, 0.57 dev), Elementary (`60e7234`) and crates.io. [ARCHITECTURE.md](ARCHITECTURE.md) remains the source of truth for product decisions. This file gives recommendations for how to implement them. When the two disagree, ARCHITECTURE.md wins. Record the disagreement there.

Reference repos: [Zed](https://github.com/zed-industries/zed), [Pure Data](https://github.com/pure-data/pure-data) and [Elementary](https://github.com/elemaudio/elementary). Clone them next to this repo and read them before designing something they already solved. Paths in this file are relative to each repo's root.

Versions below were current on the research date. AI agents tend to write code for older APIs. Use these pinned versions and check the crate's changelog before relying on memory.

## 1. Toolchain and workspace

### Toolchain

Pin an exact toolchain in `rust-toolchain.toml`. The shared target directory rebuilds every dependency when the compiler changes, so upgrades should be deliberate commits. Stable was 1.98.1 on the research date. The pin follows the gpui revision instead, see section 6.

```toml
[toolchain]
channel = "1.97.1"
profile = "minimal"
components = ["rustfmt", "clippy", "rust-analyzer", "rust-src"]
```

Edition 2024 everywhere. Useful recent language features: let chains (1.88), `if let` guards in match arms (1.95), async closures (1.85), `File::lock` (1.89), `get_disjoint_mut` (1.86). `std::simd` is still nightly only.

### Cargo config

Keep the shared target directory from `.cargo/config.toml`. Add Zed's v0 symbol mangling for readable closure backtraces:

```toml
[target.'cfg(all())']
rustflags = ["-C", "symbol-mangling-version=v0"]
```

Never vary `rustflags` or environment variables between builds. Any change rebuilds everything. The outer application must run cargo with the same clean environment every time, and rust-analyzer should not inherit variables from `cargo run`.

Do not set a custom linker on macOS. Apple's default linker is fast, mold dropped macOS, and Wild is ELF only. On Linux x86_64, rust-lld is already the default since 1.90.

Put `-D warnings` in a CI-only config file (`.cargo/ci-config.toml`, passed with `--config`), not in the local config. A warning in an agent-written extension must never fail a composer's build. Zed does the same.

### Profiles

Start from this. It keeps our measured `debug = 0` and adds Zed's build-time settings.

```toml
[profile.dev]
debug = 0
incremental = true
codegen-units = 16

# Build scripts and proc macros use the same settings.
# Without this, Zed measured ~400 crates compiling twice.
[profile.dev.build-override]
debug = 0
codegen-units = 16

# Fast proc macros make every derive-heavy extension build faster.
# Fast layout and SVG keep debug UI usable.
[profile.dev.package]
syn = { opt-level = 3 }
quote = { opt-level = 3 }
proc-macro2 = { opt-level = 3 }
serde_derive = { opt-level = 3 }
gpui_macros = { opt-level = 3 }
taffy = { opt-level = 3 }
resvg = { opt-level = 3 }
# Every crate with DSP code gets opt-level 3 (validated in the build-loop experiment).
# Audio in an unoptimized build glitches and hides real performance problems.

[profile.dbg]
inherits = "dev"
debug = "full"

[profile.release]
debug = "limited"
lto = "thin"
codegen-units = 1

# For profiling audio without thin-LTO link times.
[profile.release-fast]
inherits = "release"
lto = false
codegen-units = 16
debug = "full"
```

Adopted September 19, 2026. Measured on the workspace with only the UI crate and gallery: the cold build went from 38 s to 45 s, and a rebuild after touching the UI crate from 1.7 s to 1.6 s. The slower cold build is the optimized proc macros, layout and SVG crates. It is a one-time cost per machine. Measure again once derive-heavy extension crates exist.

### macOS build loop

macOS Gatekeeper scans every new binary, which Zed measured at a few seconds per iteration. Run `sudo spctl developer-mode enable-terminal`, then add the terminal and the outer application under System Settings → Privacy & Security → Developer Tools. Check this first if the reload loop is slower than the 2.2 s measured in the experiment.

Also check the Cargo book chapter "Optimizing Build Performance" (added in 1.92). Skip Cranelift for now. It cannot unwind panics on macOS and only partly supports SIMD. The parallel front end is still nightly only.

### Crate layout

- Every dependency, internal or external, is declared once in `[workspace.dependencies]`. Members write `foo.workspace = true`. Every crate sets `[lints] workspace = true`.
- Bundled extensions live in `extensions/`, everything else in `crates/` and `tooling/`. Extensions depend on the SDK and on small shared contract crates, never on each other's implementation. A test in `tooling/workspace-rules` reads `cargo metadata` and fails if one extension crate depends on another, directly or transitively. Zed does this in `tooling/xtask/src/workspace.rs`. It keeps the build wide and parallel, which protects incremental build time.
- Test fakes go behind a `test-support` feature.
- No `mod.rs` files. Use `src/foo.rs` next to `src/foo/`. Avoid many tiny files.
- Dev builds read UI assets from disk; release builds embed them (Zed's `rust-embed` pattern), so asset edits need no rebuild.

### Lints

Follow Zed: deny a short list of real mistakes, allow clippy's style group so agents do not fight style lints.

```toml
[workspace.lints.clippy]
dbg_macro = "deny"
todo = "deny"
redundant_clone = "deny"
disallowed_methods = "deny"
declare_interior_mutable_const = "deny"
undocumented_unsafe_blocks = "deny"
unwrap_used = "warn"
style = { level = "allow", priority = -1 }
type_complexity = "allow"
too_many_arguments = "allow"
```

`clippy.toml`:

```toml
allow-unwrap-in-tests = true
disallowed-methods = [
  { path = "std::process::Command::spawn", reason = "Blocks the current thread", replacement = "smol::process::Command::spawn" },
  { path = "smol::Timer::after", reason = "Non-deterministic in GPUI tests", replacement = "gpui::BackgroundExecutor::timer" },
  { path = "serde_json::from_reader", reason = "Much slower", replacement = "serde_json::from_slice" },
]
```

`rustfmt.toml` holds only `edition = "2024"` and `style_edition = "2024"`.

## 2. Dependencies

Match what gpui 0.2.2 already pulls in (smol 2, async-task, log, parking_lot, slotmap, anyhow, thiserror 2, serde_json) instead of adding competing crates.

### App infrastructure

| Need | Use | Notes |
| --- | --- | --- |
| Errors | `anyhow` 1 + `thiserror` 2 | anyhow in app code, thiserror at crate boundaries where callers match on errors. |
| Error helpers | `gpui_util` =0.2.2 | Zed's `ResultExt::log_err()`, `warn_on_err()`, `debug_panic!`, `maybe!`. Depend on it; do not copy. |
| Async | GPUI executors (`cx.background_spawn`) | No tokio unless a dependency forces it. |
| Logging | `tracing` 0.1 + `tracing-subscriber` 0.3 | The subscriber also captures gpui's `log` records. Never log on the audio thread. |
| Profiling | `tracing-tracy` 0.12 / `tracy-client` 0.19 | Behind a feature so it costs nothing normally. Pin `tracy-client-sys` to the Tracy GUI version. |
| JSON | `serde` 1, `serde_json` 1 with `preserve_order` | Key order stays stable, so agent diffs of project files stay small. |
| JSON errors | `serde_path_to_error` 0.1 | Reports `tracks[3].gain` instead of a line number. Agents fix their edits from this. |
| JSON schema | `schemars` 1 | Optional. Publish schemas of record types for agents. |
| File watching | `notify` 8.2 + `notify-debouncer-full` 0.7 | notify 9 was still a release candidate. |
| Atomic writes | `atomic-write-file` 0.3 | Or `tempfile::persist`. Ignore watcher events from our own writes. |
| Project lock | `std::fs::File::lock` | Stops two runtimes opening one project. No dependency. |
| IPC | JSON lines over the child's stdin/stdout | The runtime is the outer app's child process. Use `interprocess` 2 only if a reconnectable socket becomes necessary. Avoid `ipc-channel`. |
| IDs | `slotmap` 1 | Already in gpui. |
| Channels (non-realtime) | std `mpsc` or `crossbeam-channel` | |
| Undo | Our own | With whole-record edits, storing before and after records is simpler than an undo crate. |

### Audio

| Need | Use | Notes |
| --- | --- | --- |
| Device I/O | `cpal` 0.18 | 0.18 changed error kinds, made PipeWire the Linux default and renamed the realtime feature. The next major renames `play()` to `start()` and adds duplex streams. Keep our wrapper thin. |
| Control ↔ audio queues | `rtrb` ≥0.4 | Lock-free SPSC. Versions before 0.3.5 have a soundness bug. |
| Audio → UI snapshots | `triple_buffer` 9 | Meters, playhead, CPU load. Latest value wins. |
| Denormals | `no_denormals` 0.3 | Wrap the callback. |
| Realtime checks | `rtsan-standalone` 0.3 | Mark `process` functions `#[nonblocking]` in debug and CI builds; it reports allocations, locks and syscalls. |
| Worker thread priority | `audio_thread_priority` 0.38 | Only for our own realtime threads. cpal's callback thread already has realtime priority on macOS. |
| SIMD | `wide` 1.7 | Plain loops that auto-vectorize first. |
| Decoding | `symphonia` 0.6 | 0.6 rewrote the API. Agents will write 0.5 code; read the migration guide. |
| WAV writing | `hound` 3.5 | Old but finished. |
| Resampling | `rubato` 5 | Five majors in 2026. Pin exactly. |
| FFT | `realfft` 3.5 / `rustfft` 6.4 | |
| Filters | Our own RBJ biquad | ~50 lines. `biquad` 0.6 is fine too. |
| MIDI devices | `midir` 0.11 | |
| MIDI messages | `wmidi` 4 | No allocation, safe on the audio thread. |
| MIDI files | `midly` 0.5 | Dormant but complete. |
| CLAP hosting | `clack-host` 0.2 | The only working Rust CLAP host layer. Has a cpal example. |
| VST3 hosting | `vst3` 0.3 (coupler-rs) | Raw COM bindings; we write the safe layer. The VST3 SDK is MIT licensed since 3.8 (Oct 2025). |
| AU hosting | `objc2-audio-toolbox`, `objc2-avf-audio` | No mature Rust AU host exists. |

Read, do not depend on: `firewheel` 0.14 (closest Rust design to ours, but its README says it is not a DAW engine), `fundsp` 0.23 (useful DSP building blocks; its `Net::commit()` is the same compile-and-send idea).

Avoid: `nih-plug` (maintenance mode; plugin authoring, which we do not need), `knyst`, `dasp`, `audio-graph` (dormant), `rodio`, `kira` (playback libraries, not engines), Meadowlark code (AGPL).

### Tooling

| Tool | Use for |
| --- | --- |
| `cargo-nextest` | Running tests. `.config/nextest.toml`: `slow-timeout = { period = "60s", terminate-after = 1 }`. |
| `cargo-shear` | Unused dependencies. |
| `cargo-deny` | Licences and advisories. Plugin hosting and bundled code make licences matter. |
| `typos` | Spelling. |
| `bacon` | Background check loop during development. |
| `insta` | Snapshot tests of project JSON and render summaries. |
| `proptest` | Property tests for the graph compiler, the musical clock and state application. |
| `criterion` | Benchmarks for DSP and graph compile time. |
| `miri` | Unsafe and lock-free code. |
| `loom` or `shuttle` | Only if we write our own lock-free structure. Prefer rtrb and triple_buffer instead. |

## 3. Audio engine design

This section answers the open items "Audio graph execution, scheduling and transport notification APIs" and "queue representation" from ARCHITECTURE.md and SDK_SKETCH.md. All studied engines (Pd, Elementary, firewheel, fundsp) converge on the same core idea: edit a graph on a normal thread, compile it to a flat schedule there, and hand that to the audio thread through a lock-free queue. The design below follows them and avoids their known mistakes.

### Threads

| Thread | Owns | May |
| --- | --- | --- |
| UI / main (GPUI) | Views | Edit project state through the editing service. |
| Control | Project graph, compiler, tempo map, asset registry | Allocate, lock, do I/O. Compiles schedules and builds processors. |
| Audio (cpal callback) | Live processors, current schedule, buffers | Only preallocated work. No allocation, freeing, locks, I/O, logging or GPUI calls. |

Control can be the main thread at first if compiles stay small. Move it to a background task when compile time shows up in profiles.

The engine is a value, not global state: `Engine::process_block(&mut self, io, frames)`. The cpal callback, offline rendering and tests all call the same function. Pd needed years to retrofit multi-instance support because it started with globals.

### Messages between threads

Two `rtrb` rings, created once with fixed capacity:

- **Control → audio**: parameter changes, timed events, processor inserts, new schedules, new data snapshots. The audio thread drains it at the start of each block.
- **Audio → control**: anything the audio thread no longer needs (old schedules, removed processors, replaced snapshots) plus reports (xruns, late events, overflowed event buffers). The control thread drops returned objects there. Nothing is ever freed on the audio thread.

Plus a `triple_buffer` for continuous values the UI reads each frame (playhead, meters, CPU load).

A full ring must not lose edits. The control side keeps pending messages and retries next time. Elementary silently drops a schedule when its queue is full; do not copy that.

Every `Arc` the audio thread might release must come back through the return ring. Elementary frees sample buffers on the audio thread in one edge case because it relied on a registry holding a reference.

Send a group of messages for one edit as one batch so they land in the same block (firewheel's `update()` flush). A half-applied edit must never reach the engine: validate and compile the whole change before sending anything. This matches the SDK sketch's "decode and validate before publishing".

### Processors

Two-phase trait, from Pd's `dsp`/`perform` split:

```rust
trait Processor: Send {
    /// Control thread. May allocate. Called before the processor reaches the audio thread,
    /// and again if sample rate or max block size changes.
    fn prepare(&mut self, config: &PrepareConfig);
    /// Audio thread. Realtime safe.
    fn process(&mut self, context: &mut ProcessContext);
}
```

`ProcessContext` gives separate input and output slices per port, the block's sorted events with frame offsets, and transport info. Do not alias input and output buffers the way Pd does; in Rust that is undefined behaviour. The compiler can still reuse buffers whose lifetimes do not overlap.

Processors live in a slot table on the audio thread, indexed by IDs the control side assigns. Schedules refer to slots, not to processors. A routing change sends a new schedule while every surviving processor keeps its phase, envelopes and delay lines. This removes the SDK sketch's fallback of stopping playback on routing edits. New processors arrive as prepared `Box<dyn Processor>` inserts; removed ones go back on the return ring. If the table needs to grow, the control side sends a larger replacement table and gets the old one back.

### Schedule compile

On the control thread, from the typed project graph (instances, ports, connections):

1. Topologically sort. Order independent branches by a stable key such as instance ID, not creation order, so output is reproducible.
2. Reject cycles that do not pass through a declared feedback delay, with a typed error naming the connection. Pd silently drops nodes in a cycle; do not.
3. Assign buffers. Share one buffer for fan-out. Insert a summing step for fan-in. Unconnected inputs read the parameter or a constant.
4. Emit a flat `Vec<Step { slot, inputs, outputs }>`. The audio thread loops over it. Keep the dependency edges in the compiled result so a parallel executor can come later; v0 is single threaded.

One edit group or undo step triggers one compile, like Pd's `canvas_suspend_dsp`. Keep the project graph between compiles. Pd throws it away and rebuilds from scratch with lookups that cost objects × connections.

### Block size and feedback

- Split each device buffer into sub-blocks of at most `MAX_BLOCK = 64` frames (Pd's size). The last sub-block may be shorter. This adds no latency and keeps processor buffers fixed size. Do not skip leftover frames the way Pd does.
- A feedback connection compiles to a write half (sink) and a read half (source) sharing one delay line owned by the delay processor. The sort sees no cycle.
- The minimum feedback delay is always `MAX_BLOCK` frames, independent of sort order and device buffer size. Pd's minimum is 0 or one block depending on invisible sort order, and Elementary's changes with the host block size. Longer delays read further back in the same line. This answers the open "minimum delay and processing granularity" item.
- Loops shorter than a block (for example Karplus-Strong) stay inside one processor.

### Parameters and events

- **Base value edits** from UI or agent: a parameter message applied at the next block start. The state application hook compares previous and next records and sends only changed parameters. Elementary's `createRef` fast path works the same way. The SDK provides smoothing helpers; the core does not smooth.
- **Scheduled events** carry an explicit frame offset within the block. Each processor receives a time-sorted event list per block, the model CLAP and VST3 use. Pd gets sub-sample timing from one shared thread and an implicit logical clock; we have separate threads, so timestamps must be explicit.
- Engine time is a `u64` frame counter. Project position is separate and only advances while playing. Musical time converts through the core tempo map. Integer frames avoid Pd's floating-point time unit tricks.
- Event buffers per port are preallocated with fixed capacity. Overflow is counted and reported, never allocated.

### Arrangement playback and transport

Timeline-driven processors generate their own events on the audio thread. The arrangement extension's processor holds an immutable snapshot of its clips (`Arc`, replaced by message, old one returned). Each block it reads the transport info (playing, position in frames and beats, seek flag, tempo map snapshot) and emits the notes that fall inside the block.

This beats scheduling ahead from the control thread: timing never depends on control thread latency, seek takes effect in the next block, and open-ended projects need no preparation. Transport operations (play, pause, stop, seek) are control → audio messages. The core sets the transport info and notifies processors through a flag in the context. Each tool decides how to respond, as ARCHITECTURE.md already requires.

### Graph change clicks

Processors that survive a recompile keep their state, so most routing edits produce no discontinuity beyond the new routing itself. For v0, accept a hard switch. If clicks become a problem, add a short gain ramp (about 10 to 20 ms) on added and removed connections. Elementary crossfades the whole output on every structural edit, which doubles CPU during the fade; do not copy that.

### Realtime safety checks

- `rtsan-standalone` on all `process` implementations in debug and CI.
- Wrap the callback in `no_denormals`.
- Keep the realtime path free of `Mutex`, `Vec::push` beyond capacity, `Box::new`, `Arc` drops, `String` formatting and logging. Report through the return ring instead.
- Hold an App Nap prevention activity on macOS while the engine runs (Zed's `prevent_app_nap`). Zed's own audio locks and allocates in its callback; that is fine for calls and wrong for a DAW.

### Extension SDK surface

Extensions never touch threads or queues. Through the SDK they:

- register processor types and ports for their tools,
- implement `prepare` and `process`,
- map state changes to parameter messages or graph changes in their state application hook,
- read immutable data snapshots the SDK delivers.

The core owns everything in this section.

## 4. Testing and CI

- DSP, graph compile, clock and project state tests are plain `#[test]` with no GPUI. Offline rendering through `process_block` makes audio behaviour testable: render N frames, assert on samples or snapshot a summary with insta.
- Use `#[gpui::test]` only for views and entities. In GPUI tests use `cx.background_executor().timer(..)`, never `smol::Timer::after`, or `run_until_parked()` fails.
- Property tests: random graphs compile to valid schedules; bars/beats ↔ samples round-trips exactly; any valid record applied on top of any other gives the same state as loading it from empty.
- The gallery snapshot renderer has no test harness, so nextest skips it. Run it with `cargo test -p gallery --test snapshots`.
- CI on macOS first (`.github/workflows/ci.yml`): `cargo fmt --check`, clippy with `-D warnings` via the CI config, `cargo nextest run --workspace`, `cargo shear`, `cargo build --locked`, `typos`, `cargo deny check`, and the forbidden-dependency test. Add Miri for any unsafe code. Add Windows and Linux jobs when we claim support there.

## 5. Rules for agents writing code here

Adapted from Zed's `.rules`. Rules are traps to avoid, not general advice. Add a rule only when it is non-obvious, happened more than once and is actionable.

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
- GPUI: no `cx.notify()` or entity updates inside `render`; no blocking file I/O or sleeps on the UI thread. See the gpui skills in `.agents/skills/`.
- Audio thread: nothing from the realtime list in section 3.
- Check a crate's current version and changelog before using it from memory. symphonia, rubato and cpal changed their APIs in 2026.

## 6. GPUI version

crates.io `gpui` 0.2.2 is from October 2025 and has had no release since. Zed main has moved far ahead: platform code split into `gpui_platform` and per-OS crates, a `RealtimeAudio` executor priority, `#[gpui::property_test]`. `gpui_platform` is not on crates.io. `gpui-pre` 0.3.5 (September 2026) is a community snapshot of Zed main that gpui-component now uses.

Decided September 19, 2026: `gpui` and `gpui_platform` come from Zed git, pinned to a stable release tag's commit (v1.20.2) in the root `Cargo.toml`. `rust-toolchain.toml` matches the toolchain Zed pins at that commit. To upgrade, pick a newer stable tag, update both the rev and the toolchain, fix the build, check `cargo test -p gallery --test snapshots` against the previous PNGs, and update the gpui skills in the same change.

The pinned version also renders windows offscreen (`HeadlessAppContext` plus the Metal headless renderer), so UI checks run without opening a window.

## Sources

- Zed: `Cargo.toml` (profiles, lints), `.cargo/config.toml`, `.cargo/ci-config.toml`, `clippy.toml`, `.rules`, `.config/nextest.toml`, `tooling/xtask/src/workspace.rs`, `crates/gpui_util/src/lib.rs`, `crates/audio/`, `docs/src/development/macos.md`.
- Pure Data: `src/m_sched.c` (scheduler, clocks), `src/d_ugen.c` (graph sort, chain, buffers), `src/g_canvas.c` (`canvas_update_dsp` rebuild), `src/d_delay.c` (`delwrite~`, sort-dependent delay), `src/d_ctl.c` (`vline~`), `src/s_audio_pa.c` (callback, block splitting).
- Elementary: `runtime/elem/Runtime.h` (instructions, schedule swap, `gc`), `runtime/elem/GraphRenderSequence.h`, `runtime/elem/builtins/Feedback.h` (tap pairs), `runtime/elem/builtins/Core.h` (root fades), `runtime/elem/SharedResource.h`, `js/packages/core/src/Reconciler.res` and `Hash.ts` (diffing).
- [firewheel design doc](https://github.com/BillyDM/firewheel/blob/main/DESIGN_DOC.md), [fundsp](https://github.com/SamiPerttu/fundsp), [clack](https://github.com/prokopyl/clack), [vst3-rs](https://github.com/coupler-rs/vst3-rs), [cpal changelog](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md), [rtsan-standalone-rs](https://github.com/realtime-sanitizer/rtsan-standalone-rs), [Symphonia 0.6 migration](https://github.com/pdeljanov/Symphonia/blob/master/docs/guides/migration/0p6.md), [Rust releases](https://github.com/rust-lang/rust/blob/stable/RELEASES.md), [rust-lld default on 1.90](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable).
