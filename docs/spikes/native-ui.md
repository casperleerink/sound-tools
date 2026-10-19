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
1. Keyed tree patches, so a click costs what it changes.
2. Engine meters (peaks) and lists (a waveform, a scope) as live values.
3. Menus and text input (rename, the tempo field).
4. File drop from the OS onto a space, in units.
5. A built-in device's own card inside a page, for the rack.
6. Drag reorder of boxes, for effect chains and tracks.
7. A pressed shape moving with its own drag before Bun answers, and sizes in points inside unit spaces.

### Round 3a

**Works.** Everything that moves on its own goes through live values, with no new element but `Wave`. `tooling/agent-eval/examples/mixer.tsx` is a mixer page: a strip per `tone` child with a fader that turns its utility's `gain_db` natively, a pan knob that shows what a cable plays into it, mute and solo, stereo meters with fall-off and peak hold, a master meter, the device output, a scope, the level's history and a blinking light. `crates/runtime/tests/window/tools/mixer.rs` checks that the meters follow the sound, the fader is one undo step and the pan follows the cable, each with zero Bun requests; `typescript-mixer.png` shows five tones playing.

**What it took:**
- *Engine sources.* `now.peak` reads peaks; `now.output` reads the device output through `SharedPeaks` (`sound_ui`), because `Peaks` have one reader and the transport reads the output too. A meter is the end of a connection, `{ to: { meter } }`: one small processor that records peaks and a scope. `now.played` reads lanes, or a watch a number cable now writes, else the record.
- *State across frames.* `.smooth`, `.fall` and `.hold` keep state under a key `sdk.ts` hashes from the expression, so the same expression drawn twice is one, as a signal used twice is. No identity from the tree was needed.
- *Per-child-field read tracking.* Without it, a fader drag on a child asked Bun for a tree on every move, because the page reads `children`. Same fix as phase 1, one level down.

**Lists, the design choice.** A list is not a live value. A `Wave` shape takes a list source: `scope` (the frames a meter keeps) or `history` (any live value, one sample a frame, keyed like the easings). The window copies the values each frame and draws one point per column: a line, or a band of quads from min to max. Rejected: live lists with ops (map, FFT); nothing needed them yet. A spectrum would be a third source worked out natively from the scope.

**Awkward:**
- Strip order is name order: the first strip `sway` moves is `bass`, not the first one written.
- A child's field range comes from its automation entry; a field that is not automatable has no fraction.
- `now.childField` names are strings, not typed by the children.
- Bands cost: 16 waves of 8192 frames as bands are 4.3 ms a frame (10 ms as filled paths). Lines are 1.3 ms.
- The page asks for a frame at display rate while anything animates, even when silent. Stopping once easings settle would cost nothing to add.
- Keyed tree patches: not built. Nothing here needed them; every tree in round 3a came only from clicks.

**Numbers.** M1 Max, release-fast, CPU only. Run with `cargo nextest run -p runtime --test window --cargo-profile release-fast --run-ignored only --no-capture -E 'test(/performance_of_the_mixer|performance_of_waves/)'`.

| | Frame | Bun requests |
| --- | --- | --- |
| Mixer, 8 strips, playing | 0.60 ms | 0 in 120 frames |
| Mixer, 32 strips, playing | 1.84 ms | 0 in 120 frames |
| 1 wave of 8192 frames, line | 0.13 ms | 0 |
| 16 waves, lines / bands | 1.3 / 4.3 ms | 0 |

**Verdict.** The model covers real-time UI beyond drags with the same mechanism: engine levels, cables, the clock, easing and lists are sources of live values, worked out natively once a frame. A playing mixer of 32 strips costs under 2 ms a frame and asks Bun for nothing.
