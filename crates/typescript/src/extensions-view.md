# The view and the control loop of a tool

Read `agent-docs/extensions.md` first. Elements and their props are in `extensions/sdk/drawing.ts` (`div` with every `Style`, `Button`, `Toggle`, `Icon`, `Input`, `DeviceCard`) and `extensions/sdk/components.ts` (`Knob`, `Fader`, `NumberBox`, `Steps`, `Pad`, `Select`, `SegmentedControl`, `Slider`, `Meter`, `Notice`).

- A control's `field` names a field as `now.value` does, and turns it with no code. Or `value` and `onChange` are what code keeps, such as `memory`, which hears a drag only at its end: to draw along with a drag, turn a number input and draw from `now.value`. A `Slider` takes only a `field`.
- `<Input>` is a text field, or with `rows` a text area; keys typed there play nothing. A dialog's OK reads what `onChange` kept: its click leaves the field the keys, so nothing commits.
- `<DeviceCard path>` shows the card of a child, or a deeper one: a built-in device's own card, a plugin's, or a tool's view.
- A div's `onDrop` hears a dragged div's `carry`, to reorder rows, and audio files from the system, copied into `assets/audio/`.
- `onRelease` hears the button go up wherever the pointer went: a note off. A press reaches every div, `Button` and `Toggle` it is in, `onClick` only the innermost; a `Space` and its shapes keep their presses.

All code of a tool gets the same `Context` (`extensions/sdk/tools.ts`): `state` and `children` with their defaults filled in, `outputs`, `time`, `clock`, `memory`, `now`, `update`, `send` and `play`, and what reaches the window: `openFile`, `render` (a child's sound into a file), `openMenu`, `confirm`, `select` (what the MIDI keyboard plays), `offers` and `addOffer` (the devices that fit a child), `numbers` (what a lane can move).

## Values that change

A view and a component run once. A value that changes (a field, a child, an output, `clock.playing`, `memory`, `size`) goes in a function, which the window runs again when what it read changes; only what it gives changes on screen: `style={() => (state.on ? lit : plain)}`, and `{() => `${state.notes.length} notes`}` as a child. Read such a value in the body itself and the view fails with an error that says how: `x={note.start}` is `x={() => note.start}`. A handler reads values as it runs, so it needs no function around them.

- `<For each={() => state.notes}>{(note, index) => ...}</For>` makes a row per item once and keeps it while its item stays: a moved note changes only what reads it. `index()` follows its place. In a column that scrolls down, rows that each have a fixed `height` and hold nothing tab goes to draw only where they show: a long list costs what shows.
- `<Show when={() => memory.open}>{() => ...}</Show>` makes its part each time it shows; `<Switch fallback>` of `<Match when>{() => ...}</Match>` shows the first that holds.
- `<Popover open onClose>` hangs under the div it is in, `<Dialog>` over the window.
- `timeout(run, ms)` and `interval(run, ms)` stop when what made them goes (the view, a `Show` part, a `For` row, the node whose handler made them) or when what they give is called, as a held button's `onRelease` ends its repeat. `onCleanup(run)` runs `run` then.
- `memo(() => ...)` works a value out once for many functions. `selector(() => memory.selected)` gives `isPicked(key)`, which changes only for the keys that come or go, so a click selects one note at the cost of one.
- Set a field of `memory` to change it, from any code. Replace a `Set` or a `Map`; its type refuses `.add`.

## A control loop

- `memory: () => ({ ... })` makes what the tool keeps and nothing saves, per instance.
- `tick: ({ state, memory, dt, time, play, update }) => { ... }` runs about 30 times a second while the window is open, `dt` seconds apart, not in `--render`. A view that reads what it writes draws again each time: what moves goes on a shape, from `now`.
- What the loop plays at once lands up to 30 ms late. Patterns in time with the bars belong in a `timeline` child (`agent-docs/extensions-children.md`). For a rhythm the loop makes up, play ahead: `time` is the clock of the sound, in seconds, and `play`, and `send` of a bang or notes, with `{ at }` happen on the sample of `at`:

  ```ts
  tick: ({ memory, time, play }) => {
    if (memory.next < time) memory.next = time; // after a pause, start from now
    while (memory.next < time + 0.1) {
      play(60, { at: memory.next });
      memory.next += 0.25;
    }
  },
  ```

- `send("notes", event, { at })` sends a `NoteEvent`, such as `{ type: "cc", controller: 64, value: 1 }`, the pedal down. At most 256 wait, up to 10 s ahead: send a recording as the loop plays. `play(pitch, { hold: true })` sends only the `noteOn`; `{ type: "noteOff", pitch }` ends it, and so does a reload of the code.

## Keys and MIDI

- `onKey: (tool, { key, down, shift, cmd, alt, ctrl }) => { ... }` hears each key of the computer keyboard by name (`"a"`, `"1"`, `"space"`, `"left"`) after a click on its view, or at once on a view at the top: those `keys: ["delete", "cmd-d"]` names, else every plain key. A view at the top also hears its `globalKeys: ["space"]` wherever the focus is, outside a text field. A key with cmd, ctrl or alt only goes down; undo, opening, quitting and the panel stay the app's.
- `onNotes: (tool, event) => { ... }` hears what reaches the tool's `notes`, not its own `send`; its sound plays it too. `event.at` is the `time` it sounded, so a recording sent again with `{ at }` plays as played. The MIDI keyboard plays `notes`, and in a track its clips too.
