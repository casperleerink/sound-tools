# Spike: a small core and a TypeScript kit that builds any tool

## Goal

Sound Tools becomes a small core plus a TypeScript SDK with few concepts and many Rust building blocks used as children. They compose into any kind of tool. The DAW is not built in: it is a project written with the SDK, like any other.

## Decided constraints

- **Few concepts.** The current cut: tool (record, memory, tick), children, typed ports and connections (signal, number, bang, note), signal, view (elements, live values, gestures), update, clock. Anything that can be a child or a helper is one. Change the cut when the work shows a better one; every new concept needs a reason.
- **Rust builds blocks, TS owns structure.** Rust is welcome for building blocks the SDK uses as children: a good reverb, a synth, a sampler, the plugin host, timeline, recorder, audio and MIDI in and out, meters. A block composes through ports and knows no DAW idea. Rust never owns how a tool or project is organised: no tracks, clips, mixer layout, transport bar or views that decide a workflow, as the arrangement does today.
- **The core knows no music beyond the clock:** position, tempo map, start, stop, jump, a repeat range. What play or record means, the transport bar and every musical key are SDK or project code. The app keeps only app and file shortcuts and machine settings.
- **Motion never crosses to Bun; structure updates are fine-grained.** The mechanism is open: change it when a measurement says so.
- **TS tools run unchanged on any OS, with no build.** Compiled Rust extensions were rejected: they need the Xcode tools or MSVC, and a build when a project opens.
- **Typed end to end.** Misuse is a type error, or an error that says the fix.
- **Possible, not easy.** The SDK must make a full arrangement possible and leave room to improve it. The arrangement project is a work of its own.
- Branches: `spike/native-ui-solid` (current), `spike/native-ui` (React-like, round 3a). Never main.

## How to verify

- **The final proof:** a project composed of TS tools that works almost like main's Rust arrangement: tracks with instruments and effects, note and audio clips, a note editor, automation, a mixer with sends and meters, the master, transport and loop, MIDI and audio recording, file drop, device cards. Fit tempo is out of scope.
- The app shows it without naming it: nothing in the runtime knows the arrangement.
- It feels as smooth as main's arrangement on the same machine. Numbers come from a repeatable harness, in Findings.
- Agents still write new tools from the docs reliably (agent eval).
- Every new concept or building block has window or render tests.

## Findings

### Phase 1

**Works.** `Knob`, `Steps`, `Meter`, `Pad` and `Canvas` are now TS components in `sdk.ts` made of `Space` + shapes; their native code in `card.rs` / `tree.rs` is gone. The props agents use stayed the same, so the agent-eval examples type-check unchanged. A knob turns its field natively (one undo step, zero Bun requests). A drag moves everything bound to `now.drag(id)` and Bun hears it once. The window tests in `crates/runtime/tests/window/tools/gestures.rs` check both.

**What it took beyond the plan:**
- *Read tracking.* Without it, a native field drag still asked Bun for a tree on every record change, which undid the whole point. `host.ts` now records which fields, children, watches and transport the card's code read, and the window asks for a new tree only when one of those changed. Same idea as MobX or signals. It belongs in the model.
- *A drag follows the newest tree.* A long `onMove` drag outlived the 8 trees Bun keeps handlers for, and went silent. It now finds its handler again by key in each new tree.

**Awkward:**
- JSX can't say "shapes only inside a Space": a `Rect` is a `Node` type-wise, and the error only comes at draw time.
- `drag.x.field` names are plain strings. A typed `now.field` exists through the context, but the components use untyped names.
- Live-value JSON is verbose (`{"op":"plus","a":{"op":"drag",...}}` per shape). A Space tree is about 4x the size of the old canvas tree for the same notes.
- Only snap-by-delta. Snapping to absolute grid lines needs a start-aware op.
- Escape doesn't cancel a drag yet.

**What I'd change:** share live subexpressions across shapes, either by a reference table or by binding a group transform once ("these keys move by `drag.dx`"), instead of repeating the expression per shape. That cuts the tree size and makes multi-select cheaper.

**Numbers.** Apple M1 Max, `--cargo-profile release-fast`. Window frames are GPUI's test platform: event + work + layout + paint into the scene, not the GPU, which is the same thing `snapshots.rs` measures. Repeat with the ignored tests in `crates/typescript/src/performance.rs` and `crates/runtime/tests/window/tools/performance.rs`.

| Shapes | Frame, every shape moving with a drag | Frame, no drag | Tree bytes, space / old canvas | Bun render round trip + Rust parse, space | Old canvas drag: move to drawn tree |
| --- | --- | --- | --- | --- | --- |
| 100 | 0.08 ms | 0.08 ms | 28 KB / 7 KB | 0.8 + 0.5 ms | 0.3 ms |
| 1,000 | 0.46 ms | 0.45 ms | 279 KB / 72 KB | 6.6 + 4.4 ms | 2.1 ms |
| 2,000 | 0.95 ms | 0.97 ms | 560 KB / 145 KB | 13 + 9 ms | 4.2 ms |

