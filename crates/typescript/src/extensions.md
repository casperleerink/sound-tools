# Writing a tool for this project

When no tool does what the composer asks, write one in TypeScript, with no build. A tool takes one of two shapes:

- **A device**: an effect or an instrument, a child of a parent, such as in the chain of a track of the DAW template. Its view is a card its parent shows.
- **The project is the tool**: an experiment, a game, a thing to play live. One record at the top, and its view fills the window. See "A project that is one tool".

To play from the computer keyboard, write nothing: with the computer keys on (cmd-K), the letter rows play what is selected.

## Where it goes

One tool per file: `extensions/<name>.ts`, or `.tsx` when it draws its own view. Make the folder, then run `sound-tools . --inspect` once: the runtime writes the SDK (`extensions/sdk.ts`, `extensions/sdk/`, `bun.d.ts`, `tsconfig.json`), which you do not edit. Import everything from `./sdk`; `sdk.ts` lists every name by the module that declares it with its doc.

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

- `name` may not be the name of a built-in tool.
- `when` and `doc` are what the next agent reads. Say what the tool does to the sound and what each field does musically, also between its ends. The runtime adds where the record goes and a table of the fields.
- A `sound` that reads `input` is an effect; one that does not makes its own: a drone, a generative part, or an instrument that plays its notes. `voices`, up to 8, runs it once per note, for chords.

## The sound

`sound` returns a `Signal`: one value per sample, run for each channel and each voice apart. It is called when the tool loads, once for each combination of choices (at most 256), to build the sound; the sound then plays by itself. So a number field is no number in `sound` but a `Signal` that moves: `if (rate > 5)` is a type error on purpose, because turning a knob must not build the sound again. A `choice` is its value, and `sound` can branch on it.

Build it with the methods of `Signal` (`.plus`, `.times`...) and the functions of `sdk/sound.ts`: oscillators, filters, envelopes, `delay`, `note` for an instrument, `beat` for the piece. A built-in effect, such as the reverb or a plugin, is not part of a sound: it is a child (`agent-docs/extensions-children.md`).

## Fields

| Field | In `sound` |
| --- | --- |
| `number({ min, max, default?, integer?, unit? })` | a `Signal` that glides; a lane or a cable moves it unless `integer` |
| `boolean({ default })` | a `Signal`, 1 or 0 |
| `choice({ options, default })` | the option itself: a change fades to the sound of that option over 10 ms |
| `text({ default? })` | nothing |
| `file()`: the name of a file under `assets/audio/`, such as `"voice.wav"`, or `""` | a table of its sound in stereo, up to 3 minutes |
| `list(item, { max?, default? })`: `item` is a field, or an object of fields such as `{ name: text(), notes: notes() }` | a list of numbers is a table: `.at(index)` |

`pattern(...)` is a list of numbers, such as the steps of a sequence. `notes()` is a list of `{"start": 0, "length": 480, "pitch": 60, "velocity": 100}`, times in ticks (960 a quarter); `quantize`, `humanize` and `legato` edit one as a piano roll does. A sound reads at most 32 numbers and booleans.

## Inputs

What plays the tool as a performer plays, which nothing saves and no undo takes back: `number(...)`, which glides over 20 ms from its default, and `bang()`, 1 for the one sample after it is sent. Every sound also has the input `notes`, which plays its voices. `send` plays an input: `send("rate", 2)`, `send("hit")`.

## Its view

Without `view`, it gets a control per number, boolean, choice, file, list of numbers, number input and bang. For your own, give `view` in a `.tsx` file. It runs once; what changes goes in a function (`agent-docs/extensions-view.md`).

A card is a row: a `Display` of what the tool does to the sound, then `Controls`, its knobs and other controls in cells (`extensions/sdk/card.ts`). It shows the first columns of cells and keeps the rest behind its expand icon, so put first what matters most.

Name what a colour is for and a step of a scale, not a raw value: `background: "fill"`, `gap: "sm"`, `size="sm"` on a control, `<Text variant="label">` for text. `ThemeColor`, `Style` and `Text` in `extensions/sdk/drawing.ts` list the names.

