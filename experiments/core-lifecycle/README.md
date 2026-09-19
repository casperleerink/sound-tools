# Core lifecycle prototype

A small working core and two extensions, built to test the SDK against real use. This is an isolated experiment, not the Sound Tools application. There is no built-in agent.

- `core/` owns typed instances, edits, persistence, undo/redo, record polling, saved connections and offline mono processor execution. It has no GPUI dependency.
- `ui/` exposes the project as an observable GPUI Session and registers optional views.
- `tone/` is the reference extension. Its two views share one record; a second instance is independent.
- `tremolo/` was authored by a Codex subagent from the SDK notes and Tone. Its custom editor controls frequency, gain, tremolo rate and depth.
- `runtime/` registers both extensions and supplies a small window plus headless inspection and WAV rendering.

Read [SDK.md](SDK.md) for the implemented APIs and [the authoring report](tremolo/AUTHORING.md) for the subagent's results. The earlier root-level SDK sketch remains a design hypothesis; this code implements only the small lifecycle needed by these examples.

## Run

From this directory:

```sh
export CARGO_TARGET_DIR=/private/tmp/sound-tools-timing/target
cargo run --offline --locked -p lifecycle-runtime -- /private/tmp/my-sound-project
```

The target directory reuses the previous experiment's cached dependencies on the original development machine. On another machine, omit it and run `cargo fetch --locked` once before the offline commands. Rust/GPUI native platform dependencies are required. Only macOS has been tested.

A new project starts with two Tone instances and two Tremolo instances. Only Tone A connects to the output. The initial workspace opens Tone A twice, Tone B once, and both Tremolo editors. Scroll to see all editors. Closing a view changes `workspace.json` and leaves its instance intact. Use the Open buttons to reopen it. The default folder, when no argument is supplied, is `sound-core-lifecycle-demo` inside the OS temporary directory.

Run renderer processes small sample blocks and displays their RMS. It does not send sound to an audio device or advance a real-time transport. Stop rendering returns the meter to zero. Parameter buttons make named edits and write the record immediately. Undo and redo restore state through the same path. Replace a known `state/<id>.json` file to apply an outside edit within the 50 ms polling interval. Last write wins throughout.

The project consists of `project.json`, independent instance records in `state/`, and `workspace.json`. The extension source currently lives in this experiment workspace; project-local source copies are not implemented.

Headless operations need no window:

```sh
cargo run --offline --locked -p lifecycle-runtime -- /private/tmp/my-sound-project --inspect
cargo run --offline --locked -p lifecycle-runtime -- /private/tmp/my-sound-project --render /private/tmp/tone.wav
```

Rendering exports one second of 48 kHz mono float WAV using the same core and registered processors. To hear Tremolo in that export, edit `project.json` while the runtime is closed and route its `audio` port, then reopen. Changing topology live is outside this prototype.

## Verification

```sh
cargo test --offline --locked --workspace
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
python3 verify_reload.py
```

The reload runner requires desktop access. It creates a disposable project, builds an intentional failure while the current runtime stays open, then edits Tone DSP successfully and restarts. It restores source and rebuilds the original executable on exit. Do not edit Tone concurrently. It does not watch source automatically or integrate an agent.

Five integration tests and strict Clippy pass. See [test output](results/tests.txt), [Clippy output](results/clippy.txt) and [native verification notes](results/verification.json). Core tests cover typed edits reaching an existing processor without resetting its position, independent instances, file replacement, undo/redo, persistence, restored connections, deletion removing output, and last-write-wins during gestures and cancellation. Invalid JSON leaves live state intact. Tremolo tests also check both oscillator phases survive parameter replacement.

The subagent compiled Tremolo on its first attempt and passed both tests on its first run. It made no core or UI changes. It needed to inspect a few public Project signatures omitted from the initial SDK notes; those signatures are now documented. This is one successful authoring example, not evidence that the SDK is complete.

[Reload evidence](results/reload.json) records a failed build keeping the old process and executable, followed by a successful optimized Tone edit. Only Tone and the runtime rebuilt. The edit reached a replacement GPUI frame in 2.136 seconds, including 1.385 seconds for the build. All four records and routing were restored, with rendering stopped and undo history empty. This is one measured reload, not a latency distribution.

## Current limits

Audio rendering is offline and single-threaded. There is no audio device, callback handoff, scheduler, general graph, modulation contract or complete transport. The UI uses buttons; the core gesture API is covered by tests but no slider is implemented. Parameters are fields in typed state; declarative parameter metadata is still a proposal.

Record polling covers existing records. External additions, deletion, index changes and changing a record's tool require reopening. Missing tool registration fails loading without changing files. Creation, deletion and routing are persisted but are not undoable yet. Owned children, assets, project-local extension copies and a general workspace editor remain outside this slice.

Native automated checks confirmed edits reach disk and both views read the current record when rendered. The [shared-view screenshot](results/native-shared-views.jpg) shows Tone A changed independently of Tone B. The [authored-editor screenshot](results/native-authored-editor.jpg) shows Tremolo A at 4.5 Hz after its native button edit; Tremolo B remained at 4 Hz. Closing one Tone A view preserved the record. Screenshots after clicks and file edits remained stale until resizing the window. Temporary logs confirmed Session and view notifications fired. This native repaint issue is unresolved; do not count automatic visible refresh as verified. The custom controls also do not appear in the accessibility tree. These are GUI validation limits, separate from the passing core and extension tests.
