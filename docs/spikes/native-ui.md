# Spike: TS describes the UI, native runs the motion

## Goal

Find out if a tool of the project can draw any interface, up to a DAW arrangement, with TypeScript as the description and GPUI as the renderer. The model is React Native after Reanimated and Gesture Handler: JS owns structure and meaning, native owns everything that moves.

The rule, the same as the sound graph's: **motion never crosses to Bun**. Bun hears about a gesture once, when it ends, and answers with a new tree.

## Decided constraints

- Branch `spike/native-ui`, never main. A spike: findings matter more than polish, but the code follows ENGINEERING.md.
- General building blocks, not DAW widgets. No `Timeline` or `PianoRoll` element.
- Existing native special cases (`Knob`, `Steps`, `Meter`, `Pad`, `Canvas`) that the new blocks can express become TS components in `sdk.ts`. That is a success signal, not churn.
- Full trees over the wire until a measurement says otherwise. Keys on elements so native state (a drag, hover, scroll) survives a new tree.
- TS stays typed end to end: a wrong binding or prop is a type error.

## Building blocks to try

1. **Live values**: a numeric prop can be a live expression instead of a number, worked out natively every frame. Sources: transport tick, watches, record fields, live controls, gesture values, scroll and zoom. Same method names as `Signal` (`.plus`, `.times`, ...) plus `snap`.
2. **Space**: an area whose children sit in its own units (e.g. ticks × rows). Native scroll, zoom, wheel and pinch; skips what is off screen.
3. **Shapes and text as elements** placed in a Space or a box, absolute or flex. Replaces the canvas shape list.
4. **Gestures**: a drag on an element gives live values (start, delta, in Space units, snapped) that any element can bind to: a move preview, a marquee, a resize. On release TS gets one call with the result. A drag can instead write a record field live (knob, fader, velocity bar) as one undo step, with no Bun.
5. **Pointer and keys in full**: button, click count, modifiers, position in units; hover and pressed styles; keys with modifiers (cmd-z stays the window's).

## Proof set

1. `Knob`, `Steps`, `Meter`, `Pad` rebuilt in TS on the blocks; old native code removed. The knob turns and shows automation without Bun.
2. A piano roll page on a `notes` field: zoom, scroll, draw, move many selected notes with snap, resize, marquee, playhead, cmd-A/C/V/D, delete.
3. Stress: the piano roll with 2,000 notes.
4. Round 3, every other kind of real-time UI, all through live values, not one-off elements:
   - Engine to UI: track and master meters from the engine's peaks, automation shown moving on a knob, a waveform that grows while recording, a scope or spectrum (a live list, not one number).
   - UI on its own: time and smoothing ops (meter fall-off, peak hold, fades, a blinking record button), live colours.
   - Plus menus, text input, file drop, a built-in device's card inside a page, drag reorder.
   - Proof: a mixer and track panel slice.

## How to verify

- Window tests (`crates/runtime/tests/window`) drive the piano roll with a simulated mouse and keys and check the records and undo steps.
- A snapshot PNG of the piano roll page.
- Performance, measured by a repeatable test from phase 1 on, numbers in Findings with machine and profile: native frame cost at 100 / 1,000 / 2,000 elements (idle and dragging), live value cost per frame, Bun round trip and tree size at the same sizes, and old path (Bun per drag step) vs new path (Bun at release only). Compare with the Rust note editor.

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
- EVAL_RESULTS

**Verdict.** The model works and the protocol is simpler: no versions, no handler cache, no read/unchanged dance, no render requests. A discrete event now costs what it changes on the wire (a click is 0.7 KB, not 1 MB) and under a millisecond on the main thread. At 2,000 notes the end-to-end win is small (click 23 to 16 ms, release 25 to 23 ms), and a big selection change is slower (12 to 23 ms). Phase 2's off-thread tree read was already cheap at that size. At 20,000 notes everything discrete is faster (click 178 to 106 ms). What then limits is not the UI model but the record round trip and native culling. The cost: an 800-line runtime, and agents must wrap what changes in functions, made safe by an error at load. For an arrangement (10,000+ clips) I'd adopt it, together with list-item record edits, culling before placing, a compact live-value format and stable item ids for selection.

