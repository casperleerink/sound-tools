# Spike: a small core and a TypeScript kit that builds any tool

## Goal

Sound Tools becomes a small core plus a TypeScript SDK with few concepts and many Rust building blocks used as children. They compose into any kind of tool. The DAW is not built in: it is a project written with the SDK, like any other.

## Decided constraints

- **Few concepts.** Anything that can be a child or a helper is one. Change the cut when the work shows a better one; every new concept needs a reason.
- **Rust builds blocks, TS owns structure.** Rust is welcome for blocks the SDK uses as children: a reverb, a synth, a sampler, the plugin host, timeline, recorder, audio and MIDI in and out, meters. A block composes through ports and knows no DAW idea. Rust never owns how a tool or project is organised: no tracks, clips, mixer layout, transport bar or views that decide a workflow.
- **The core knows no music beyond the clock:** position, tempo map, start, stop, jump, a repeat range. What play or record means, the transport bar and every musical key are SDK or project code. The app keeps only app and file shortcuts and machine settings.
- **Motion never crosses to Bun; structure updates are fine-grained.** The mechanism is open: change it when a measurement says so.
- **TS tools run unchanged on any OS, with no build.**
- **Typed end to end.** Misuse is a type error, or an error that says the fix.
- **Possible, not easy.** The SDK must make a full arrangement possible and leave room to improve it. The arrangement project is a work of its own.
- Branches: `spike/native-ui-solid` (current), `spike/native-ui` (React-like, round 3a, reference only). Never main.

## How to verify

- A project of TS tools works almost like main's Rust arrangement: tracks with instruments and effects, note and audio clips, a note editor, automation, a mixer with sends and meters, the master, transport and loop, MIDI and audio recording, file drop, device cards. Fit tempo is out of scope. **Met:** `templates/daw`, driven by `crates/runtime/tests/window/tools/{song_view,template}.rs`.
- Nothing in the runtime knows the arrangement. **Met:** the Rust arrangement is deleted.
- It feels as smooth as main's arrangement: numbers below.
- Agents still write new tools from the docs: eval below.
- Every new concept or block has window or render tests.

## Decisions

Settled during rounds 1–14; the round-by-round log is in git history. The lasting ones are in [ARCHITECTURE.md](../../ARCHITECTURE.md).

**The UI model**
- **GPUI draws, TypeScript decides, as in React Native.** Bun runs a tool's code; the window keeps a node table and draws it.
- **Fine-grained, as in Solid, not a tree per change.** Round 3a (`spike/native-ui`) redrew the whole tree per change: a click at 20,000 notes sent 1 MB; with ops it sends 0.7 KB. A component runs once; a function given as a prop is a binding that runs again when what it read changes.
- **Our own reactive runtime in `sdk.ts`.** Rejected: Solid with its Babel transform (5–8 MB in every project, about 100 ms cold start, a compile step agents cannot see); Solid without the compiler (DOM-only). Owning it lets misuse throw with the fix ("x={() => note.start}, not x={note.start}").
- **Bun hears only what its bindings read** (read tracking), so a knob turned natively costs Bun nothing.
- **Motion is native:** live values (`now`), `Space` units, scroll, zoom and culling, `Group` `moves`/`grows`, clipping, declarative drags with snap and grid. Code hears a drag once, at its end. `onMove` was removed for crossing on every pointer move.

