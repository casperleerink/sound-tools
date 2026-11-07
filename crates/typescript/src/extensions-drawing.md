# Shapes that move

Read `agent-docs/extensions.md` first, and "Values that change" in `agent-docs/extensions-view.md`. `Space`, its shapes and their props are in `extensions/sdk/drawing.ts`; live values and drags in `extensions/sdk/live.ts`.

A `<Space>` draws shapes: `<Rect>`, `<Circle>`, `<Line>`, `<Arc>`, `<Label>` (text), `<Path>`, `<Wave>`, `<Notes>` and `<Group>`. It has units of its own, such as ticks across and notes down (`unit` is points per unit), scrolls over `extent` and zooms in `zoom` by itself, and draws only what shows. The knob, steps, meter and pad are spaces.

Anything that changes every frame, such as the playhead, a level or a blink, is a live value from `now` on a shape in a `Space`, in place of a number: it moves with no code running and keeps up with the pointer. A `div` changes only when code runs, and each change draws the whole view again. `now.tick` is where the piece plays, `now.value(name)` a number by its name (a field as a drag turns it, a number input, an output; `"bass/synth.cutoff_hz"` a child's), `now.drag(id).dx` how far the drag `id` went, rounded to its `snap`, `now.hover(key)` 1 while the pointer is over the shape of that `key`, and `now.space(key).pointerX` where the pointer is over a space. Combine them with `.plus`, `.times`, `.snap` and the rest; a function may give one. Code hears a drag once, at `onEnd`, and what moved stays where it is until what its code changes then arrives:

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

## Levels, sounds and the clock

These are live values too. A path such as `"bass/synth"` reaches a record deeper down.

- `now.peak(path, channel?)`: the loudest sample since the last frame of a `meter` child (`agent-docs/meter.md`), `"bass-meter"` or `"bass/meter"`. `now.devicePeak()` is the device's.
- `now.value(name, { played: true })`: a field as it plays, what a lane or a cable plays into it.
- `.smooth(ms)`, `.fall(perSecond)` and `.hold(seconds)` follow a value over frames: a meter's bar is `level.fall(0.5)`, its peak line `level.hold(1.5).fall(0.5)`. `now.elapsed` counts seconds.
- `blend(from, to, by)` is a colour between two: `blend("#3a1418", "#ff4d5e", now.elapsed.mod(1).lt(0.5))` blinks.
- `<Wave list>` draws a list across its box: `now.scope(path)`, the last sound of a meter; `now.spectrum(path)`, how loud it is from 20 Hz to 20 kHz; `now.history(value, frames)`, the last values of any live value; `now.file(asset)`, the waveform of a file under `assets/audio/`; `now.take(path)`, what a recorder records so far, and `<Notes list={now.take(path)}>` its notes. `now.input(channel)` is the level of the audio input while a recorder is armed or records.
- A `Label` of `now.tick` with `unit: "position"`, `"time"` or `"signature"` shows where the piece plays; `now.bpm` its tempo.

## Canvas

`<Canvas x y width height pixels={(pixel) => colour}>` is a shape of a `Space` filled with a colour per pixel: a light show, a texture that breathes with the sound, a video folded in a mirror. `pixels` runs once and builds the colour with the functions of a sound (`sin`, `mix`, `max`, `wrap`, `hsv`...); the window then works out every pixel 60 times a second with no code running. The colour is `{ red, green, blue }`, each 0 to 1, and `alpha` to let what is under it show. `across` is how many pixels wide it is worked out, 160 when left out, at most 480.

What a pixel reads:

- `pixel.x` and `pixel.y`, 0 to 1 across and down; `pixel.aspect`, so `x.minus(0.5).times(aspect)` and `y.minus(0.5)` measure alike; `pixel.time` in seconds.
- `pixel.noise(x, y, z)`: smooth noise, -1 to 1, a bump per unit, for smoke, water and clouds; `z` of `time` makes it flow.
- `pixel.live(value)`: any live value. `live(now.value("level"))` follows an output of the sound, `live(now.space("stage").pointerX)` the pointer.
- `pixel.wave(list, at)`: a list at `at`, 0 to 1, such as `now.spectrum("meter")`, how loud each band of a meter's sound is from 20 Hz to 20 kHz, or `now.scope("meter")`, its last sound.
- `pixel.video("clip.mp4", { at, loops, x, y })`: a video under `assets/video/`, `at` seconds in: `now.elapsed` loops it, `now.seconds` follows the piece. macOS only.
- `pixel.photo("sky.jpg", { x, y })`: a photo under `assets/images/`, JPEG or PNG: a texture, a mask, a photo to bend.
- `pixel.last(x, y)`: this canvas a step before, for trails and feedback; `last(x.plus(1 / across), y)` scrolls it one pixel a step.
- `pixel.canvas(key, { x, y })`: the last step of the `Canvas` of that `key` in the view, such as a water simulation in one and its light in another. It must show to step.

`pixel.measure("bright", value)` plays the number input `bright` of the tool, every step, with the mean of `value` over the canvas: a canvas or a video plays the sound, gliding, with no code running. A step comes 60 times a second, more often than a video changes: smooth what moves against the step before in the sound. What runs in time (`phasor`, filters, `delay`, the fields) belongs in the sound: send it with `output` and read it with `live`.

```tsx
<Canvas x={0} y={0} width={() => size.width} height={() => size.height} across={240}
  pixels={({ x, y, aspect, time, live, last }) => {
    const [across, down] = [x.minus(0.5).times(aspect), y.minus(0.5)];
    const distance = sqrt(across.times(across).plus(down.times(down)));
    const ring = sin(distance.times(40).minus(time.times(3))).times(live(now.value("swell")));
    const colour = hsv(distance, 0.7, ring);
    const trail = last(mix(x, 0.5, 0.015), mix(y, 0.5, 0.015));
    return { red: max(colour.red, trail.red.times(0.94)), green: max(colour.green, trail.green.times(0.94)), blue: max(colour.blue, trail.blue.times(0.94)) };
  }} />
```

### The stage

`<Stage>` around part of a page's view shows it in a window of its own: full screen on a second screen or a projector, else floating in a corner. The rest stays in the app, such as the knobs that play it. In a `Space key="stage" grow` there, `width={now.space("stage").right}` and `height={now.space("stage").bottom}` fill it.

### Check it

`sound-tools . --screenshot screenshot.png --seconds 3` draws the window after 3 seconds, so what moves shows; the stage is not in it. `--render-video film.mp4 --seconds 10` films the window with its sound, for the composer.
