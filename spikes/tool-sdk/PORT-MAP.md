# Port map: GPUI → Rune (+ tool SDK v2)

For agents. Repo: `sound-tools` at `t3/rune-rendering-spike` (daff08df). Rune rev `7a92d24`
(checkout `~/.cargo/git/checkouts/rune-c9b805e9ed8a3730/7a92d24`). All line counts are `wc -l`
of whole files that mention `gpui`, so they include tests, docs and non-UI code in those files.

Decided (not reopened here): the window moves to Rune; tools of the project get a new TS view
SDK: `view(state)` re-run on change with keyed `memo`, free layout from primitives, theme
tokens in `theme.json`, components as source in the project's `ui/`, a behaviour library
(value drag, selection, grid move/resize, menus, typeahead), and an agent snapshot/act where
every control reports label, state and actions.

---

## 0. Headline facts (verified in this research)

- **~85,000 lines touch gpui** in 201 files (matches the spike): runtime 30.7k (src 9.1k,
  tests 21.6k), ui 16.5k (src 14.8k, tests 1.6k), typescript 14.4k, extensions 12.0k,
  agent 8.8k, gallery 2.9k.
- **gpui and Rune live in one workspace.** A scratch crate depending on gpui (our rev, our
  features) + `rune` (tooling) + `rune-kit` + Rune's 5 `[patch.crates-io]` entries resolves,
  type-checks **and links** on macOS (`/tmp/coexist`). No version conflict: duplicates are
  separate semver majors (accesskit 0.24/0.25, accesskit_macos 0.26/0.27, taffy 0.13/0.15,
  objc2 0.5/0.6, objc2-app-kit 0.2/0.3) or different sources (gpui's wgpu is a **git** wgpu
  30.0.0, Rune's is crates.io 30.0.2; Rune's wgpu-hal patch does not touch gpui's). winit and
  parley are only Rune's. So **no hard switch is needed**.
- **Rune builds on our 1.98.1.** `cargo +1.98.1 build` of that scratch crate (rune with
  `tooling`, default features, rune-kit) links. The spike's "1.98.1 is too old" did not
  reproduce for Rune used as a library; 1.99.1 is only Rune's own workspace pin. gpui also
  builds on 1.99.1. Either toolchain works; stay on 1.98.1 until gpui is gone.
- **Plugin windows are not a hard gap.** Rune has `rune::open_window(WindowSpec)` at run time,
  `WindowHandle::with_raw_handle` (raw-window-handle 0.6 → AppKit `ns_view` / Win32 `hwnd`),
  `set_position`, `resize`, `placement()`, `on_close`. The spike's "no public NSView" is wrong
  at this rev. What is missing: a moved/resized callback (poll `placement()` instead) and proof
  that a child NSView over Rune's CAMetalLayer view gets drawn and keyed correctly → a small
  spike (workstream F).
- **Bun side is mostly UI-free already.** In `crates/typescript`, `bun.rs`, `tools.rs`,
  `behaviours.rs`, `feeds.rs`, `timeline.rs`, `recorder.rs`, `touch.rs`, `records.rs`,
  `lane.rs`, `region.rs`, `fader.rs`, `live.rs` (~8,500 lines) have no gpui. The gpui parts are
  the window glue (`window.rs` 2,677), the TSX renderer (`card/` 6,720, `tree.rs` 2,656,
  `region_card.rs` 457, `tokens.rs` 165) and `lib.rs` key contexts.
- **Opening a project is UI-free**: `runtime::open_with_extensions`, `registry`,
  `playing_plugins`, `OFFLINE`, `tick`, `render*` in `crates/runtime/src/lib.rs`. The gpui
  seam is `sound_ui::Session` (an `Entity` that owns `Project` and polls every 16 ms).

---

## 1. Inventory of gpui use

### 1.1 `crates/runtime` — the app and the window (src 9,089 + tests 21,589)

| File | Lines | What it does for the composer | Port note |
| --- | --- | --- | --- |
| `src/main.rs` | 937 | CLI: window forms, `--headless`, `--inspect`, `--render`, `--analyze`, `--screenshot [--click x,y]… [--play s]`, `--save-template`, `--plugins`. | Only the window forms and `--screenshot` touch gpui. Keep the rest. |
| `src/lib.rs` | 936 | Registry of bundled extensions, `views()` registration of every device view and offer, `agent_panel`/`sidebar_panel`, open/inspect/render helpers. | `views()`, `agent_panel`, `sidebar_panel` are gpui; the rest stays. |
| `src/screenshot.rs` | 260 | `--screenshot`: copy folder, offline engine, gpui `HeadlessAppContext`, clicks by x,y, play N s, PNG. macOS only. | **Delete**; Rune `--snap/--tree/--script` replace it, on every OS. |
| `src/window.rs` | 1,619 | `Shell`: title row, main instance view, left panel (Agent / History tabs), notices bottom right, page fades, key bindings, open/quit. | Rewrite as the Rune shell. |
| `src/window/app_keys.rs` | 264 | App actions (undo, redo, open, new, export, settings…) → keymap + menu keys. | Logic table reusable; binding → Rune `on_key` + `Chord`. |
| `src/window/audio_input.rs` | 288 | Opens the audio input while armed/connected, level for meters, take writer. | 7 gpui lines; lift to UI-free core. |
| `src/window/live_input.rs` | 483 | MIDI keyboard, computer keys, audio input into the project. | 81 gpui lines (entities, executor); lift to core. |
| `src/window/recorders.rs` | 702 | Takes of `recorder`s → `timeline`/`region` records + assets, `onTake`. | 56 gpui lines; lift to core. |
| `src/window/other_apps.rs` | 216 | macOS process taps for "sound of other apps". | 34 gpui lines; lift to core. |
| `src/window/history.rs` | 1,026 | History tab: version cards, go to point, rename, merge. | Rewrite view; data from `Project` history is UI-free. |
| `src/window/project_menu.rs` | 801 | Project name menu: export, undo/redo, projects, templates, reveal folder, Settings, version. | Rewrite (rune-kit Menu or native popup). |
| `src/window/problems.rs` | 108 | Warning sign + list of problems. | Rewrite (small). |
| `src/window/export_settings.rs` | 269 | Small window: format, rate, depth, loudness → save panel. | Rewrite; second Rune window or a modal. |
| `src/window/new_project.rs` | 228 | Template picker window. | Rewrite. |
| `src/window/save_template.rs` | 201 | Save as template window. | Rewrite. |
| `src/window/start.rs` | 250 | No folder: open last project or folder panel; new from template. | Rewrite; folder dialog via `tx.open_folder`. |
| `src/window/settings.rs` + `settings/shortcuts.rs` | 438 + 386 | Settings page: Agents, Audio (devices), Shortcuts (rebind). | Rewrite (rune-kit `shell::Page`, `ShortcutRecorder`). |
| `src/update.rs` | 614 | Self-update from GitHub releases. | 7 gpui lines (executor); lift to core. |
| `src/app.rs`, `log_file.rs`, `recorder.rs`, `report.rs`, `templates*.rs` | — | Support folder, log, recording threads, summaries, templates. | No gpui. Keep. |
| `tests/window/**` | 18,189 | `#[gpui::test]` window behaviour on an offline engine with simulated mouse/keys: tools (song view 2,255, gestures 1,497, tools 1,993, parts, rack, timeline, recorder, mixer, piano roll…), each device card, agent 1,633, history, shell, plugin card. Plus render budgets (`tools/scenarios.rs`, `scenarios/measure.rs`) for `tooling/perf/ui.py`. | Port the **rules** they check, not the code: see workstream H. |
| `tests/snapshots.rs` + `snapshots/*` | 3,400 | Offscreen PNGs of window states (`WINDOW_SNAPSHOT_ONLY`): look, keys, takes, timeline, typescript, wavetable, agent, history, cards. | Replace by `--snap` goldens of scripted states. |
| `tests/projects/**`, `shared/`, `sdk-gallery/` | — | Whole-project tests with Bun, no gpui. | Keep; they guard sound and behaviours through the port. |

### 1.2 `crates/ui` — tokens, components, bridge (src 14,825, tests 1,649)

| Area | Files (lines) | What it is | Port note |
| --- | --- | --- | --- |
| Bridge | `session.rs` 580, `control_edit.rs` 182, `device_edit.rs` 121, `metering.rs` 248, `recording.rs` 140, `lanes.rs` 164, `render.rs` 135, `import.rs` 68, `waveforms.rs` 228, `global_keys.rs` 149, `spare_keys.rs` 33, `focus.rs` 109 | `Session` entity (project, poll, gestures, notices, playhead/loop entities, click, automation writing, render progress), drag edits, device edits, peaks per poll, live takes, automation lanes over a record, background render, file import, waveform cache, global keys. | Logic moves to a UI-free core (`Session` as a plain struct with `poll() -> Changes`); gestures become window-side behaviours. |
| Registries | `views.rs` 184, `devices.rs` 457 | `Views` (tool → view/card), `Devices` (offers, labels, picker groups, plugin numbers). | `Devices` is ~95% data; strip gpui, keep. `Views` becomes `tool → fn(&State) -> El` (Rune). |
| Tokens | `theme.rs` 286, `typography.rs` 183, `scale.rs` 58, `names.rs` 71, `assets.rs` 145 (Inter Display ×4, 85 SVG icons) | Colour roles, text variants, sizes, radius, fonts, icons. | **One `theme.json`** shared by Rust cards and the TS SDK; fonts via `AppSpec::fonts`; icons via Rune `svg`/`Icon`. |
| Components | `components/*` (31 files): `text_input` 1,741+232, `dropdown_menu` 1,505, `display` 1,342, `knob` 835, `meter` 497, `device_card` 493, `gesture` 443, `curves` 342, `waveform_display` 323, `button` 319, `toggle` 271, `select` 260, `segmented_control` 215, `pad` 201, `limiter_display` 201, `split_button` 188, `notice` 187, `dynamics_display` 164, `slider` 158, `cell` 156, `kbd` 121, `paint` 124, `indicator` 109, `popover` 109, `tooltip` 86, `empty_state` 68, `icon` 46, `automated` 31 | The shared look of every card and page. | Many have rune-kit equivalents (TextInput, Menu/Select, Button, Switch, SegmentedControl, Tooltip, Popover, Keybind). Audio-specific ones (knob, meter, display, curves, waveform, pad, dynamics/limiter displays) are rewritten on Rune shapes/canvas + `Live`. |
| `shortcuts.rs` | 456 | Keymap: actions, defaults, composer's own keys file. | 1 gpui line; keep. |
| `lib.rs` | 61 | `OpenSettings` action, `init`. | Rewrite. |
| tests | 1,649 | text input, controls, dropdown menu, bridge. | Most become rune-kit's job; bridge tests move with `Session`. |

### 1.3 `crates/typescript` — tools of the project (gpui part 14,446 of 23,133)

| Part | Files (lines) | What it does | Port note |
| --- | --- | --- | --- |
| Window glue | `window.rs` 2,677 | `Live`: one per window; runs Bun, watches `extensions/`, restarts on change/crash/hang, tells records/transport/song/watches, runs control loops every 33 ms, routes edits/controls/notes/sends/renders/menus/offers/file questions, cards and pages, takes, MIDI taps. | Rewrite UI-free as a `ToolsHost` driven by `poll(now)`; keep the message flows. Biggest single logic port. |
| TSX renderer | `card.rs` 1,056 + `card/*` (controls 560, drawing 608, edges 149, in_view 197, input 1,230, paint 587, parts 423, placed 183, shapes 1,093, space 647, steps 455, sync 588), `tree.rs` 2,656, `region_card.rs` 457, `tokens.rs` 165 | Applies Solid-style ops into a node table and draws it with gpui: cards, pages, spaces, shapes, steps, knobs/faders/meters, pointer input, scroll fades. | **Replaced** by the new tree → `El` renderer (spike: 725 lines). Delete after switch. |
| Key glue | `lib.rs` (key_context, actions_on, takes_key) | gpui `KeyContext`/`Keystroke`. | Rewrite on Rune `KeyEvent`/`Chord`. |
| SDK (TS) | `sdk.ts` 100 + `sdk/*` + `host.ts` 1,462 = 6,348 | Fields, signals, tools, live, reactivity (532), nodes (481), drawing (496), components (932), page (195), card (120). | Keep fields/signals/tools/sound/notes/host core. Replace reactivity/nodes/drawing/components/page/card and the view half of live with SDK v2 (`view`, `memo`, `change`, primitives, behaviours). |
| Agent docs | `extensions*.md`, `fader.md`, `lane.md`, … (6,328 words) | What an agent reads to write tools. | Rewrite for v2 (spike doc: 422 words). |
| UI-free | `bun.rs` 1,352, `tools.rs` 2,366, `behaviours.rs`, `feeds.rs`, `timeline.rs`, `recorder.rs`, `touch.rs`, `records.rs`, `lane.rs`, `region.rs`, `fader.rs`, `live.rs`, `samples.rs`, `path.rs` | Process, protocol, tool registration, owners' behaviours, note feeds, built-in children, automation touch. | Keep. Protocol grows v2 view messages. |

### 1.4 Extensions' views and cards (12,003 lines)

| Extension | gpui file(s) | Lines | Card content |
| --- | --- | --- | --- |
| analyzer | `view.rs` | 207 | Spectrum display. |
| compressor | `view.rs` | 381 | Dynamics display + knobs. |
| delay | `view.rs` | 447 | Knobs, sync choices, display. |
| drum-pad | `view.rs` | 509 | Pads, samples, file drop. |
| eq | `view.rs` | 425 | Response curve with band handles. |
| filter | `view.rs` | 260 | Response curve + handle, type segments, knobs, expand. Reference card. |
| gate | `view.rs` | 202 | Knobs + display. |
| instrument | `view.rs` | 249 | Synth knobs. |
| limiter | `view.rs` | 235 | Limiter display + knobs. |
| modulation | `view.rs` | 271 | Knobs + choices. |
| pitch-correction | `view.rs` | 362 | Scale/key + knobs. |
| reverb | `view.rs` | 343 | Knobs + display. |
| sampler | `view.rs` | 857 | Waveform, zones, file import. |
| saturator | `view.rs` | 237 | Curve + knobs. |
| utility | `view.rs` | 396 | Gain/width/pan. |
| vocoder | `view.rs` | 148 | Knobs. |
| wavetable | `view/{mod 1,114, matrix 230, choices 330, drawing 111}` | 1,785 | Table display, mod matrix, choices. |
| plugin-host | `view.rs` 771, `window.rs` 528, `host/windows.rs` 377, `host.rs` (6 lines: `WindowHandle`), `backend.rs`/`vst3/view.rs` (`Keystroke`) | 4,689 (gpui-ish ~160) | Plugin card (params, open window), the plugin's own window (empty gpui root; plugin view a child of its NSView/HWND), placement saved per machine, keys the plugin does not take. |
| plugin-host tests | `tests/plugin_host/{window 625, remembered 283, resizing_and_keys 203}` | 1,111 | Window open/close/placement/resize/keys. |
| metronome, midi, tone | — | 0 | No view. |

Pattern for most cards: `ParameterKnob` consts over `Parameter`s + `DeviceEdit` + `CardFrame`, sometimes a `Display`. Many are close to "a knob per number field".

### 1.5 `crates/agent` — sidebar (gpui 8,772 of 15,178)

- `view/*` 6,873: sidebar (1,054 + threads, turn, naming, went_back, attachments), markdown (1,447),
  onboarding (941), agents page (803 + 158), model picker (496), entry (511), instructions
  (288), menu (167), history (110).
- gpui also in UI-free-ish files: `lib.rs`, `settings.rs`, `environment.rs`, `install.rs`,
  `conversation.rs` (entities/executor), `provider/claude/process_tests.rs`.
- Providers (`provider/**`), store, process, naming: no gpui. Keep.
- rune-kit fits well: `Markdown` (streaming, code, tables), `TextArea` with pills, `Select`,
  `Combobox`, `Menu`, `shell::Sidebar`, `Toast`.

### 1.6 `crates/gallery` (2,878, all gpui)

Storybook of `sound_ui` components + snapshot test. **Delete**: Rune has `App::playground`
(⌘⇧D) and `--snap --scene`; a component story can be a scene.

### 1.7 Screenshot / click tooling and tests

- `runtime --screenshot <png> [--click x,y]… [--play s]` (`src/screenshot.rs`), used by agents
  and `tooling/agent-eval/eval.py` (prompts at lines ~630–646). Clicks by coordinates; no tree,
  no keys, no right-click (the agent-eval REPORT lists these as unverifiable criteria).
- `#[gpui::test]` count by file (top): song_view 55, tools 41, gestures 40, agent 34,
  text_input 24, parts 17, rack 14, timeline 12, plugin window 11, recorder 11, edits 11,
  filter 11 …
- `clippy.toml` disallows `smol::Timer::after` "for GPUI tests" → drop at switch.

### 1.8 `tooling/perf` and agent eval

- `tooling/perf/ui.py` (96) runs `runtime --test window tools::scenarios::measure` with the
  `perf` feature (gpui `bench-support`, `bench_metrics`, `criterion`, `gpui_apple`; profile
  `perf` turns gpui debug assertions on). Rewrite against Rune: `--native --trace`,
  `frame-profile` feature, or the spikes' `measure.py` (process CPU / fps in real windows).
- `tooling/perf/bench.py window` measures the window process (CPU, memory) — keeps working on
  any binary.
- `tooling/agent-eval` (`eval.py ui`): builder + QA agents; replace `--screenshot/--click`
  guidance with `--snap/--tree/--script/--play` and `bun test`.

### 1.9 Workspace config that names gpui

`Cargo.toml` (gpui, gpui_platform, gpui_apple, bench_metrics deps; `[profile.dev.package]`
gpui/gpui_macros/taffy/resvg/lyon opt-levels; `[profile.perf.package.gpui*]`), `rust-toolchain.toml`
("match Zed"), `deny.toml` (advisories through gpui, git sources), `clippy.toml`,
`ENGINEERING.md` ("GPUI version", gpui rules, `#[gpui::test]` rules, main thread table),
`ARCHITECTURE.md` ("Shape of the system" GPUI line, "The window" section), `.agents/skills/gpui-*`,
`crates/ui/README.md`, CI apt packages (`.github/workflows/ci.yml:104`, `release.yml:111`).

---

## 2. The seams: gpui ↔ non-UI code, and what Rune gives

| Seam | Today (gpui) | Rune at 7a92d24 | Gap / plan |
| --- | --- | --- | --- |
| **Project ownership + poll** | `Session` entity owns `Project` on the main thread; `every_poll` timer 16 ms; emits `ProjectEvent`; playhead/loop/recording are child entities. | One app state `S`; `AppSpec::poll(&mut S, secs, &mut Tx) -> bool` at frame start; `Waker`/`AppHandle::update` from threads; `wake_after`; hidden windows still run `poll`/updates. | Make `Session` a plain struct (`poll() -> what changed`). While playing or Bun busy, a timer thread wakes at ~60 Hz; idle draws nothing. Playhead/meters/time are `Live`/`LiveText` set in `poll` (no view run). |
| **Engine** | `EngineControl` via Session; status triple buffer; `Peaks`. | — (pure Rust). | None. Headless: step the offline engine by Rune's virtual clock in `poll`. |
| **Bun host** | `Live` (gpui entity, executor tasks, 33 ms frame timer, `notify` watcher, `prompt_for_paths` for code asking a file). | Threads + `Waker`; `tx.open_file/save_file`; `tx.popup_menu`. | UI-free `ToolsHost`; reader thread wakes Rune. Headless needs a **settle**: before each virtual frame, wait (bounded) until Bun answered what it was told, so `--snap` is deterministic. |
| **Gestures / drags** | `DragEdit`/`DeviceEdit` publish per move into the session gesture; one undo step. | `on_drag(|s, drag, tx|)` with phases; `Tx::instant()`. | Behaviour library on the window side, calls the same `Session::gesture` API. |
| **Plugin windows** | One gpui window per plugin, empty root; plugin NSView/HWND child via `HasWindowHandle`; `observe_window_bounds` saves placement; unhandled keys forwarded (`SpareKeys`). | `rune::open_window(spec)` at run time; `WindowHandle::with_raw_handle` → `ns_view`/`hwnd`; `set_position`, `resize`, `placement()`, `on_close`, `on_window` for native styles. | No moved/resized callback (poll `placement()` on a slow timer and on close). Unproven: child NSView over Rune's Metal layer (z-order, first responder, keys back to app). Spike first (F). Linux: none, as today. |
| **Menus** | In-app dropdown menus (`dropdown_menu.rs`), no native menu bar. | Native `tx.popup_menu` (macOS NSMenu, Windows TrackPopupMenu; **not Linux**), headless `menu-pick`; rune-kit `Menu`/`context_menu` (in-app, all OS); native app menu bar (macOS). | Use rune-kit in-app menus everywhere (one road, testable, Linux works); native only if wanted later. |
| **File dialogs** | `prompt_for_paths`, `prompt_for_new_path` (6+7 sites). | Feature `native-file-dialog`: `tx.open_file/save_file/open_folder`; headless `fileDialog` in `--tree`, script `file-dialog`. | Direct map. Tests become scripts. |
| **Drop files in** | `ExternalPaths` / `on_drop` (62 refs: drum pad, sampler, displays, TS cards). | `on_drop_files`, script `drop_files`. | Direct map. |
| **Keys / shortcuts** | gpui actions + `KeyBinding` (77 refs) + key contexts (`ToolKeys`), `Shortcuts` keymap. | `on_key` routing (documented 9 steps), `Chord`/`Keybind` (mod = ⌘/Ctrl), `physical_key`, `key_base`, `on_key_up`, `ShortcutRecorder` (rune-kit). | Keymap (`crates/ui/src/shortcuts.rs`) stays; matching via `Chord`. Tool `onKey` held keys: `on_key` + `on_key_up`. |
| **Accessibility / agent snapshot** | None. | AccessKit tree from role/label/value/states/actions; `--tree` writes it under `"accessibility"`; `Runtime::accessibility_action(key, action)` in tests. | SDK v2 primitives map to `role`, `label`, `checked`, `range_value`, `a11y_value`; that tree *is* the agent snapshot. Act: scripts click/drag/key by key; a11y actions only in Rust tests (open: script event for a11y actions → Rune issue). |
| **Fonts / icons** | Inter Display ×4 embedded; 85 SVG icons via gpui assets. | `AppSpec::fonts/families`, `Family::Named`; `svg` feature (resvg), `Icon`. | Map. Drop `embedded-fonts` feature to save 1.25 MB. |
| **Text input** | Own `text_input.rs` (1,973). | rune-kit `TextInput/TextArea/TextField`, IME, undo, rich. | Delete ours. |
| **Clipboard / open URL / reveal** | gpui platform calls. | `tx.read_clipboard`, rich clipboard, `open_url`; reveal in Finder not seen → `open -R` via `sound_core::process`. | Small. |
| **Window lifetime** | `start_window_process`, quit, "Open project…" = new process. | `App::new(..).on_close`, `Tx::quit`, `AppHandle`, `single_instance` (Windows), `on_reopen`. | Keep "new process per project". |
| **Background work** | `cx.background_spawn` (45 sites) + `this.update`. | No executor; `Waker`, `AppHandle::update`, `rune::Latest` (requests off main thread). | Threads/`smol` + `AppHandle::update`. ENGINEERING's main-thread rule stays, wording changes. |
| **Main thread for plugins** | gpui main thread. | Rune's UI thread is the main thread (winit). | Same. |
| **Offscreen snapshots** | gpui `test-support` headless, macOS only. | Headless on every OS, same renderer, virtual clock. | Gain: Linux CI can snapshot. |

---

## 3. Toolchain and dependency plan

**Answer: both live in one workspace during the transition; no hard switch.**

Verified in `/tmp/coexist` (scratch crate: gpui `c4273271…` + gpui_platform features as ours +
rune `7a92d24` `tooling` + rune-kit + 5 patches):

| Check | Result |
| --- | --- |
| `cargo generate-lockfile` | resolves (fresh; seeding with our `Cargo.lock` fails only because it locks objc2 0.6.4, Rune's fontique needs ≥0.6.5 → `cargo update -p objc2` in the real repo) |
| `cargo +1.99.1 check` / `build` | ok / links, binary runs |
| `cargo +1.98.1 build` (with rune-kit) | links |

Duplicate crates in the lock (both stacks): accesskit 0.24.1 + 0.25.2, accesskit_macos 0.26.3 +
0.27.2 (Rune's patched), taffy 0.13 + 0.15, objc2 0.5.3 + 0.6.5, objc2-app-kit 0.2 + 0.3,
naga/wgpu/wgpu-hal 30.0.0 (git, gpui on Linux/Windows/web) + 30.0.x (crates.io, Rune),
core-foundation/core-graphics two each. Cost: build time and binary size until gpui leaves.

Plan:

1. Root `Cargo.toml`: `rune = { git, rev = "7a92d24…", features = ["tooling", "native-file-dialog"], default-features = false + ["accessibility","metal-backend","svg","dbus"] }`, `rune-kit` same rev, and the 5 patches **at the same rev** (`winit`, `parley_engine`, `wgpu-hal`, `accesskit_windows`, `accesskit_macos`, all `git = rune`). Drop `ogg-audio` and `embedded-fonts` (we ship Inter Display).
2. Stay on **1.98.1** while gpui is in; move to Rune's pin after gpui is deleted. Note new
   clippy lints if the toolchain moves (CI denies warnings).
3. `deny.toml`: allow the Rune git source; run `cargo deny check` — expect new crates (rfd,
   zbus on Linux, ICU4X, swash/skrifa, fontique) and maybe new licence ids (check OFL for any
   bundled font assets). Remove the gpui advisories at the switch.
4. CI Linux: add `libxkbcommon-dev` (has `-x11-dev`; check), `libwayland-dev` (present).
   Headless snapshots on Linux need `mesa-vulkan-drivers`.
5. `[profile.dev.package]`: add `rune = { opt-level = 3 }`, `rune-kit`, `parley`, `swash`,
   `skrifa`, `rune-taffy` (debug Rune was 5.7 s edit-to-PNG vs 4.4 s gpui; measure).
6. `cargo verify` will rebuild/test widely on the commit that adds the deps (root Cargo.toml
   changes). Expect one long run.
7. Linking both is fine (one event loop per process; Rune and gpui register different ObjC
   classes). Not verified: Linux/Windows link of both; run the CI matrix on the first PR.

---

## 4. First vertical slice (runnable, eval-able)

**Goal.** `sound-tools-next <project-folder>` opens a real project (from `templates/daw`) in a
Rune window, runs its tools of the project through Bun with SDK v2 views, plays, and an agent
can check it headlessly. No agent sidebar, no settings, no history, no plugin windows.

**Decided constraints.**
- New crate `crates/window` (binary `sound-tools-next` during the transition), depending on
  `runtime` (lib), `sound-core`, `sound-typescript`. The gpui app is untouched and stays the
  release. gpui is linked into the new binary through the extensions; accepted.
- SDK v2 is additive in the same Bun host: a tool may export `view` (v2) next to its old
  `card`/`page`. The old window ignores `view`; the new window ignores old views and falls back
  to a **generic card** (one control per record field from its schema: knob for number,
  toggle for boolean, segments for choice) for anything without `view`, including built-in
  devices and plugins. So any project opens.
- What moves every frame never crosses to Bun: playhead, meters, time, scroll, zoom, and drags
  are window-side (`Live`, behaviours); code hears the end.

**What it needs, per crate:**

| Crate | Needs |
| --- | --- |
| root | `rune`, `rune-kit` deps, 5 patches, `deny.toml` source, member `crates/window`. |
| `sound-core` | Nothing. (Uses `EngineControl` offline stepping as `screenshot.rs` does.) |
| `runtime` (lib) | Reuse `open_with_extensions`, `registry`, `playing_plugins(true)`, `OFFLINE`, `tick`, `PROJECT_FILE`, `app::check_project_folder`. Make `screenshot.rs`'s offline setup a reusable `pub fn` (or copy it). No window code changes. |
| `sound-ui` | Nothing from the gpui side. Optionally move `shortcuts.rs` keymap later. |
| `sound-typescript` | (a) `host.ts` + new `sdk/view.ts`: `view(state, ctx)`, primitives `row/col/box/text/shapes`, `memo`, `change`, behaviours `press`, `drag` (axis/min/max), `valueDrag` on a record field, `live` bindings (`playhead`, `time`, `meter:<instance>`), a11y props (`label`, `role`, `value`, `checked`); test helpers `render/find/press/dragEnd/apply` (spike `sdk.ts`). (b) `bun.rs`: requests `View{instance, size}`, `Press{key}`, `DragEnd{key, moved}`; events `Tree{instance, root, memos, drop}`, `Change{instance, ops}` (ops → `RecordEdit::Fields`, one commit/undo step). (c) a UI-free `ToolsHost` (new module, ~1,000 lines, built from `window.rs`'s `Live`): start/restart Bun, watch `extensions/`, tell records/transport/song, apply changes, control-loop frames at 33 ms, actions, keys. Not in the slice: takes, MIDI taps, menus from code, file questions, renders, offers. |
| `crates/window` (new) | `main.rs`: parse `<folder>` + our flags (`--play <s>`), hand the rest to Rune (`App::args`). App state: `Session` (plain: project, engine status, notice, gesture), `ToolsHost`, trees by instance, `Lives` (playhead, time, meters per instance from `Peaks`), theme. View: title row (project name, play/stop, position), the main instance's v2 tree (or generic card), cards of children the tree names, a notice line. Renderer tree → `El` with `theme.json` tokens (seed: spike `ts/window/src/main.rs`, 725 lines). Poll: engine status → `Live`s; Bun messages → trees/changes; project events → tell Bun. Headless (`rune::tooling::headless_run()`): offline engine stepped by virtual seconds, Bun settle before each frame, `--play <s>` presses play at 0 and sets `--at`. Keys: space = play/stop, cmd-Z/⇧cmd-Z undo/redo. |
| `templates/daw` | v2 `view` for `song` only (tracks with name + M/S/R + fader, clip lanes as `shapes` in tick units, ruler, playhead `live`, transport), `ui/` (button, toggle, fader, meter, knob) and `theme.json`. Other tools fall back to the generic card. Written into new projects; existing projects get them via `templates::update` only where unchanged. |
| `tooling/agent-eval` | A `ui-next` mode pointing the builder/QA at `sound-tools-next` with the new flags and `bun test`. |
| Not touched | `crates/agent`, `crates/gallery`, extensions' views, plugin-host windows, settings, history. |

**Agent checks in the slice.**
- `sound-tools-next <folder> --snap out.png [--at 2] [--size 1400x900]`
- `--tree out.json` (positions, text, colours, and `"accessibility"`: role, key, name, value,
  states, actions per control = the agent snapshot)
- `--script steps.json` (click/drag/key by element key; `menu-pick`, `file-dialog` later)
- `--play 3` (plays from the start for 3 virtual seconds before the snap/tree)
- `bun test` in the project (`render`, `find`, `press`, `apply`), `bun x tsc -p .`

**How to verify.**
- Open a new DAW project: window shows the song; space plays; playhead/meters/time move with
  the view not re-run (count view runs in a test).
- Press M on track 2 (`--script` by key): `state/song/…` record changes on disk; cmd-Z undoes.
- Edit a tool file while open: Bun restarts, view updates, no window restart.
- `--snap` of the same project twice gives identical PNGs (determinism with Bun settle).
- `--play 2 --tree` shows the playhead moved and meters non-zero.
- `cargo test -p runtime --test projects` still green (sound unchanged).
- Agent eval `ui-next`: run the 5 baseline tasks; compare criteria pass/unverifiable and cost to
  the Opus 5.5 baseline in `spikes/tool-sdk/REPORT.md`.

**Size.** ~3,000–4,000 new lines (Rust ~2,500: window crate + ToolsHost + protocol; TS ~800:
SDK v2 + song view + ui/). One strong agent ~3–5 days, or 2 agents (SDK/Bun side, window side)
on a shared protocol type defined first.

---

## 5. Workstreams after the slice

Each owns separate directories, so worktrees rarely conflict. Shared hot spots: root
`Cargo.toml`, `crates/window/src/main.rs` (module list, app state). The slice should leave a
registration point per area (e.g. `shell::`, `cards::`, `plugin_windows::` modules with one
`fn` each called from the app), so streams add files, not edit the same lines.

Sizes: S < 1k lines, M 1–3k, L 3–8k, XL > 8k (written lines, new or ported).

| # | Workstream | Owns | Depends on | Size | Done when | Verify |
| --- | --- | --- | --- | --- | --- | --- |
| A | **UI-free app core**: `Session` as a plain struct; live input (MIDI, computer keys), audio input, recorders/takes, other apps, update, support-folder prefs; `ToolsHost` grows takes, MIDI taps, file questions, menus from code, renders, offers; executor → threads + waker. | new `crates/app` (or `crates/host`), moves from `runtime/src/window/{audio_input,live_input,recorders,other_apps}.rs`, `runtime/src/update.rs`, `ui/src/{session,metering,recording,lanes,render,waveforms,import}.rs` logic, `typescript/src/window.rs` logic | slice | L | The new window records MIDI and audio takes, hears other apps, updates itself, with no gpui in `crates/app`. | `cargo tree -p app -i gpui` empty; port `tests/window/tools/{recorder,takes_and_notes}.rs` rules as plain tests on the core with the simulated device. |
| B | **SDK v2 complete**: behaviours (value drag with fine/reset/typed value, selection rectangle, move/resize on a grid with snap, menus and context menus, typeahead lists, text input, keys/actions), a11y props on every component, `bun test` helpers for each, `theme.json` schema typed as literal unions, agent docs rewrite (`extensions*.md`), delete v1 view modules at the switch. | `crates/typescript/src/{host.ts,sdk/**,*.md}`, protocol in `bun.rs`, window-side behaviour code in `crates/window/src/behaviours/` | slice | L | Every behaviour has a TS test helper and a Rune runtime test; agent docs ≤ ~1,000 words. | `bun test` in `templates/daw`; Rust tests drive each behaviour via `Runtime` by key; a fresh agent adds a feature using only the docs (spike method). |
| C | **DAW template on v2**: song page (tracks, clips move/resize/split, ruler, sections, loop), automation lanes, clip editor/piano roll, mixer, panel with device cards, tempo, keys, time. ~3,400 lines of v1 TSX today; spike's song was 155. | `templates/daw/extensions/**`, `templates/daw/ui/**`, `theme.json` | B (behaviours as they land) | L–XL | Every scenario of `tests/window/tools/{song_view,gestures,piano_roll,mixer,timeline,rack}.rs` has an equivalent `bun test` or `--script` check. | Port those test rules; `tooling/perf` real-window measure on the template (spike `measure.py`): play/edit/scroll ≤ GPUI CPU; zoom ≥ 55 fps at template size. |
| D | **Device cards on Rune**: card frame, knob, meter, display/curves, waveform, pad, segmented, select on Rune + one `theme.json`; generic card from schema; then each of 17 extension views. Recommend: generic card + optional `display` hook per extension, replacing most hand-written `view.rs`. | `crates/ui` (becomes Rune), `extensions/*/src/view*.rs` | slice | L (parallel per extension, S each) | Every built-in device shows and edits on the new window; a drag is one undo step; lanes show the playing value. | Port `tests/window/{filter,eq,compressor,…}.rs` rules to Rune `Runtime` tests; `--snap` goldens per device. |
| E | **Shell**: project menu, problems, notices, start window, new project, save template, export settings + export flow, file dialogs, app keys + Shortcuts settings, Audio settings, window placement, "Open project…" new process. | `crates/window/src/shell/**`; ports `runtime/src/window/*.rs` views | slice, A for audio devices | M–L | Parity checklist of `runtime/src/window/*.rs` doc comments met. | Port `tests/window/{shell,history}.rs` rules as scripts; manual run on macOS + Windows. |
| F | **Plugin windows**: spike first (child NSView in a Rune window: draw order, focus, keys back), then port `plugin-host/src/window.rs` (open per plugin with `rune::open_window`, placement via `placement()`, resize to plugin size, float above main window), replace gpui `WindowHandle`/`Keystroke` in `host.rs`/`backend.rs`/`vst3/view.rs` with own types; plugin card via D. | `extensions/plugin-host/src/{window,host,backend,view}.rs`, `host/windows.rs`, `vst3/view.rs` | slice | M (+ S spike) | A CLAP and a VST 3 test plugin open, resize, keep placement, take keys, on macOS and Windows. | Port `plugin-host/tests/plugin_host/{window,remembered,resizing_and_keys}.rs`; manual with a real plugin. |
| G | **Agent sidebar + Agents settings** on rune-kit (`Markdown`, `TextArea` with pills/attachments, `Select`, `shell::Sidebar`, onboarding). | `crates/agent/src/view/**`, gpui bits in `agent/src/{lib,settings,environment,install,conversation}.rs` | slice, E (settings page frame) | L | Sidebar parity: threads, streaming turns, approvals, attachments, naming, went-back note, onboarding, model picker. | Port `tests/window/agent.rs` (34 tests) + `snapshots/agent.rs` as Rune runtime tests / scripts. |
| H | **Tests, perf and eval tooling**: harness for Rune window tests (open project, offline engine, Bun, `Runtime` + script by key), `--snap` golden runner replacing `tests/snapshots.rs`, render-budget rule replaced by "view runs per scenario" counts, `tooling/perf/ui.py` on Rune (`--native --trace` / `frame-profile`), agent-eval `ui-next` default. | `crates/window/tests/**`, `tooling/perf`, `tooling/agent-eval` | slice; feeds C–G | M (harness) + ongoing | Every gpui test file has a mapped replacement or a reason it is gone. | A table file→replacement in the switch PR; CI green on all three OSes. |
| I | **Switch and delete**: `sound-tools-next` becomes `sound-tools`; delete gpui deps, `crates/gallery`, `runtime/src/{screenshot,window*}.rs`, `typescript/src/{card,tree,region_card,tokens,window}.rs`, v1 SDK modules, gpui tests, gpui skills, `clippy.toml` timer rule, `deny.toml` gpui advisories, `[profile.*]` gpui entries; rewrite ARCHITECTURE "The window" and ENGINEERING GPUI/test sections (shorter); bundle scripts/CI packages; toolchain to Rune's pin. | everywhere | A–H at parity | M | `cargo tree -i gpui` empty; docs under word limits. | `cargo verify`, full CI, release dry run, agent-eval on the release build. |

**Order.**
1. Slice.
2. In parallel: A, B, D, F-spike, H-harness.
3. Then C (as B's behaviours land), E, G, F-port.
4. I when the parity table is complete.

---

## 6. Risks and open questions

**Risks.**
- **Pre-release dependency.** Rune takes no pull requests (issues only), and every bump means
  re-pinning 5 patches at the same rev. API churn on bump. Pin one rev per quarter, and file
  issues early for gaps (window moved callback, a11y actions in scripts).
- **Plugin editors.** A child NSView/HWND inside a Rune (Metal/D3D12) window is unproven: z-order,
  first responder, key passthrough, HiDPI on Windows (physical px), live resize. Spike F before
  committing.
- **Headless determinism with Bun.** Rune's virtual clock does not wait for a second process.
  Without a settle step, `--snap` and scripts are flaky. Must be designed in the slice.
- **Per-frame view rebuilds on big songs** (Rune zoom 47 fps at 512 clips). Keep zoom and scroll
  window-side (`units` scaling, `Live`), never in a tool view.
- **Memory.** Rune +20 MB; Bun holds the state twice (+30 MB template, +100 MB big song).
- **Transition cost.** Both stacks compile in every build (slower `cargo verify`, bigger
  binaries, two wgpu copies on Linux/Windows). Linux/Windows link of both not yet verified.
- **Test coverage gap.** 21.6k lines of gpui window tests hold behaviour rules (undo steps,
  gestures, lanes, render budgets). Porting the rules, not the code, is easy to skip; H tracks it.
- **Old projects.** Project code is a copy of the template. Projects whose tool code the
  composer or agent changed keep v1 views; `templates::update` cannot merge them. After the
  switch they show generic cards until re-written.
- **Linux.** Native popup menus are unsupported there (use rune-kit menus); plugin windows stay
  unsupported; Rune needs `libxkbcommon` at build time.
- **Licences/advisories** of new crates under `cargo deny`.
- **Doc churn.** ARCHITECTURE "The window" and several ENGINEERING rules are GPUI facts; they
  must shrink, not grow, per `tooling/workspace-rules`.

**Open questions (owner: Casper).**
1. **Built-in device cards: Rust or TS?** Decided: TS, layer 4 of SPEC's layers. Rust
   extensions export record + parameters (+ an optional display); the project draws the card.
2. **Keep v1 views working in the new window?** Recommendation: no; generic card fallback.
3. **Binary during the transition:** separate `sound-tools-next` (recommended, no conflicts
   with `runtime`) or `sound-tools --next`.
4. **Toolchain:** stay on 1.98.1 (verified) until gpui is gone, then Rune's pin?
5. **Native vs in-app menus:** recommendation in-app (rune-kit) everywhere for one road.
6. **Agent act beyond scripts:** is `--script` by key enough, or do we want an `act` command
   (a11y actions by key: increment, set value, expand) in our binary for agents?
7. **Sidebar before switch?** The switch needs G; or ship the new window first without the
   sidebar behind a setting?
8. **`crates/gallery`:** delete in favour of Rune's playground and `--snap --scene`?
