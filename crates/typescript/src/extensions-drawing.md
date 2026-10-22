# Shapes that move

Read `agent-docs/extensions.md` first, and "Values that change" in `agent-docs/extensions-view.md`. `Space`, its shapes and their props are in `extensions/sdk/drawing.ts`; live values and drags in `extensions/sdk/live.ts`.

A `<Space>` draws shapes: `<Rect>`, `<Circle>`, `<Line>`, `<Arc>`, `<Label>` (text), `<Path>`, `<Wave>`, `<Notes>` and `<Group>`. It has units of its own, such as ticks across and notes down (`unit` is points per unit), scrolls over `extent` and zooms in `zoom` by itself, and draws only what shows. The knob, steps, meter and pad are spaces.

What moves on a space moves in the window with no code running, so it keeps up with the pointer and the playhead: give a prop a live value from `now` in place of a number. `now.tick` is where the piece plays, `now.value(name)` a number by its name (a field as a drag turns it, a number input, an output; `"bass/synth.cutoff_hz"` a child's), and `now.drag(id)` how far the drag `id` went, rounded to its `snap`. Combine them with `.plus`, `.times`, `.snap` and the rest; a function may give one. Code hears a drag once, at `onEnd`, and what moved stays where it is until what its code changes then arrives:

```tsx
const roll = { unit: { x: 0.1, y: 12 }, extent: { x: [0, 38400], y: [0, 128] }, zoom: { x: [0.1, 10] }, scroll: { y: 40 } } as const;

view: ({ state, memory, update, now }) => {
  const isPicked = selector(() => memory.selected);
  // One drag for every note: a drag many shapes share crosses once.
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
          <Rect x={() => note.start} y={() => 127 - note.pitch} width={() => note.length} height={1} color="#a9b1ff"
            moves={() => (isPicked(index()) ? ["move"] : undefined)} drag={drag} onPress={() => { memory.selected = new Set([index()]); }} />
        )}
      </For>
      <Line from={[now.tick, 0]} to={[now.tick, 128]} color="#7ee0a0" width={2} />
    </Space>
  );
},
```

- `moves` names the drags a shape moves with, `grows` those that make it wider and taller, as the selected notes above: one small change when the selection does. A `<Group x y>` places its shapes from `x`, `y`, and moves them with its own `moves`; with a `clip` of `{ width, height }` they do not show or hear the pointer outside that box: a clip and its notes.
- `snap` with `grid: true` puts the pressed shape on the grid, wherever it started; `anchor` names the point that lands there, such as `{ x: end }` on the handle at the end of a note. With cmd held `dx` does not snap: round ticks at `onEnd`. Escape lets go of a drag: its code hears only `onRelease`.
- `follow: { x: "roll" }` shows the scroll and zoom of the space `roll` across, and scrolls it: a ruler, or a lane under it. On a `div`, `follow: { y: "timeline" }` scrolls its children with it: the heads of the rows of a timeline.
- `<>...</>` gives several children where one goes, such as a component that gives two shapes; import `Fragment` with `h`.
- A drag can also turn a number as it goes, with no code, as a knob does: `{ y: { value: "cutoff" } }`; for a field the whole drag is one undo step.
- A `Space` hears dropped audio files too, in its units.

## Levels, sounds and the clock

These are live values too, so a meter or a light moves every frame with no code. A path such as `"bass/synth"` reaches a record deeper down.

- `now.peak(path, channel?)`: the loudest sample since the last frame of a `meter` child (`agent-docs/meter.md`), `"bass-meter"` or `"bass/meter"`. `now.devicePeak()` is the device's.
- `now.value(name, { played: true })`: a field as it plays, what a lane or a cable plays into it.
- `.smooth(ms)`, `.fall(perSecond)` and `.hold(seconds)` follow a value over frames: a meter's bar is `level.fall(0.5)`, its peak line `level.hold(1.5).fall(0.5)`. `now.elapsed` counts seconds.
- `blend(from, to, by)` is a colour between two: `blend("#3a1418", "#ff4d5e", now.elapsed.mod(1).lt(0.5))` blinks.
- `<Wave list>` draws a list across its box: `now.scope(path)`, the last sound of a meter; `now.history(value, frames)`, the last values of any live value; `now.file(asset)`, the waveform of a file under `assets/audio/`; `now.take(path)`, what a recorder records so far, and `<Notes list={now.take(path)}>` its notes. `now.input(channel)` is the level of the audio input while a recorder is armed.
- A `Label` of `now.tick` with `unit: "position"`, `"time"` or `"signature"` shows where the piece plays; `now.bpm` its tempo.
