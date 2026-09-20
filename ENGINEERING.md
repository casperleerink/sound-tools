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
| File watching | `notify` 8.2 | notify 9 was still a release candidate. No `notify-debouncer-full`: the project reads changed paths again instead of trusting event kinds, and it groups events by a quiet window itself, so a burst is never split. See ARCHITECTURE.md "Project storage". |
| Atomic writes | Our own | Temporary file and rename, no `fsync` (6 ms per file on macOS): a dozen lines in `project/storage.rs`. Power loss is left to git and snapshots. Own writes are known by a fingerprint of the bytes, not by ignoring events. |
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
| Denormals | `no_denormals` 0.3 | Wraps the body of `Engine::process_block`. The function is `unsafe` since 0.3. |
| Realtime checks | `rtsan-standalone` 0.3 | `Engine::process_block` is `#[nonblocking]`. It does nothing unless the build sets `RTSAN_ENABLE=1`; then it aborts on allocations, locks and syscalls. See section 3. |
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

Status, September 19, 2026: built in `crates/core` (`processor.rs`, `graph.rs`, `engine.rs`, `control.rs`, `device.rs`, `clock.rs`, `transport.rs`), except the parts marked "not built yet". The API guide for extension authors is [crates/core/README.md](crates/core/README.md). Where the build took a simpler road than the first design, the text says so and why.

This section answers the open items "Audio graph execution, scheduling and transport notification APIs" and "queue representation" from ARCHITECTURE.md and SDK_SKETCH.md. All studied engines (Pd, Elementary, firewheel, fundsp) converge on the same core idea: edit a graph on a normal thread, compile it to a flat schedule there, and hand that to the audio thread through a lock-free queue. The design below follows them and avoids their known mistakes.

### Threads

| Thread | Owns | May |
| --- | --- | --- |
| UI / main (GPUI) | Views | Edit project state through the editing service. |
| Control | Project graph, compiler, tempo map, asset registry | Allocate, lock, do I/O. Compiles schedules and builds processors. |
| Audio (cpal callback) | Live processors, current schedule, buffers | Only preallocated work. No allocation, freeing, locks, I/O, logging or GPUI calls. |

Control can be the main thread at first if compiles stay small. Move it to a background task when compile time shows up in profiles.

The engine is a value, not global state: `Engine::process_block(&mut self, output)` takes interleaved samples. The cpal callback, offline rendering and tests all call the same function. `Engine::new(config)` returns the control half and the audio half. Pd needed years to retrofit multi-instance support because it started with globals.

### Messages between threads

Two `rtrb` rings, created once with fixed capacity:

- **Control → audio**: one message per edit, a batch (`Vec<Command>`) of processor inserts and removals, processor updates, a larger slot table, a new schedule, transport operations and a new clock. The audio thread drains the ring at the start of each sub-block.
- **Audio → control**: the same batches, coming back. The audio thread applies every command by swapping: the new value goes in, the old value ends up inside the command. So old schedules, removed processors and replaced snapshots ride back inside the batch that replaced them, and `EngineControl::poll` drops them. Nothing is ever freed on the audio thread.

One batch in gives one batch out. The audio thread takes a batch only when the return ring has room. Otherwise the batch waits in the command ring for a later block. This is why a full return ring can never force a drop on the audio thread.

Plus a `triple_buffer` for values the control side or UI reads at its own pace. Today it carries `EngineStatus`: blocks and frames processed, edits applied, event overflows, port handle misuses, full-ring counts, whether the project plays and the playhead in frames and ticks. Meters join it later.

Simpler than first designed: reports are not messages on the return ring. They are counters that only grow, published through the triple buffer. Reading the latest value never misses a count, and the return ring keeps its one in, one out rule. Device xruns and late callbacks are counted the same way in `OutputStream::status`.

A full ring must not lose edits. `EngineControl` keeps batches that did not fit and sends them, in order, on the next `poll` or commit. When the `Engine` is dropped, `poll` returns `EngineStopped` and the waiting batches are dropped. Elementary silently drops a schedule when its queue is full; do not copy that.

