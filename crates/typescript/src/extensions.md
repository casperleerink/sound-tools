# Writing a tool for this project

When no tool does what the composer asks, write one in TypeScript. It plays as soon as you save the file, with no build. A tool takes one of two shapes:

- **A device**: an effect or an instrument, a child of a parent, such as in the chain of a track of the DAW template. Its view is a card its parent shows.
- **The project is the tool**: an experiment, a game, a thing to play live. One record at the top, and its view fills the window. See "A project that is one tool".

To play from the computer keyboard, write nothing: with the computer keys on (cmd-K, or the keyboard button in the title row), the letter rows play what is selected.

This doc is enough for a first tool. Open a deeper one when the tool needs it:

- `agent-docs/extensions-sound.md`: the sound does more than the examples here: feedback, buffers, files, stereo, voices, outputs a parent connects.
- `agent-docs/extensions-view.md`: its own view: elements, values that change on screen, lists, popovers, a control loop, keys and MIDI.
- `agent-docs/extensions-drawing.md`: shapes that move with the playhead, a level or a drag: a piano roll, a meter, a scope, a canvas.
- `agent-docs/extensions-children.md`: it plays other tools: built-in devices, plugins, timelines, lanes, recorders, connections and cables.

## Where it goes

One tool per file: `extensions/<name>.ts`, or `.tsx` when it draws its own view. Make the folder, then run `sound-tools . --inspect` once: the runtime writes the SDK, `extensions/sdk.ts` and `extensions/sdk/`, with `bun.d.ts` and `tsconfig.json`. Do not edit them. Import everything from `./sdk`. Every type and function has its doc in the module that declares it, and `sdk.ts` lists them by module:

| Module of `extensions/sdk/` | Holds |
| --- | --- |
| `fields.ts` | fields and inputs |
| `sound.ts` | signals and their functions |
| `tools.ts` | `tool`, `Context`, children, connections, `NoteEvent` |
| `live.ts` | live values (`now`), `Clock`, pointer events, drags, keys, menus |
| `drawing.ts` | `div` and its `Style`, `Space` and its shapes, `Button`, `Input`, `Icon`, `DeviceCard` |
| `components.ts` | `Knob`, `Fader`, `NumberBox`, `Select`, `Steps`, `Meter`, `Pad` |
| `nodes.ts`, `reactivity.ts` | `For`, `Show`, `Popover`, `Dialog`; `memo`, `selector`, `timeout` |
| `records.ts` | the records of the built-in tools |

A tool needs no entry in `project.json`. While the app has the project open, a save in `extensions/` starts the tools again in a moment: every file loads again, every `memory` starts over, and the runtime writes the tool's doc, `agent-docs/<name>.md`, and lists it in `AGENTS.md`. `--inspect`, `--analyze` and `--render` load the tools too, but write no docs.

## A tool

```ts
import { input, number, phasor, sin, TAU, tool } from "./sdk";

tool({
  name: "wobble",
  title: "Wobble",
  when: "You want the volume of a sound to pulse",
  doc: "A tremolo: the volume moves up and down `rate` times a second. `depth` is how far it dips, 0 not at all, 1 to silence.",
  state: {
    rate: number({ min: 0.1, max: 20, default: 4, unit: "hz" }),
    depth: number({ min: 0, max: 1, default: 0.5 }),
  },
  sound: ({ rate, depth }) => {
    const wave = sin(phasor(rate).times(TAU)).times(0.5).plus(0.5);
    return input.times(depth.times(wave).negate().plus(1));
  },
});
```

- `name` is the `tool` of its records and the name of its doc, and not the name of a built-in tool.
- `when` and `doc` are what the next agent reads in the map and in `agent-docs/<name>.md`. Say what the tool does to the sound and what each field does musically, also between its ends. The runtime adds where the record goes and a table of the fields, so do not repeat those.
- `sound` returns a `Signal`. Left out, it passes `input` through. A sound that reads `input` is an effect; one that does not makes its own: a drone, a generative part, or an instrument that plays its notes.
- `voices`: 1 when left out: one voice that runs all the time and follows the newest held note. Up to 8 for an instrument that plays chords: the sound then runs once per note.