```tsx
import { Button, Cell, Controls, Display, h, Knob, type Style, tool, Wave } from "./sdk";

const row: Style = { direction: "row" };

tool({
  // name, title, when, doc, state, inputs and sound
  view: ({ now, send }) => (
    <div style={row}>
      <Display caption="Level">
        <Wave x={0} y={0} width={1} height={1} list={now.history(now.value("level"), 120)} low={0} high={1} color="text" />
      </Display>
      <Controls>
        <Knob field="cutoff" label="Cutoff" min={100} max={8000} default={1200} unit="hz" />
        <Knob field="note" label="Note" min={24} max={60} default={36} unit="pitch" />
        <Cell label="Drum">
          <Button label="Hit" size="sm" variant="subtle" onClick={() => send("hit")} />
        </Cell>
      </Controls>
    </div>
  ),
});
```

## Changing records

`update(label, change)` changes its record and its children's, from a view, a handler or `tick`: `change` edits copies of the latest `{ state, children }`, and only what it changes lands. Updates of one label in a row are one undo step, a new one every 5 seconds at most, so a `tick` may update every frame.

```ts
update("Add a note", ({ state }) => {
  state.notes = [...state.notes, { start: 0, length: 480, pitch: 60, velocity: 100 }];
});
```

## A project that is one tool

1. Put one record of the tool at the top: `state/<name>.json`, or `state/<name>/instance.json` for a parent.
2. In `project.json` (`agent-docs/project-json.md`), name it `main` so the window shows it, and connect its `audio` to the speakers. The microphone or another app can play into its `input`: tell the composer to use headphones.
3. Give it a `sound`, with `voices` when it plays chords with `play`.

Its `view` is a `Page` (`extensions/sdk/page.ts`): its `header` shows in the window's title bar, and the rest one under the other, scrolling where the window is too small. A `Panel` is a surface with a title; panels side by side go in a `div` that is a row. What has `grow` takes the room that is left, a panel and the `Space` in it alike, so no code works out a size. `<Page flush>` is one surface to the window's edges, such as a canvas.

```tsx
import { h, Knob, lowpass, now, number, Page, Panel, playing, playPause, saw, smooth, Space, type Style, tool, Transport, Wave } from "./sdk";

const knobs: Style = { direction: "row", gap: "md" };

tool({
  name: "drone",
  title: "Drone",
  when: "You want one long tone to tune by hand",
  doc: "A saw through a filter, while the piece plays. `pitch` is how high it is, `tone` how bright.",
  state: {
    pitch: number({ min: 30, max: 1000, default: 110, unit: "hz" }),
    tone: number({ min: 100, max: 8000, default: 800, unit: "hz" }),
  },
  actions: { play: playPause },
  sound: ({ pitch, tone }) => lowpass(saw(pitch), tone).times(smooth(playing, 50)).times(0.3),
  view: () => {
    // The edges of the display, which grows with the window.
    const { right, bottom } = now.space("level");
    return (
      <Page header={<Transport />}>
        <Panel title="Drone" grow>
          <Space key="level" grow background="well" radius="md">
            <Wave x={0} y={0} width={right} height={bottom} list={now.history(now.devicePeak(), 240)} low={0} high={1} color="green" />
          </Space>
          <div style={knobs}>
            <Knob field="pitch" label="Pitch" min={30} max={1000} default={110} unit="hz" />
            <Knob field="tone" label="Tone" min={100} max={8000} default={800} unit="hz" />
          </div>
        </Panel>
      </Page>
    );
  },
});
```

## The clock

One clock plays the project: `clock` in a view, its handlers and `tick`, and `playing` and `beat` in a sound. A `Transport` in the header plays and stops it and sets its tempo; no key does unless a tool has an action for it, as `actions: { play: playPause }` gives space (`agent-docs/extensions-view.md`). `clock.tick`, `bar`, `beat` and `bpm` move every frame: read them in a handler, and show them with `now.tick` (`agent-docs/extensions-drawing.md`).

## Check your work

1. `bun check -p extensions` checks the types.
2. `problems.txt`, or `sound-tools . --inspect`, lists what is wrong under `extensions/<file>`: what does not load or build, and, while the window is open, the first tick or click that failed. Code that runs for more than a moment, such as a loop that never ends, stops the tools of its file until the next change.
3. `console.log` goes to `extensions/log.txt` while the window is open (the last lines since the tools started), and `--inspect` prints it.
4. Use the tool as the composer will: write its record (`"state": {}` is its defaults) where a parent plays it, as the parent's doc says, such as in the `order` of a `chain`. Then measure it with `--analyze` (`agent-docs/inspect.md`).
5. To see how it looks, `sound-tools . --screenshot screenshot.png` draws the window as the project opens, on macOS only; then read the picture. Add `--click x,y` for each click before it, in points from the window's top left, half the picture's pixels: a click on a track's name shows its cards. Use that one name, so each picture replaces the last.
