# Shapes that move

Read `agent-docs/extensions.md` first, and "Values that change" in `agent-docs/extensions-view.md`. `Space`, its shapes and their props are in `extensions/sdk/drawing.ts`; live values and drags in `extensions/sdk/live.ts`.

A `<Space>` draws shapes (`Rect`, `Circle`, `Line`, `Arc`, `Label`, `Path`, `Wave`, `Notes`, `Group`) in units of its own, such as ticks across and notes down. It scrolls and zooms by itself and draws only what shows. The knob, steps, meter and pad are spaces.

What changes every frame, such as the playhead, a level or a blink, is a live value from `now` on a shape, in place of a number: it moves with no code running and keeps up with the pointer. A `div` changes only when code runs, and each change draws the whole view again. Combine live values with `.plus`, `.times`, `.snap` and the rest; a function may give one. A drag moves its shapes in the window, and code hears it once, at `onEnd`:

```tsx
const roll = { unit: { x: 0.1, y: 12 }, extent: { x: [0, 38400], y: [0, 128] }, zoom: { x: [0.1, 10] }, scroll: { y: 40 } } as const;

view: ({ state, memory, update, now }) => {
  const isPicked = selector(() => memory.selected);
  // One drag for every note: a drag many shapes share goes to the window once.
  const drag: Drag = {
    id: "move", snap: { x: 240, grid: true }, axis: "x",
    onEnd: ({ dx }) => update("Move notes", ({ state }) => {
      state.notes = state.notes.map((note, index) => (memory.selected.has(index) ? { ...note, start: note.start + dx } : note));
    }),
  };
  return (
    <Space key="roll" width={800} height={400} {...roll}>
      <For each={() => state.notes}>
        {(note, index) => (
          <Rect x={() => note.start} y={() => 127 - note.pitch} width={() => note.length} height={1} color="blue"
            moves={() => (isPicked(index()) ? ["move"] : undefined)} drag={drag} onPress={() => { memory.selected = new Set([index()]); }} />
        )}
      </For>
      <Line from={[now.tick, 0]} to={[now.tick, 128]} color="green" width={2} />
    </Space>
  );
},
```

- `moves` names the drags a shape moves with, `grows` those that make it wider and taller: one small change when the selection does. A `<Group x y>` moves its shapes together; with a `clip` they do not show or hear the pointer outside it: a clip and its notes.
- `snap` with `grid: true` puts the pressed shape's `anchor` on the grid, wherever it started. Escape lets go of a drag: its code hears only `onRelease`.
- `follow: { x: "roll" }` shows and scrolls with the space `roll` across: a ruler, or a lane under it. On a `div`, `follow: { y: "timeline" }` scrolls its children with it: the heads of the rows of a timeline.
- `<>...</>` gives several children where one goes; import `Fragment` with `h`.
- A drag can also turn a number as it goes, with no code, as a knob does: `{ y: { value: "cutoff" } }`.

## Levels, sounds and the clock

These are live values too (`Now` in `sdk/live.ts`):

- `now.peak` of a `meter` child (`agent-docs/meter.md`), `now.devicePeak()` of what the device plays; `now.value(name)`, a field as it plays.
- `.smooth`, `.fall` and `.hold` follow a value over frames, as a meter's bar and peak line do. `blend(from, to, by)` is a colour between two: `blend("red/20", "red", now.elapsed.mod(1).lt(0.5))` blinks.
- `<Wave list>` draws `now.scope` (the sound the tool plays, or a child by its path), `now.history` (the last values of any live value), `now.file` (a file's waveform) or `now.take` (what a recorder records so far; `<Notes>` its notes), over its whole space unless it says a place.
- A `Label` of `now.tick` with `unit: "position"`, `"time"` or `"signature"` shows where the piece plays.
