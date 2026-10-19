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
4. Later round, not this one: menus, text input, file drop, engine meters, a built-in device's card inside a page, drag reorder. Proof: a mixer and track panel slice.

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