- Live values: 4,000 evaluated per frame in 0.06 ms.
- Requests per 60 drag moves: new path 0 renders and 0 events; old canvas path 60 events, each returning a full tree.
- For reference, the Rust arrangement in `snapshots.rs`, same profile: note drag 4.1 ms per move, timeline scroll 3.6 ms per frame (100 tracks x 100 clips, which includes a project publish per move).
- What this means: a frame stays far under 16 ms even at 2,000 moving shapes, without Bun. The old path is not slow on this machine in release, but every move costs Bun work and a full JSON tree both ways. In debug builds that is 50+ ms at 2,000 shapes. The Bun round trip now happens only at release and on record changes. At 2,000 shapes that is about 22 ms, which is the next thing to cut (see What I'd change).

### Phase 2

**Works.** `tooling/agent-eval/examples/piano-roll.tsx` is a whole piano roll in TypeScript with no element of its own: zoom and scroll, keys and a ruler that follow the roll, double-click to add or remove, drag many selected notes on the grid (alt copies), resize from the end, marquee and shift-click, a velocity lane, the playhead, delete, cmd-A/C/X/V/D and the arrows. Selection and clipboard live in `memory`. `crates/runtime/tests/window/tools/piano_roll.rs` drives each with the mouse and the keys and checks the record and the undo step; `typescript-piano-roll.png` shows bar 2 selected and held two beats on, before its code hears the drag.

**Phase 1 items, fixed:**
- *Tree size.* `Group` moves its shapes by one live value, a drag shared by many shapes crosses once per space, a handler used twice is kept once. Plus three costs that were not the model's: serde read every tree twice (tagged and untagged enums buffer), on the main thread; Bun wrote a tree's JSON twice. Now one pass, on the thread that hears Bun. 2,000 dragged notes: 560 KB and 21 ms from render request to nodes, now 226 KB and 2.7 ms. Keyed patches were not needed at this size, so not built.
- *Grid snap.* `snap.grid` puts the pressed shape's `anchor` (its x and y, or e.g. the end of a note for its resize handle) on the grid.
- *Escape* cancels a drag: the preview drops, a field turned goes back, no `onEnd`.
- *Typed drag field names:* not done. It needs `Shape` generic over the record, through every component. Not cheap.

**New blocks the piano roll needed,** both general: `follow` (a space shows another's scroll and zoom on an axis and scrolls it: ruler, keys, lanes) and `scroll` (where a space starts).

**Awkward:**
- *A press on an unselected note crosses to Bun* to select it before it joins the moving group, so its first moves wait for one tree (about 10 ms at 2,000 notes). A pressed shape that moves with its own drag natively would close that gap.
- *Every click redraws the whole tree*: selection is memory, so each click is about 5 ms in Bun plus 5 ms reading 1 MB, at 2,000 notes. Keyed patches would cut the second half.
- *No size in points inside a unit space* without a live value per shape: velocity bars are a fixed number of ticks wide, so they get thin when zoomed out.
- *Invisible shapes cost as much as visible ones in GPUI*, which orders a quad by what it overlaps: 900 transparent resize handles cost 4 ms a frame. They are no longer painted, only hit.
- *Keys:* a page that hears keys takes all plain keys, so space no longer plays. The tool would need to say which keys it takes.
- Keys of notes are their index, which shifts on delete; real code wants stable ids.

**Numbers.** Same machine and profile as phase 1. The piano roll has 2,000 notes: 6,300 shapes with handles, velocity bars, keys and grid.

| | Frame (CPU) | Bun requests | Tree |
| --- | --- | --- | --- |
| 2,000 grouped shapes, idle / dragged | 0.92 / 0.92 ms | 0 per 60 moves | 226 KB, 2.7 ms render to nodes |
| Piano roll, hover | 3.2 ms | 0 | |
| Piano roll, scroll / zoom | 3.1 / 2.5 ms | 0 | |
| Piano roll, drag of 512 selected notes | 3.8 ms | 0 per 60 moves | |
| Piano roll, release to new tree drawn | 22 ms | 1 event | 1.1 MB |
| Old canvas path, 2,000 shapes, per move | 2.4 ms | 1 event per move | 145 KB |
| Rust note editor, per move (phase 1) | 4.1 ms | | |
| Rust timeline scroll, 10,000 clips (phase 1) | 3.6 ms | | |

The 22 ms release splits into: Bun runs `onEnd` and sends the 2,000-note edit (about 5 ms), Rust applies it (3 ms), Bun draws and writes the tree (5 ms), Rust reads 1.1 MB (5 ms), the frame (4 ms). Run with `cargo nextest run -p runtime --test window --cargo-profile release-fast --run-ignored only performance_of --no-capture`.

**Verdict.** The model holds. At 2,000 notes everything that moves is native and costs 2.5 to 4 ms a frame, the same as the Rust editor, and Bun hears only presses and releases. What still grows with size is every discrete event, which costs about one tree. For a whole arrangement (10,000 clips, lanes, a rack) trees reach several MB, so keyed patches and shared live values change from optional to needed. Nothing seen so far needs a DAW-specific element.

**Round 3 shortlist,** in order of what an arrangement needs most:
1. Keyed tree patches, so a click costs what it changes. Tried as a Solid-style model on `spike/native-ui-solid`, below.
2. Engine meters (peaks) and lists (a waveform, a scope) as live values.
3. Menus and text input (rename, the tempo field).
4. File drop from the OS onto a space, in units.
5. A built-in device's own card inside a page, for the rack.
6. Drag reorder of boxes, for effect chains and tracks.
7. A pressed shape moving with its own drag before Bun answers, and sizes in points inside unit spaces.

### Solid-style branch

Branch `spike/native-ui-solid`. The question: does a Solid-style model beat redrawing the whole tree? A card, a page and a component run once. A function given as a prop or a child is a binding, which runs again when a field, memory, a watch or the transport it read changes. Only what it changes crosses, as ops on a node table the window keeps by id, as in React Native's Fabric. Live values, spaces, groups and drags stay native, unchanged.

**Design.**
- *Own runtime, about 800 lines in `sdk.ts`*: signals with owners, stores with a reconcile that keeps objects, `For` (rows kept by item, moved with a longest-rising-run diff), `Show`, `memo`, `selector`. Borrowed from Solid's source. Rejected: Solid with its Babel transform in a Bun plugin (5–8 MB to ship into every project, about 100 ms cold start, a compile step agents cannot see), and Solid without the compiler (`@solidjs/h` is DOM-only in 2.0, and reads are wrapped by hand anyway, so it gives our API plus DOM types). With our own runtime, misuse can fail with an error that says how to fix it.
- *Wire*: after each Bun message (and in a microtask after an `await`), one batch per card of `create`, `set` (only the changed props), `insert` (an existing node moves), `remove`, `root` and `drag` (a drag shared by many shapes goes once). Events name a node and a kind. This removes tree versions, the handler cache, the "unchanged" check, render requests and "a drag follows the newest tree".
- *Records* are a store per instance. Our own edits apply at once. Rust's echo then diffs to no ops, and so does undo. Notes have no ids, so a list of the same length is kept by place (a move updates fields in place). A list that grew or shrank keeps the items whose content did not change.
- *Bun says which fields, children, watches and transport its bindings read.* The window tells Bun of a change at once only when something reads it, so a knob turned on a card still tells Bun nothing. Before an event it tells the watches and the transport, so a handler reads them fresh.
- *Rust* keeps nodes in slots made in order. A first version walked ids through a hash map, and its frames cost a third more than the tree's.

**Mistakes made into errors.** Reading a value that changes in a component body throws: "x={() => note.start}, not x={note.start}". The load check runs every card at its defaults, so most of these show as problems before the window opens. These are type errors: `each` must be a function, `Show` children must be a function, a `Set` or `Map` in `memory` is read-only (`.add` caught four in-place mutations in the piano roll port), and shapes have no `key`.

**Numbers.** Same machine and profile as phase 2. Both columns come from the same harness, the phase 2 one run from its own commit. "Until drawn" counts from the pointer event and includes the test's own frame for each mouse event, so compare the columns, not the absolute values. Repeat with `cargo nextest run -p runtime --test window --cargo-profile release-fast --run-ignored only performance_of_the_piano --no-capture`.

| Piano roll | 2,000 notes: tree / ops | 20,000 notes: tree / ops |
| --- | --- | --- |
| Open: until drawn, bytes | 67 ms, 1.0 MB / 97 ms, 1.25 MB | 299 ms, 9.9 MB / 564 ms, 12.3 MB |
| Idle frame | 3.1 / 3.2 ms | 19.8 / 21.1 ms |
| Frame dragging 512 selected | 3.2 / 4.1 ms | 26.5 / 22.8 ms |
| Click that selects a note | 23 ms, 1.0 MB / 16 ms, 0.7 KB | 178 ms, 9.9 MB / 106 ms, 0.7 KB |
| ...of which Bun, Rust apply | / 0.6 ms, < 0.01 ms | / 0.8 ms, < 0.01 ms |
| Marquee of 512, released | 12 ms, 2.1 MB / 23 ms, 457 KB | 98 ms, 19.9 MB / 78 ms, 458 KB |
| Release that moves 512 | 25 ms, 1.07 MB / 23 ms, 314 KB | 204 ms, 10 MB / 152 ms, 314 KB |
| ...of which Bun, Rust apply | / 9.6 ms, 0.4 ms | / 41 ms, 0.6 ms |

- A watch that moves one of 2,000 notes: 0.05 ms from the request to ops read, under 200 bytes, applied in under 1 µs. In phase 1 it was a 560 KB tree at 13 + 9 ms (`crates/typescript/src/performance.rs`).
- At 20,000 notes, 41 ms of a release is the record. `update` copies, compares and writes the whole `notes` field (1 MB), Rust parses and checks it, sends it back, and Bun reconciles 20,000 notes. Edits of list items in records would cut it. This is not the UI model's cost.
- A selection of 512 costs Bun about 9 ms and 450 KB, because each picked shape gets its own live expressions (`move.dx.plus(start)`) where phase 2 moved one `Group`. Interned live leaves, or a compact live format, would cut the bytes by half or more.
- At 20,000 notes every frame is 20 ms: 60,000 shapes are placed before culling, in both models. That needs culling before placing, a native fix.

**Agent authoring.** A tool written from the updated doc alone (a 16-step grid with a count and a playhead) type-checked at once. It found a real bug: a step lit by a click kept its colour, because a list index was tracked as `"3"` but changed as `3`. Fixed, with a Bun-driven test in `crates/typescript/src/bindings.rs`. Notes:
- What changes needs `() =>` all over. It is the biggest change for agents, and the error message is what teaches it. Static cards (knobs, steps, meters) are unchanged.
- Animation driven by memory (a playhead a `tick` writes) works but sends ops 30 times a second. Continuous motion still belongs in live values (`now.watch`, `now.tick`).
- A size-dependent `Canvas` needs a function child that makes it again on resize. It works but is not obvious.
- `tooling/agent-eval`, one run each of `ui-chords` and `ui-paint`: both pass (19 and 17 turns, about $0.70 each), neither hit the error. The chord card put every changing value in a function at once. The paint page wrapped its whole page in a function of `size`, as the doc's canvas example does, and kept the painted cells in a `For` over a `memo`. So the doc's examples are what agents copy, and they should show the best pattern.

**Verdict.** The model works and the protocol is simpler: no versions, no handler cache, no read/unchanged dance, no render requests. A discrete event now costs what it changes on the wire (a click is 0.7 KB, not 1 MB) and under a millisecond on the main thread. At 2,000 notes the end-to-end win is small (click 23 to 16 ms, release 25 to 23 ms), and a big selection change is slower (12 to 23 ms). Phase 2's off-thread tree read was already cheap at that size. At 20,000 notes everything discrete is faster (click 178 to 106 ms). What then limits is not the UI model but the record round trip and native culling. The cost: an 800-line runtime, and agents must wrap what changes in functions, made safe by an error at load. For an arrangement (10,000+ clips) I'd adopt it, together with list-item record edits, culling before placing, a compact live-value format and stable item ids for selection.


### Round 4

Goal: realistic DAW-shaped examples. A song is many small records (tracks of clips), not one 20,000-note field; 20,000 notes in one clip may be slow, it is the exception. Branch `spike/native-ui-solid`. Casper's rule for this round and after: every new block is a general concept (tree ownership, events in time), never a DAW one.

**Decided: no compiled Rust extensions.** We considered letting a project add Rust (rebuilding the app locally, wasm, dylibs) and rejected it: rebuilding needs the Xcode tools or MSVC and a build when a project opens. TS tools run unchanged on any OS with no build, which is a selling point.

**4a: what was built.**
- *Children at any depth, as plain tree ownership.* Bun is told every record inside a tool of the project. A child carries its own `children`, typed by the `children` its tool names, and `update` edits, adds (with their children) and takes out records at any depth; the core deletes what a removed record owns. `"*"` in `children` stands for any other name, so `{ "*": track }` is any number of children of one tool, checked in Rust too. Watches of descendants read as `<path>.<watch>`. The core needed no change. Tests: `crates/runtime/tests/window/tools/nested.rs` (a page reads a grandchild, its edit is one undo step, a removed child takes its subtree, an added one comes with its own).
- *`timeline`: events at positions in the song.* A built-in child like `sequencer`: notes at song positions, played once as the transport passes, out of `notes`. `start` and `length` are optional, so it is a clip, a cue or a whole part. Each is its own record, so moving one changes one file. One `NotePlayer` each, so offs survive moves and deletes. Rendered to the frame in `crates/runtime/tests/projects/song.rs` (a song of tracks of timelines, nested two deep). Out of scope: expression lanes (bend, pedal, wheels: one owner per instrument needs one player per track), audio, other event kinds.
- *`moves` and `grows` on shapes.* A shape names the drags it moves and grows with; native code adds the drag's `dx`/`dy`. A selection then changes one small prop per note instead of a live expression per note: the general form of phase 1's "group transform" idea, without moving nodes between groups.
- *A card reads its record once per change, not once per frame.* This was the real cost of a big page: every frame wrote and parsed the whole record (1 MB at 20,000 notes). Shapes outside the shown part of a space are also skipped before placing; that alone saved little.
- *A store skips a frozen value it was last made from*, so an edit of one clip leaves other records' store objects unwalked.
- *Selection needed no fix:* `selector` already reruns only the notes whose membership changed. The piano roll keeps index keys: notes have no ids in the record format, and every edit that shifts indexes sets the selection.

**Numbers** (M1 Max, release-fast, piano roll harness, "until drawn" includes the test's own frames):

| Piano roll | 2,000 notes: before / after | 20,000 notes: before / after |
| --- | --- | --- |
| Idle frame | 3.1 / 1.9 ms | 21.1 / 5.2 ms |
| Frame dragging 512 selected | 4.1 / 2.8 ms | 22.8 / 7.4 ms |
| Click that selects a note | 16 ms, 0.7 KB / 12 ms, 0.3 KB | 106 / 40 ms |
| Marquee of 512, released | 23 ms, 457 KB / 15 ms, 141 KB | 78 / 36 ms |
| Release that moves 512 | 23 ms, 314 KB / 20 ms, 101 KB | 152 / 129 ms |
| Open | 97 / 104–118 ms | 564 / 517–579 ms |

**Open time, not fixed.** The Bun mount is the cost: 34 ms at 2,000 notes, 260 ms at 20,000, about 0.8 µs per binding across ~40,000 bindings, plus 5 ms to write the ops. Phase 2 called one function per shape. Fixing it means a cheaper reactive core (no computation for a binding that read nothing, lighter dependency sets), not a quick change. At 20,000 notes a release is still 41 ms in Bun, mostly the whole `notes` field crossing both ways: realistic songs keep fields small, so list-item edits stay unbuilt.

**Too specific (to revisit).** SDK concepts tied to one idea, found on the way; not changed now:
- `kind: "effect" | "instrument" | "source"` describes a place in an arrangement track, not what a tool is (ports would say it).
- A card is at most 144 points tall because of the rack under the tracks.
- `transport: true` turns on one fixed transport bar; showing transport controls should be blocks a page places.
- `sequencer` and now `timeline` are special built-in children; both are "events in time", one looping, one at song positions.
- Notes are the only event a timeline, `onNotes`, `play` and `send` know; `notes()` is a field kind of its own.
- Every tool of the project gets a `notes` input in its type (`ProjectEntry`).
- `devices` offers only effects registered as in-code devices.
- Docs place tools by `state/arrangement/<track>/...` paths, and the computer keys play "the selected track".

**4b plan.**
1. Port round 3a's feeds and mixer from `spike/native-ui` by hand (engine peaks, scope/waveform lists, time and smoothing ops, live colours, the mixer example and its tests).
2. A TS mini-arrangement example, built only from general blocks: about 12 tracks, about 150 timelines of 30 to 150 notes, an instrument and an effect per track; a timeline page with the clips, click a clip to edit its notes in the piano roll, meters in the track headers.
3. Measure: select and move a clip, edit notes in one clip, scroll and zoom, play with meters. Compare with the Rust arrangement where `snapshots.rs` has numbers.

### Round 4b

**Built.** Round 3a ported by hand onto the Solid-style model: engine peaks, the device output, numbers as they play, child fields (at any depth, by path), the clock, `.smooth`/`.fall`/`.hold`, `blend` and `Wave`; the mixer and its window tests pass. Then a song as plain example code (`tooling/agent-eval/examples/song.tsx`, `track.ts`): tracks that each own a synth, a filter, a utility and their `timeline` clips, a meter per track, clips that move on the grid (alt copies, up and down to another track) and resize, a note editor for one clip, M and S, space plays. Window tests (`crates/runtime/tests/window/tools/song.rs`) move a clip (one undo step, only its file changes), add a note in the editor, mute a track, and play with every meter moving and nothing asked of Bun. Snapshots `typescript-song.png` and `typescript-mixer.png`. One fixture (`crates/runtime/tests/shared/song.rs`) feeds the tests, the snapshots and the measurement.

**Fixed on the way:** a component may read a child's `name` and `tool` as it is made: they never change (a rename is another record), and the row of a list needs them as its key. Before, that threw the "read inside a function" error.

**Numbers** (M1 Max, release-fast, `measure_song` in `crates/runtime/tests/window/tools/performance.rs`; "until drawn" includes the harness's polling, about 10 ms):

| Song | 12 tracks, 150 clips (13,500 notes) | 24 tracks, 600 clips (54,000 notes) |
| --- | --- | --- |
| Open until drawn | 220 ms (Bun mount 66 ms, 2 MB of ops) | 620 ms (Bun 223 ms, 8 MB) |
| Idle frame | 4.9 ms | 12.6 ms |
| Frame scrolling / zooming | 4.7 / 3.1 ms | 12.7 / 8.1 ms |
| Frame as it plays, every meter moving | 4.9 ms, 0 Bun requests | 13.0 ms, 0 Bun requests |
| Frame dragging a clip | 4.8 ms, 0 Bun requests | 12.6 ms, 0 Bun requests |
| Click that selects a clip, until drawn | 19 ms (Bun 0.7 ms, 4 KB of ops) | 50 ms (Bun 0.5 ms) |
| Release that moves a clip, until drawn | 20 ms (Bun 8.9 ms, 3 KB) | 52 ms (Bun 25 ms) |
| Note added in the open clip, until drawn | 32 ms (Bun 1.8 ms, 0.5 KB) | not measured: no room, see below |
| Mixer playing, 8 / 32 strips | 0.6 / 1.8 ms a frame, 0 Bun requests | |

For reference, the Rust arrangement scrolls 100 tracks of 100 clips at 3.6 ms a frame (phase 1). Two costs grow with the whole song, not with what changed:
- *Every frame places every shape.* A page draws whole when anything on it moves, and the meters and the playhead move on every frame while it plays, so the play frame costs what an idle frame does: 4.9 ms at 13,500 shapes, 13 ms at 54,000. Culling skips painting, not working out each shape's numbers. Next: keep each space's placed shapes and work out again only those whose live values move.
- *Bun brings a page's stores up to date by walking every record under it after an update*, most likely, which is why one clip move costs 9 ms in Bun at 150 clips and 25 ms at 600. Next: reconcile only the records that changed.

**Painful as an author** (what an agent will hit):
- Inside `update`, a child's `children` and its tool's fields are optional, so every step down needs `?? []` or `?? 0`, and a push becomes a reassignment.
- Track headers are `div`s, which cannot scroll with the timeline (a space holds shapes only, a `div` does not scroll). Past the window's height tracks are cut off and the note editor has no room.
- A group's shapes are not clipped to a box: the notes of a clip draw past its end.
- No JSX fragment: a component that gives several shapes wraps them in a `Group`.
- No colour field: a `text` field and a check before it is a `Paint`.
- Keys are all or nothing: a page with `onKey` takes every plain key, so space no longer plays. The song has no shortcuts (delete, copy) to keep space for the transport.
- Moving a clip to another track is a new record in that track and the old one gone (the id is the path), so its file moves and its name may change. One undo step still.
- A press on a clip that is not selected waits for Bun before the drag preview follows it (phase 2 already said so).

**Too specific (to revisit)**, added to round 4a's list:
- Keys taken all or none, as above: a page should take the keys it names.
- A behaviour runs again in Bun on any change below its record, even a field it does not read: a fader dragged on a child sends Bun a question per move. The window's bindings already track the fields they read; behaviours could too.
- A colour is `#rrggbb` text in shapes but no field kind holds one.

**Verdict.** The model carries an arrangement-shaped page: at a realistic size (12 tracks, 150 clips) every frame stays under 5 ms, nothing that moves asks Bun, and a click or a release is drawn in about 20 ms, all from about 300 lines of example code with no DAW-specific block. A full TS arrangement template is realistic once two SDK costs stop growing with the song (above), and these blocks exist:
- audio clips (stereo, long files, trims, fades), automation lanes into any device number, expression lanes;
- recording into timeline records and audio takes into `assets/`;
- rows that scroll with a space (headers next to a timeline), clipping, menus, text input (rename), file drop, drag to reorder, a built-in device's card inside a page (the track panel);
- keys a page takes one by one.

### Round 5

Goal: the song scales with what shows and what changed, not with the song, and the author pains of round 4b are gone.

**The frame.** A profile said most of a frame of many shapes was GPUI ordering each quad by what it overlaps (`BoundsTree::insert`), not our code. Inside a layer, primitives share one order and paint by kind (quads, then paths, then text), so a run of shapes whose kinds come in that order is now one layer, and an earlier kind starts the next. Then a space keeps an index of its shapes until the next batch of ops: those that stay (no live number places them, no drag moves them) keep where they are, and a frame skips those that do not show before working out anything; those that move are worked out every frame, unless they sit in a group box that does not show.

**Bun.** The 25 ms of a clip move was the first edit after a page opened: the store's copy of the records had never been reconciled, so `reconcile` walked all 54,000 notes once. A store's copy now remembers the frozen value it was made from. A list of children is made again only when an edit not heard yet is inside it. A behaviour's answer says the fields it read; the runtime keeps the answer under the key of those, so a fader on a child, or a clip moving in a track, asks a behaviour that reads names and tools nothing.

**Author pains, fixed generically:**
- A `Group` places its shapes from `x`, `y`, moves with its own `moves`/`grows`, and with `clip` hides and ignores the pointer outside its box: a clip's notes end at its end and move with it.
- A `div` with `follow: { y }` scrolls its children with a space, laid over it so they never make it taller. A page's body is a column, so a top `div` that grows fills the window: the timeline scrolls and the editor has room.
- `<>…</>` fragments.
- A tool's `keys` are the keys it takes; the rest, space too, stay the window's. The song deletes and copies on chosen clips while space plays.
- In an `update`, a child's state has every default, built-in tools' too (generated from their schemas), and only what the change changed is sent. Children are typed as a list to edit, `push` to, or replace, so `?? []` and `?? 0` are gone from the song's edits.
- Not done: moving a child to another parent is still delete and create. An id is its path in the core; a move there is a core change, not an SDK one.

**Numbers** (M1 Max, release-fast; "until drawn" includes about 10 ms of the harness's polling):

| | Round 4b | Round 5 |
| --- | --- | --- |
| 12 tracks / 150 clips: idle, scroll, play frame | 4.9, 4.7, 4.9 ms | 1.7, 1.6, 1.8 ms |
| 24 tracks / 600 clips: idle, scroll, play frame | 12.6, 12.7, 13.0 ms | 3.5, 3.7, 3.6 ms (2.8 ms before clipped groups and the filling layout) |
| 24 / 600: release that moves a clip, Bun | 25 ms | 1.7 ms |
| 12 / 150: release that moves a clip, until drawn | 20 ms | 7.6 ms |
| Scale project, 100 × 100 clips: idle, scroll across, scroll down, play | not measured | 3.2, 3.2, 3.2, 3.7 ms |
| Scale project: open until drawn | not measured | 3.1 s |
| Behaviour questions while a fader drags | one per move | none |

The Rust arrangement scrolls the same scale project at 3.6 ms. 24 tracks cost about twice 12 because twice as many rows show. Opening is now the cost that grows with the song: about 0.8 µs per binding in Bun (round 4a), 3 s at 10,000 clips. Fixing that is the next SDK cost, a leaner reactive core or lazy rows.

**Too specific (to revisit)**, added: a tool's `keys` cannot take space or `r` from the window without taking every plain key's binding context; fine for now, a per-key context would be cleaner.

### Round 6a

Goal: fewer author-facing concepts, toward the cut in the Goal. Mostly renames and merges; the engine needed one real change.

| Before (concepts an author met) | After |
| --- | --- |
| `kind`: effect, instrument, source | `voices` (1 to 8). A sound that reads `input` has an audio input; that alone makes it an effect in a track's picker |
| `controls`: `live`, `trigger`; `set`, `fire`, `send(event)`, `play` | `inputs`: `number(...)`, `bang()`; one `send(input, value?)`; `play` a helper |
| cable keys `number`, `live`, `bang`, `trigger` into a child | `number`, `bang`: a number into a child is an input of that name, else a field a lane moves |
| fields `knob`, `toggle`, `choice`, `pattern`, `sample`, `notes`, `text`, `number`, `list` (9) | `number`, `boolean`, `choice`, `text`, `file`, `list` (6); `pattern()` and `notes()` are helpers that make lists |
| `card`, `page` | `view`, drawn in the room the window gives (`size`, a card or the whole window) |
| `watch()` and `output()`; `watches` | `output()`; a view reads a number output as `now.value(name)` or `outputs` |
| `now.field`, `childField`, `played`, `control`, `watch`, `time`, `output` | `now.value(path, { played?, fraction? })`, `now.elapsed`, `now.devicePeak` |
| drag turns `{ field, child }` or `{ control }` | `{ value: path }`, named as `now.value` names it |
| `SampleChooser`, `Canvas`, a div's `onClick` | `openFile()` and a div; a `Space`; `onPress` everywhere |

Where the code showed a better cut than the plan:
- **The `notes` input stays with every sound.** It is how a sound's voices are played, not a DAW idea; making it a declaration would make every effect that follows a note declare it.
- **Effect and source were one thing in the engine.** `Signals` played them the same; only the audio input a behaviour declared told them apart. Now one voice always has the input, so `Kind::Source` is gone, and a tool that plays its track's notes while sitting in `effects` still sits in the chain.
- **A number is live where it is read.** One `number` field glides in a sound that reads it, reruns a behaviour that reads it, rebinds a view that reads it. A sound declares only the numbers it reads, so a data tool may hold any number of them; a lane moves one only when the sound reads it and it is not whole.
- `transport: true` stays until the clock round gives views a clock to draw. `devices` stays until an eval says otherwise.

**Numbers** (M1 Max, release-fast). Round 5 numbers held:

| | Round 5 | Round 6a |
| --- | --- | --- |
| 12 / 150: idle, scroll, play frame | 1.7, 1.6, 1.8 ms | 1.5, 1.5, 1.7 ms |
| 12 / 150: release that moves a clip, until drawn | 7.6 ms | 7.2 ms |
| 24 / 600: idle, scroll, play frame | 3.5, 3.7, 3.6 ms | 3.7 to 4.2, 3.7 to 4.3, 3.8 to 4.5 ms (three runs) |
| Scale project, 100 × 100: idle, scroll across, play; open | 3.2, 3.2, 3.7 ms; 3.1 s | 3.3, 3.3, 3.6 ms; 3.1 s |

**Agent eval** on the new doc, one run each: `ui-chords` passed (18 turns, $0.61), `ui-paint` passed (17 turns, $0.69). Both used `view`, `number`, `pattern`, `output` read by `now.value`, `onPress`, and a `Space` with `onMove` to paint, unprompted.

**Open questions for Casper:**
- A number a sound does not read is not automatable, so a TS track's volume must be heard by its sound to have a lane. That is "a lane moves what plays"; fine?
- A view now reads at most 8 number outputs of a sound (watches allowed 16).
- `send("notes", event)` replaces `send(event)`: one more word, one less concept.

**Too specific (to revisit)**, from round 4a's list, now done: `kind`, the default `notes` input in types (it is intrinsic now, see above), the card height in the SDK (the window gives the room). Still open: `transport: true`, `sequencer` beside `timeline`, notes as the only event kind, `devices` as only in-code effects, docs placing tools by `state/arrangement/...`.

### Round 6b

Goal: finish the cut by turning the remaining special cases into children.

**Decisions on 6a's questions:** "a lane moves what plays" stays. A sound sends up to 16 numbers again (`MAX_NUMBERS`, one event port each; bangs and notes stay at 8). `send("notes", event)` stays.

| Before | After |
| --- | --- |
| `sequencer` (looping patterns by index, `plays`, launch) beside `timeline` (notes once at a song position) | one `timeline`: `loop`, `plays`, `quantize` and a `launch` input. A song is timelines that play once; a clip launcher is looping timelines, one per pattern |
| `{ to: { meter: name } }`, a connection end only the owner had, read as `now.peak(name)` | a `meter` child, `{ to: { child: "bass-meter" } }`, read by its path: `now.peak("bass-meter")`, `now.peak("t00/level")` |
| `"players"` migration message | gone |

The timeline is the note player of the arrangement with two small additions in `sound-notes`: a pattern can be placed on the song (anchored on `start`, still through a seek, nothing before it) and can play one round. The Rust arrangement uses neither. A record edit places the timeline again; a launch only moves it for the performance, so a render plays what the records say. Only a looping timeline keeps watches, so 600 clips add none.

**What stayed, and why:**
- **Device in and out stay `project.json` connection ends** (`device_input`, `device_output`, `app`). An `audio-out` child would need the parent's own sound as a source, which is the `ownSound` end ARCHITECTURE rejected; a sounding tool at the top would have to split into a parent and a child. And the window opens the microphone and taps apps only for these connections, so a project that never asks never gets a permission prompt. They are where the project meets the machine.
- **MIDI and the computer keys stay machine inputs**, played into the `notes` of what is selected or at the top. ARCHITECTURE keeps them out of the project (one lock-free ring, no record), and where they play is the `select` hook of a later round, not a child.
- **`now.devicePeak`** stays: the device's output is the machine's, like the output meter in the title bar.

**Concepts:** the cut is unchanged; two special cases are gone (the sequencer and the meter end), and two built-in children carry them. A meter costs a record file per meter: the song has one per track, the mixer one per strip.

**Numbers** (M1 Max, release-fast). Unchanged from 6a within noise; opening the scale project reads 100 more records:

| | Round 6a | Round 6b |
| --- | --- | --- |
| 12 / 150: idle, scroll, play frame | 1.5, 1.5, 1.7 ms | 1.5, 1.5, 1.6 ms |
| 12 / 150: release that moves a clip, until drawn | 7.2 ms | 7.4 ms |
| 24 / 600: idle, scroll, play frame | 3.7 to 4.5 ms | 3.5, 3.5, 3.6 ms |
| Scale project, 100 × 100: idle, scroll across, play; open | 3.3, 3.3, 3.6 ms; 3.1 s | 3.1, 3.1, 3.2 ms; 3.4 s |
| Mixer, 8 and 32 strips, playing | 0.6, 1.8 ms | 0.6, 1.8 ms |

**Open:** two timelines into one instrument can put an off and an on of one pitch on the same frame, ordered by connection, so a launcher that swaps patterns on a bar may cut the new note of that pitch. The arrangement never meets it: a track's clips are one player.

**Too specific (to revisit)**, now done: `sequencer` beside `timeline`. Still open: `transport: true`, notes as the only event kind, `devices` as only in-code effects, docs placing tools by `state/arrangement/...`.
