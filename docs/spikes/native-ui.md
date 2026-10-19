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

(Filled in as the spike goes.)