## The sound

`sound` gets the fields and inputs and returns a `Signal`: one value per sample, run for each channel and each voice apart. It is called when the tool loads, once for each combination of choices (at most 256), to build the sound; the sound then plays by itself. So a number field is no number in `sound` but a `Signal` that moves: `if (rate > 5)` is a type error on purpose, because turning a knob must not build the sound again. A `choice` is its value, and `sound` can branch on it.

Combine signals with methods that take a signal or a number (`.plus`, `.times`, `.lt`...) and the functions of `sdk/sound.ts`: oscillators, filters, envelopes, `delay`, `note.freq` and `note.gate` for an instrument, `beat` for the piece. A built-in effect, such as the reverb or a plugin, is not part of a sound: it is a child (`agent-docs/extensions-children.md`).

## Fields

Whatever reads a field follows it: a sound that reads a number glides to it at once, a behaviour or a view that reads it runs again.

| Field | Record holds | In `sound` |
| --- | --- | --- |
| `number({ min, max, default?, integer?, unit?, label? })` | a number; `default` is `min` when left out | a `Signal` that glides; a lane or a cable moves it unless `integer` |
| `boolean({ default, label? })` | `true` or `false` | a `Signal`, 1 or 0 |
| `choice({ options, default, label? })` | one of `options` | the option itself: a change switches to the sound of that option, fading over 10 ms |
| `text({ default?, label? })` | text | nothing |
| `file({ label? })` | the name of a file under `assets/audio/`, such as `"voice.wav"`, or `""` | a table of its sound: one channel, up to 60 s |
| `list(item, { max?, default?, label? })` | a list of `item`: a field, or an object of fields such as `{ name: text(), notes: notes() }` | a list of numbers is a table: `.at(index)` |

`pattern({ length, min, max, default })` is a list of `length` numbers, such as the steps of a sequence. `notes()` is a list of `{"start": 0, "length": 480, "pitch": 60, "velocity": 100}`, times in ticks (960 a quarter). A knob in `unit: "hz"` turns on a log scale; one in `"percent"` still holds 0 to 1. A sound reads at most 32 numbers and booleans.

## Inputs

What plays the tool as a performer plays, which nothing saves and no undo takes back: `number(...)`, which glides over 20 ms from its default, and `bang()`, 1 for the one sample after it is sent. Every sound also has the input `notes`, which plays its voices. `send` plays an input: `send("rate", 2)`, `send("hit")`, `send("notes", { type: "noteOn", pitch: 60, velocity: 0.8 })`.

## Its view

Without `view`, it gets a knob per number field and number input, a row of steps per list of numbers, a chooser per file, options per choice, a switch per boolean and a button per bang. For your own, give `view` in a `.tsx` file. It runs once and makes what shows; what changes goes in a function (`agent-docs/extensions-view.md`).

A card is 144 points tall: more is cut off. It is as wide as it needs, at least 200. So put things side by side: a `Knob` is 56 by 72, so two rows of knobs fill a card; a `Pad` 120 square unless `size` says; a `Meter` 64 tall; `Steps` 14 tall; a line of text about 16.

```tsx
import { Button, Knob, Meter, Pad, Steps, h, type Style, tool } from "./sdk";

const column: Style = { gap: 8 };
const row: Style = { direction: "row", gap: 8, align: "center" };
const pad = { x: "bend", y: "drive" };

tool({
  // name, title, when, doc, state, inputs and sound
  view: ({ send }) => (
    <div style={column}>
      <div style={row}>
        <Knob field="cutoff" label="Cutoff" min={100} max={8000} default={1200} unit="hz" />
        <Pad field={pad} size={72} />
        <Meter value="level" />
      </div>
      <div style={row}>
        <Steps field="steps" playing="step" />
        <Button label="Hit" onClick={() => send("hit")} />
      </div>
    </div>
  ),
});
```

## Changing records

