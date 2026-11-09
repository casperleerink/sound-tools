# The view and the control loop of a tool

Read `agent-docs/extensions.md` first. The elements and their props are in `extensions/sdk/drawing.ts` and `extensions/sdk/components.ts`. All code of a tool gets the same `Context` (`extensions/sdk/tools.ts`), which also asks the window things: `openMenu`, `confirm`, `openFile`, `render`, `openWindow` (a child's own window, such as a plugin's).

- A control's `field` turns a field with no code. `value` and `onChange` are what code keeps instead, such as `memory`, which hears a drag only at its end: to draw along with a drag, turn a number input and draw from `now.value`.
- A dialog's OK reads what an `<Input>`'s `onChange` kept: a click on a button leaves the field the keys, so nothing commits.
- A press reaches every div, `Button` and `Toggle` it is in, `onClick` only the innermost; a `Space` and its shapes keep their presses. `onRelease` hears the button go up wherever the pointer went: a note off.

## Values that change

A view and a component run once. A value that changes (a field, a child, an output, `clock.playing`, `memory`, `size`) goes in a function, which the window runs again when what it read changes; only what it gives changes on screen: `style={() => (state.on ? lit : plain)}`, and `{() => `${state.notes.length} notes`}` as a child. Read such a value in the body itself and the view fails with an error that says how. A handler reads values as it runs, so it needs no function around them.

- `<For each={() => state.notes}>{(note, index) => ...}</For>` makes a row per item once and keeps it while its item stays: a moved note changes only what reads it. In a column that scrolls down, rows that are divs of a fixed `height` and hold nothing tab goes to draw only where they show: a long list costs what shows.
- `<Show>`, `<Switch>`, `<Popover>` and `<Dialog>` make their part again each time it shows.
- `timeout` and `interval` stop by themselves when what made them goes, such as a `Show` part.
- `selector(() => memory.selected)` gives `isPicked(key)`, which changes only for the keys that come or go, so a click selects one note at the cost of one.
- Set a field of `memory` to change it, from any code. Replace a `Set` or a `Map`; its type refuses `.add`.

## A control loop

- `tick` runs about 30 times a second while the window is open, not in `--render`. A view that reads what it writes draws again each time: what moves goes on a shape, from `now` (`agent-docs/extensions-drawing.md`).
- What the loop plays at once lands up to 30 ms late. Patterns in time with the bars belong in a `timeline` child (`agent-docs/extensions-children.md`). For a rhythm the loop makes up, play ahead: `time` is the clock of the sound, in seconds, and `play` and `send` with `{ at }` happen on that sample:

  ```ts
  tick: ({ memory, time, play }) => {
    if (memory.next < time) memory.next = time; // after a pause, start from now
    while (memory.next < time + 0.1) {
      play(60, { at: memory.next });
      memory.next += 0.25;
    }
  },
  ```

- At most 256 sends wait for their `at`, up to 10 s ahead: send a long recording bit by bit as the loop plays.

## Keys and MIDI

- `onKey` hears keys after a click on the tool's card, or at once when its view is the window; `keys` and `globalKeys` say which (`ToolSpec` in `sdk/tools.ts`). A key with cmd, ctrl or alt only goes down.
- `onNotes` hears what reaches the tool's `notes`, not its own `send`. `event.at` is the `time` it sounded, so a recording sent again with `{ at }` plays as played.