Every `Arc` the audio thread might release must come back through the return ring. Elementary frees sample buffers on the audio thread in one edge case because it relied on a registry holding a reference.

One edit is one batch, so its changes land in the same block (firewheel's `update()` flush). `EngineControl::edit()` collects changes on a copy of the graph and `commit()` compiles before it sends. A half-applied edit must never reach the engine: validate and compile the whole change before sending anything. This matches the SDK sketch's "decode and validate before publishing".

### Processors

Two-phase trait, from Pd's `dsp`/`perform` split:

```rust
trait Processor: Send + 'static {
    /// The one message type the control side sends to this processor.
    type Update: Send + 'static;
    fn ports(&self) -> Ports;
    /// Control thread. May allocate. Called before the processor reaches the audio thread.
    fn prepare(&mut self, config: &PrepareConfig);
    /// Audio thread, at a block start. Copy or swap values out of `update`, never drop them.
    fn update(&mut self, update: &mut Self::Update);
    /// Audio thread. Realtime safe.
    fn process(&mut self, context: &mut ProcessContext);
}
```

Calling `prepare` again after a sample rate change is not built yet. The engine is created for one device configuration.

`ProcessContext` gives separate input and output slices per port, the block's sorted events with frame offsets, the engine time of the block and the transport info (see "Arrangement playback and transport"). Do not alias input and output buffers the way Pd does; in Rust that is undefined behaviour. The compiler can still reuse buffers whose lifetimes do not overlap.

Processors live in a slot table on the audio thread, indexed by IDs the control side assigns. Schedules refer to slots, not to processors. A routing change sends a new schedule while every surviving processor keeps its phase, envelopes and delay lines. This removes the SDK sketch's fallback of stopping playback on routing edits. New processors arrive prepared and boxed; removed ones go back on the return ring. When the table is full the control side sends a table of twice the size in the same batch, the audio thread moves the processors over, and the old table goes back.

### Schedule compile

On the control thread, from the typed project graph (instances, ports, connections):

1. Topologically sort. Order independent branches by a stable key such as instance ID, not creation order, so output is reproducible.
2. Reject cycles that do not pass through a declared feedback delay, with a typed error naming the connection. Pd silently drops nodes in a cycle; do not.
3. Assign buffers. Every output port gets one buffer, which all its connections read (fan-out). Before a step runs, the engine sums the sources of each audio input into a scratch buffer and merges the sources of each event input into that port's event buffer (fan-in). Unconnected inputs are silent. Sources are summed in schedule order, so results do not depend on the order connections were made in.
4. Emit a flat `Vec<Step>` with the slot, the source buffers per input and the output buffer ranges. The audio thread loops over it. The source lists are the dependency edges a parallel executor needs later; v0 is single threaded. The schedule owns its buffers, so they are allocated at compile time and travel back with the old schedule.

Simpler than first designed: inputs are copies, and buffers are not reused between steps. This costs one 64 sample copy per connection and 256 bytes per output port. In return no input can alias an output, so the engine needs no `unsafe` for buffers. Reuse can come later inside `compile` without touching processors.

Feedback delays are not built yet, so step 2 rejects every cycle. The error names a connection on the cycle.

One edit group or undo step triggers one compile, like Pd's `canvas_suspend_dsp`. Keep the project graph between compiles. Pd throws it away and rebuilds from scratch with lookups that cost objects × connections.

### Block size and feedback

- Split each device buffer into sub-blocks of at most `MAX_BLOCK = 64` frames (Pd's size). The last sub-block may be shorter. This adds no latency and keeps processor buffers fixed size. Do not skip leftover frames the way Pd does. Edits are taken at every sub-block start, so an edit waits at most 64 frames whatever the device buffer size is.
- Not built yet, the next three points. A feedback connection compiles to a write half (sink) and a read half (source) sharing one delay line owned by the delay processor. The sort sees no cycle.
- The minimum feedback delay is always `MAX_BLOCK` frames, independent of sort order and device buffer size. Pd's minimum is 0 or one block depending on invisible sort order, and Elementary's changes with the host block size. Longer delays read further back in the same line. This answers the open "minimum delay and processing granularity" item.
- Loops shorter than a block (for example Karplus-Strong) stay inside one processor.

### Parameters and events

- **Base value edits** from UI or agent: a typed `Processor::Update` message applied at the next block start. The same message type carries data snapshots and "do this now" events from the UI, so the core has one control-to-processor path. A tool's behaviour sends the current values on every state application, which costs no compile. Elementary's `createRef` fast path works the same way. The SDK provides smoothing helpers; the core does not smooth.
- **Scheduled events** travel between processors through typed event ports and carry an explicit frame offset within the block. Any `Copy + Send + 'static` type is an event; the core moves it without knowing it. `Copy` rules out allocation and drops on the audio thread. Port handles carry the event type, and `connect` rejects two ports with different types. Each processor receives a time-sorted event list per block, the model CLAP and VST3 use. Not built: events scheduled from the control thread for a future frame. Timeline-driven processors make their own events on the audio thread (next heading), which covers the milestone. Pd gets sub-sample timing from one shared thread and an implicit logical clock; we have separate threads, so timestamps must be explicit.
- Engine time is a `u64` frame counter. Project position is separate and only advances while playing. Musical time converts through the core tempo map. Integer frames avoid Pd's floating-point time unit tricks.
- Event buffers per port are preallocated with fixed capacity (`EngineConfig::event_capacity`, per block). Overflow is counted in `EngineStatus::event_overflows`, never allocated. `push` tells the sender whether the event fitted, so a sender keeps a note off that did not fit and sends it in the next block. A processor counts an event it dropped for a full list of its own with `count_dropped`, into the same counter.

### Arrangement playback and transport

Built in `transport.rs` and `clock.rs`, and in `extensions/arrangement/src/sequencer.rs`, whose README has the rules of the sequencer.

Timeline-driven processors generate their own events on the audio thread. The arrangement extension's processor holds an immutable snapshot of its clips (`Arc`, replaced by an update message with `std::mem::swap`, old one returned). Each block it reads the transport info (`ProcessContext::transport`: playing, the block's range in project frames and in ticks, the jump flag, the clock) and emits the notes that fall inside the block.

This beats scheduling ahead from the control thread: timing never depends on control thread latency, seek takes effect in the next block, and open-ended projects need no preparation. Transport operations (play, pause, stop, seek) are control → audio messages, applied at a sub-block start like every other command. The core sets the transport info and notifies processors through two flags in the context, each set for one block: `jumped` after a seek or a stop, and `stopped_playing` when the previous block played and this one does not. A processor with held notes releases them on either flag, because while not playing the ranges are empty and nothing else would end them. The second flag is in the core so that not every processor keeps its own copy of the previous playing state. Each tool decides how to respond, as ARCHITECTURE.md already requires.

The transport state on the audio thread is small: playing, whether the previous block played, the project position in frames, the jump flag and an `Arc<Clock>`. Each sub-block gets the frame range `position..position + frames` (empty while not playing) and the tick range `tick_at(start)..tick_at(end)`. Both ends come from the same pure function, so the end of one block is the start of the next by construction, and every tick belongs to exactly one block. Nothing is cached between blocks. The cost is two binary searches over the tempo changes and two 128 bit divisions per sub-block.

A tempo map change is a new `Arc<Clock>`, compiled on the control thread. The audio thread reads the next tick from the old clock, swaps the clocks, and moves the frame position to that tick's frame in the new clock. So the tick sequence goes on with no gap and no repeat, and `jumped` stays false. The frame position does change, by design. The old clock rides back in the batch. The control side keeps the same `Arc` for its own conversions. `set_tempo_map` with a map equal to the current one sends nothing, so a project file saved again unchanged does not move the frame position.

### Musical clock

Decided and built September 19, 2026, in `clock.rs`. The rules behind "rounds in one place":

- `Clock::frame_of(tick)` is the one conversion. A tick lands on the frame that contains its exact time: the exact position rounded down. `tick_at(frame)` is its inverse, the first tick at or after a frame. Rounding down makes that inverse a plain ceiling division. The error is below one frame and the same everywhere.
- All math is integers. A tempo is held in steps of 0.001 bpm, so frames per tick is the fraction `sample_rate * 60000 / (milli_bpm * 960)`. Products use `u128`, so positions far beyond any real project stay exact.
- Each tempo change starts a segment on the whole frame of its own tick. Math inside a segment then starts from an integer, and a segment is found with a binary search by tick or by frame. The start is rounded down like any tick, so each tempo change can move the ticks after it early by less than one frame. That is far below what anyone hears, and every conversion uses the same clock, so all parts still agree on the frame of a tick.
- Tempo bounds are 10 to 1000 bpm. With the 1000 bpm limit a tick is at least one frame long from 16000 Hz up (`MIN_EXACT_SAMPLE_RATE`). Then no two ticks share a frame and `tick_at(frame_of(tick)) == tick`. 44100, 48000 and 96000 Hz are tested. `OutputDevice` refuses devices below 16000 Hz. Offline engines below it still run and never panic, but two ticks can share a frame there, and a tempo map change can repeat a tick.
- Seconds go through frames (`seconds_of`, `tick_at_seconds`), so they agree with the audio.
- Bars and beats need only the time signature: `TimeSignature::bar_beat_of` and `ticks_of`. Denominators 1 to 32 all divide the 3840 ticks of a whole note, so a beat is a whole tick count.

Saved form: `TempoMap` derives serde and validates while loading. Step 3 puts it in `project.json`.

```json
{
  "time_signature": "4/4",
  "tempo_changes": [
    { "tick": 0, "bpm": 120.0 },
    { "tick": 15360, "bpm": 93.5 }
  ]
}
```

Simpler than planned: the time signature is saved as a string. It reads well, and the validated type needs no second unvalidated struct for serde. `Engine::new` stays infallible: it takes any sample rate and the device wrapper checks the limit, instead of a validated sample rate type through every config.

### Graph change clicks

Processors that survive a recompile keep their state, so most routing edits produce no discontinuity beyond the new routing itself. For v0, accept a hard switch. If clicks become a problem, add a short gain ramp (about 10 to 20 ms) on added and removed connections. Elementary crossfades the whole output on every structural edit, which doubles CPU during the fade; do not copy that.

### Realtime safety checks

- `rtsan-standalone`: `Engine::process_block` is `#[nonblocking]`, which covers every `update` and `process` it calls. The crate reads `RTSAN_ENABLE` in its build script and does nothing without it. Run `RTSAN_ENABLE=1 cargo nextest run -p sound-core -p tone -p instrument -p sound-notes -p arrangement`. Add every new crate with a processor to this list and to the CI step. CI does, as its last step. This is the one allowed exception to "never vary environment variables": it rebuilds only the sanitizer and our audio crates. One test starts a child that allocates inside `process` and expects the sanitizer to abort it, so a sanitizer that is silently off fails CI.
- `no_denormals` wraps the body of `process_block`, so offline renders and the device callback compute the same.
- Keep the realtime path free of `Mutex`, `Vec::push` beyond capacity, `Box::new`, `Arc` drops, `String` formatting and logging. Report through the return ring instead.
- Not built yet: hold an App Nap prevention activity on macOS while the engine runs (Zed's `prevent_app_nap`). It belongs with the application window. Zed's own audio locks and allocates in its callback; that is fine for calls and wrong for a DAW.

### DSP speed

Decided with the synth, September 19, 2026:

- Measure a DSP crate as a whole project: 100 instances in one engine, rendered offline in the dev profile, as a realtime ratio. `extensions/instrument/tests/synth/performance.rs` is the pattern. An idle processor must return before it touches its output.
- A recursive filter is bound by the chain of operations from one frame to the next, not by the count of operations. Multiplying out the state update of the synth's filter made the chain half as long and a voice 1.4 times faster. It is still the same filter.
- Rust never fuses a multiply and an add by itself. Do not reach for `f32::mul_add`: it is one instruction on Apple Silicon, and a slow library call on x86 without the FMA feature.
- Work out filter factors (`tan`, `powf`) once per block and only while a parameter moves, never per frame. Smooth the parameter, not the factors.

### Extension SDK surface

Extensions never touch threads or queues. Through the SDK they:

- register processor types and ports for their tools,
- implement `prepare`, `update` and `process`,
- map state changes to update messages or graph changes in their state application hook,
- read immutable data snapshots delivered as update messages.

The core owns everything in this section. It is built and described in [crates/core/README.md](crates/core/README.md): processors, and tools with their state application hook, which the code calls a behaviour. The hook gets the next state only, not the previous one. It declares what the instance needs and the core sends the difference, so a behaviour sends its few parameters every time instead of comparing records. A behaviour has nowhere to keep a previous state. If rebuilding a large snapshot ever shows up in a profile, the core can hand behaviours the previous records then.

### Device output

`OutputDevice::default_output()` opens the default device with its default configuration, f32 samples only. `start(engine)` moves the engine into the cpal callback. It refuses an engine built for another channel count or another sample rate than the device has. Device selection, audio input and sample formats other than f32 are not built yet. `cargo run -p runtime -- <project-folder>` opens the window: it plays a project folder on the device and keeps it live. It creates the default project of the arrangement extension in an empty folder. When the window closes it prints the device counters. `--headless` does the same without a window and without starting GPUI, for tests, CI and agents. It prints every change it applies and, at the end, the counters. It reads `play`, `pause`, `stop`, `seek <ticks>`, `undo`, `redo`, `status` and `quit` as lines on stdin. This is provisional and not the outer application protocol. `--inspect` prints a summary without a device and without the project lock. A tool with a registered summary, such as the arrangement, describes what it owns there. The runtime is a library plus a thin binary, so tests of whole projects use the same registry, summary and render. `--render <wav> --seconds <n>` renders offline. The earlier hard-coded scenario with its beat click is gone. `tests/transport.rs` covers what it checked.

## 4. Testing and CI

- Project folder tests call `Project::apply_outside_changes` with explicit paths, the function the watcher calls, so they do not depend on timing. One test uses the real watcher, with long timeouts. The scale test (10,000 child records) is `#[ignore]`: `cargo nextest run -p sound-core --run-ignored only ten_thousand --no-capture`.
- DSP, graph compile, clock and project state tests are plain `#[test]` with no GPUI. Offline rendering through `process_block` makes audio behaviour testable: render N frames, assert on samples or snapshot a summary with insta.
- Use `#[gpui::test]` only for views and entities. In GPUI tests use `cx.background_executor().timer(..)`, never `smol::Timer::after`, or `run_until_parked()` fails.
- Clippy's `allow-unwrap-in-tests` covers `#[test]` functions only. A file under `tests/` with helper functions starts with `#![allow(clippy::unwrap_used)]`.
- Property tests: random graphs compile to valid schedules; ticks ↔ frames and ticks ↔ bars/beats round-trip exactly; a processor that emits from the transport info fires every tick exactly once over random tempo maps, device buffer sizes, a pause and a tempo map change; any valid record applied on top of any other gives the same state as loading it from empty.
- The two snapshot renderers have no test harness, so nextest skips them. `cargo test -p gallery --test snapshots` renders the components and `cargo test -p runtime --test snapshots` renders the application window for three projects and prints frame times. Both run in CI, which has no display. Look at the PNGs after a UI change.
- Window behaviour is tested with `#[gpui::test]` and simulated keys and clicks (`crates/runtime/tests/window.rs`), on an offline engine that the test runs by hand with `process_block`.
- CI on macOS first (`.github/workflows/ci.yml`): `cargo fmt --check`, clippy with `-D warnings` via the CI config, `cargo nextest run --workspace`, `cargo shear`, `cargo build --locked`, `typos`, `cargo deny check`, the forbidden-dependency test, and the realtime sanitizer run from section 3. Add Miri for any unsafe code. Add Windows and Linux jobs when we claim support there.

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