`update(label, change)`, from any of a tool's code and also after an `await`, changes its record and its children's: `change` runs at once on `{ state, children }`, copies of the latest with every field at its default where the file leaves it out. Only what changes lands, on the records as they are then; a record its tool refuses fails the whole update, in `problems.txt`. Updates of one label in a row are one undo step, so a `tick` may update every frame.

```ts
update("Add a note", ({ state }) => {
  state.notes = [...state.notes, { start: 0, length: 480, pitch: 60, velocity: 100 }];
});
```

## A project that is one tool

1. Put one record of the tool at the top: `state/<name>.json`, or `state/<name>/instance.json` for a parent.
2. Name it in `project.json`, `"main": "<name>"`, so the window shows it.
3. Give it a `sound`, with `voices` when it plays chords with `play`.
4. Connect it to the speakers in `project.json` (`agent-docs/project-json.md`): `{"from": {"instance": "<name>", "port": "audio"}, "to": {"device_output": 0}}`.

Its `view` draws the whole window; `size` is the window's room in points, which may be any size: read it in a function, `width={() => size.width}`, or give the top `div` and what fills it `grow`. `{"from": {"device_input": 0}, "to": {"input": {"instance": "<name>", "port": "audio"}}}` plays the microphone into its `input` (tell the composer to use headphones); `{"app": "Music"}` in place of `{"device_input": 0}` plays another app, `{"app": "all"}` every app.

```tsx
import { bang, Circle, feedback, h, lowpass, max, noise, number, Space, tool } from "./sdk";

type Ball = { x: number; y: number; speed: number };

tool({
  name: "rain",
  title: "Rain",
  when: "You want rain drops you can drop with the pointer",
  doc: "Each drop that lands clicks; a press drops one where it is.",
  state: {},
  inputs: { drop: bang(), bright: number({ min: 200, max: 8000, default: 2000 }) },
  memory: () => ({ balls: [] as Ball[] }),
  sound: ({ drop, bright }) => {
    const level = feedback();
    level.set(max(drop, level.times(0.995)));
    return lowpass(noise(), bright).times(level);
  },
  tick: ({ memory, dt, send }) => {
    for (const ball of memory.balls) {
      ball.speed += 2 * dt;
      ball.y += ball.speed * dt;
      if (ball.y > 1) {
        ball.y = 1;
        ball.speed *= -0.5;
        send("drop");
      }
    }
  },
  // Positions from 0 to 1, scaled by the room as it is now.
  view: ({ memory, size }) => (
    <Space grow onPress={({ x, y }) => memory.balls.push({ x: x / size.width, y: y / size.height, speed: 0 })}>
      {() => memory.balls.map((ball) => <Circle x={ball.x * size.width} y={ball.y * size.height} radius={5} color="#60a5fa" />)}
    </Space>
  ),
});
```

## The clock

One clock plays the project, `clock` in all code. Nothing plays it but a tool: no key plays or stops unless a tool takes it. A page with `globalKeys: ["space"]` and `onKey: ({ clock }, { key, down }) => { if (key === "space" && down) (clock.playing ? clock.pause() : clock.play()); }` plays on space. `clock.tick`, `bar`, `beat` and `bpm` move every frame: read them in a handler, and show them with `now.tick` (`agent-docs/extensions-drawing.md`).

## Check your work

1. `bun check -p extensions` checks the types.
2. `problems.txt`, or the problems `sound-tools . --inspect` prints, list what is wrong under `extensions/<file>`: what does not load or build, and, while the window is open, the first tick or click that failed. Code that runs for more than a moment, such as a loop that never ends, stops the tools of its file until the next change.
3. To see what your code does, `console.log` it: while the window is open, `extensions/log.txt` holds the last lines printed since the tools started; `--inspect` prints them too.
4. Use the tool as the composer will: write its record (`"state": {}` is its defaults) where a parent plays it; the parent's doc says which children it plays, such as the `order` of a `chain`. Then measure it with `agent-docs/inspect.md`: `--analyze` shows the loudness and the bands of each moment.