**The shape of a project**
- **Realistic means many small records:** a song of tracks of clips, not one 20,000-note field. Children nest at any depth; a pattern name (`"ret-*"`, `"*"`) stands for many children of one tool.
- **The concept cut:** tool (record, memory, tick), children, typed ports and connections (signal, number, bang, note), signal, view, update, clock. What became a child or a helper: the sequencer (`timeline` with `loop`), meters (`meter`), mixing (`fader`), automation (`lane`), audio clips (`region`), recording (`recorder`), the transport bar (`<Transport/>`), Knob, Steps, Meter, Pad (TS components), the canvas (`Space`).
- **Two hooks with reasons:** `onTake`, because what a take becomes is structure, which TS owns; `takes(port)` slots, because a list of tools would leave out the project's own instruments and plugins.
- **Children reach all code filled with defaults**, one shape, no casts.
- **One road to a built-in effect: a child.** `devices.reverb(...)` inside a `sound` was removed: a second road with its own types, checks and engine ports.
- **Clock in the core, everything musical outside it.** `clock.play/pause/stop/jump(to, { at })/setRepeat/setClick/update`; the transport bar is ~50 lines of SDK; space plays only where a tool's `globalKeys` takes it.
- **Device in and out stay `project.json` connection ends.** An `audio-out` child would need the parent's own sound as a source, which ARCHITECTURE rejects (`ownSound`).
- **`--only <path>`** plays one instance's sound by rerouting `project.json` in memory; `--solo` named tracks.
- **Solo is a cable:** a `lane` holding -1000 dB into every other track's fader gain. Pre-fader sends are the return's choice.
- **No compiled extensions.** Rejected: rebuilding the app, wasm or dylibs from a project: Xcode tools or MSVC and a build when a project opens.
- **Templates:** a folder of `templates/` is a template; Bare and DAW. The Rust arrangement, `arrangement_wiring` and fit-tempo are deleted.

## Numbers

M1 Max, release-fast. Repeat with `projects/render_cost.rs` and the ignored tests in `window/tools/performance.rs`. The Rust arrangement column is from round 12, before it was deleted.

| | TS song | Rust arrangement |
| --- | --- | --- |
| Render, 12 tracks / 150 clips, same content | 26x real time | 28x |
| Scale project 100 × 100: open | 3.3 s, until drawn | 1.8 s |
| Scale project: frame playing | 2.1 ms | 1.1 ms |
| Scale project: frame scrolling / far zoomed out | 2.0 / 1.9 ms | 3.4 / 3.5 ms |
| Clip drag, per move | 2.5 ms (12 / 150), 3.5 ms (24 / 600), 0 Bun requests | 3.9 ms |
| Select a clip / release a move, until drawn (12 / 150) | 12.4 / 14.0 ms, Bun 1.0 / 1.5 ms | |
| Space until the clock plays (12 / 150) | 17 ms | |

"Until drawn" includes about 10 ms of the test's polling. `tooling/perf` and the agent evals now build DAW-template projects, so their numbers start a new baseline.

**Agent eval** (round 12): `daw-arp-builder` (an arpeggiator before a track's synth, $1.90), `daw-return` (a return with a long reverb, $0.18), `ui-chords` (a chord player card, $0.75): all passed. Round 14c's run on the trimmed docs is under "Round 14c".

## Open gaps

- Opening grows with the song: about 0.8 µs per binding in Bun, 3.3 s at 10,000 clips. Needs a leaner reactive core or rows made only when they show.
- A playing frame costs about twice the arrangement's on the scale project.
- Template features: dragging a ruler marker, a double-click that clears a lane, a foldable returns section, the layer rule of regions (overlapping regions sum; `player.rs` still has the rule), fades as dials rather than handles, a clip moved to another track leaves its automation behind.
- A clip moved to another track is a delete and a create: a record's id is its path.
- Copied notes and copied clips are two clipboards.
- `"*"` and named children cannot be optional.
- Notes are the only event kind. A second waits for a tool that needs one.
- Inline TS tools in Rust tests are not type-checked: some are wrong on purpose and need a mark first.
- Headless snapshots show no meter motion.
- Recorder fixes with no test that fails without them: expression lanes kept, a take at a loop waiting, the old clock closing a take, missed events reported.
- `now` is exported untyped and on the context; they are one object.
- Two new-project flows (restart on a folder, open from the start window) share one write.
- A recorder test failed once in a parallel run and passed in reruns.
