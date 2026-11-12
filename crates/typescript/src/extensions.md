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
- What sounds with nothing playing it, a drone or the hiss of a tape, sounds on while the piece is stopped unless it follows `playing`: `.times(smooth(playing, 50))`.
- An instrument that makes its own notes, a sequencer, follows the notes that reach it or goes on a track of its own: clips on its track would show and play nothing.

## The sound

`sound` returns a `Signal`: one value per sample, run for each channel and each voice apart. It is called when the tool loads, once for each combination of choices (at most 256), to build the sound; the sound then plays by itself. So a number field is no number in `sound` but a `Signal` that moves: `if (rate > 5)` is a type error on purpose, because turning a knob must not build the sound again. A `choice` is its value, and `sound` can branch on it.

Build it with the methods of `Signal` (`.plus`, `.times`...) and the functions of `sdk/sound.ts`: oscillators, filters, envelopes, `delay`, `note` for an instrument, `beat` for the piece. A built-in effect, such as the reverb or a plugin, is not part of a sound: it is a child (`agent-docs/extensions-children.md`).

## Fields

| Field | In `sound` |
| --- | --- |
| `number({ min, max, default?, integer?, unit? })`: `unit` is how every control reads it, `"hz"`, `"ms"`, `"db"`, `"semitones"`, `"pitch"`; from 0 to 1 it reads as a percent unless `unit: "plain"` | a `Signal` that glides; a lane or a cable moves it unless `integer` |
| `boolean({ default })` | a `Signal`, 1 or 0 |
| `choice({ options, default })` | the option itself: a change fades to the sound of that option over 10 ms |
| `text({ default? })` | nothing |
| `file()`: the name of a file under `assets/audio/`, such as `"voice.wav"`, or `""` | a table of its sound in stereo, up to 3 minutes |
| `list(item, { max?, default? })`: `item` is a field, or an object of fields such as `{ name: text(), notes: notes() }` | a list of numbers is a table: `.at(index)` |

`pattern(...)` is a list of numbers, such as the steps of a sequence, whose `default` may be a list: a groove to start with. `notes()` is a list of `{"start": 0, "length": 480, "pitch": 60, "velocity": 100}`, times in ticks (960 a quarter); `quantize`, `humanize` and `legato` edit one as a piano roll does. A sound reads at most 32 numbers and booleans.

## Inputs

What plays the tool as a performer plays, which nothing saves and no undo takes back: `number(...)`, which glides over 20 ms from its default, and `bang()`, 1 for the one sample after it is sent. Every sound also has the input `notes`, which plays its voices. `send` plays an input: `send("rate", 2)`, `send("hit")`.

## Its view

Without `view`, it gets a control per number, boolean, choice, file, `pattern`, number input and bang. Your own `view` goes in a `.tsx` file. It runs once; what changes goes in a function (`agent-docs/extensions-view.md`).

A card is a `Display` of what the tool does to the sound, or a `StepGrid` of rows of `Steps` in its place, then `Controls`, the one home of its controls: each control on a field, and in a `Cell` what has none (`extensions/sdk/card.ts`). The first columns show, the rest wait behind the card's expand icon: put first what matters most. A row of steps holds its own controls, such as a drum's level, and the grid names their columns.

A control with a `field` takes its range, unit and label from it and shows what plays, also while a lane moves it. A display says what it is measured by with `scale` and `zero`, not with shapes of its own, and what no mark says with a `caption`, which starts with a capital: `Pitch, ±50 cents`. `now.scope()` is the sound the tool plays: its wave fills the display over its own zero line and takes no marks. A part shows how it is used, so a view adds no line of instructions. Name what a colour is for and a step of a scale, not a raw value: `background: "fill"`, `<Row gap="sm">`, `<Text variant="label">`; `extensions/sdk/drawing.ts` lists the names. Sound is `green`; a value is `text`, and so is its history: a view picks no hue of its own.

```tsx
import { Button, Cell, Controls, Display, Fragment, h, Knob, tool, Wave } from "./sdk";

tool({
  // name, title, when, doc, state, inputs and sound
  view: ({ now, send }) => (
    <>
      <Display>
        <Wave list={now.scope()} color="green" />
      </Display>
      <Controls>
        <Knob field="cutoff" />
        <Knob field="note" />
        <Cell label="Drum">
          <Button label="Hit" variant="subtle" onClick={() => send("hit")} />
        </Cell>
      </Controls>
    </>
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

Its `view` is a `Page` (`extensions/sdk/page.ts`): the transport shows in the window's title bar, and shows and sets the tempo of a tool that plays in the bars, so a page adds no tempo control; the rest is one under the other in the middle of the window. A `Panel` is a surface with a title, as wide as what is in it; its `Display` and its `Controls` go side by side in a `Row`, as a card has them, and a knob with its label goes in `Controls` here too. A `StepGrid` takes its `Controls` as its last child. `grow` is for a surface that needs the room, a canvas or a piano roll, not for a picture of a sound. `<Page flush>` is one surface to the window's edges.

```tsx
import { Controls, Display, type Drag, h, Knob, Line, lowpass, mtof, number, Page, Panel, playing, Row, saw, smooth, tool, Wave } from "./sdk";

tool({
  name: "drone",
  title: "Drone",
  when: "You want one long tone to tune by hand",
  doc: "Two saws a little apart, one in each ear, through a filter, while the piece plays. `pitch` is how high they are, `tone` how bright.",
  state: {
    pitch: number({ min: 24, max: 72, default: 45, unit: "pitch" }),
    tone: number({ min: 100, max: 8000, default: 800, unit: "hz" }),
  },
  sound: ({ pitch, tone }) => {
    const ear = (apart: number) => lowpass(saw(mtof(pitch).times(apart)), tone).times(smooth(playing, 50)).times(0.3);
    return { left: ear(1), right: ear(1.005) };
  },
  view: ({ now }) => {
    // How bright it is, across the display: a line to drag.
    const tone = now.value("tone", { fraction: true });
    const brighten: Drag = { x: { value: "tone", absolute: true } };
    return (
      <Page>
        <Panel title="Drone">
          <Row>
            <Display cursor="ew-resize" drag={brighten}>
              <Wave list={now.scope()} color="green" />
              <Line from={[tone, 0]} to={[tone, 1]} color="text" />
            </Display>
            <Controls>
              <Knob field="pitch" />
              <Knob field="tone" />
            </Controls>
          </Row>
        </Panel>
      </Page>
    );
  },
});
```

## The clock

One clock plays the project: `clock` in a view, its handlers and `tick`, and `playing` and `beat` in a sound. The transport in a page's header plays and stops it and gives its tool space to play and pause. `clock.tick`, `bar`, `beat` and `bpm` move every frame: read them in a handler, and show them with `now.tick` (`agent-docs/extensions-drawing.md`).

## Check your work

1. `bun check -p extensions` checks the types.
2. `problems.txt`, or `sound-tools . --inspect`, lists what is wrong under `extensions/<file>`: what does not load or build, and, while the window is open, the first tick or click that failed. Code that runs for more than a moment, such as a loop that never ends, stops the tools of its file until the next change.
3. `console.log` goes to `extensions/log.txt` while the window is open (the last lines since the tools started), and `--inspect` prints it.
4. Use the tool as the composer will: write its record (`"state": {}` is its defaults) where a parent plays it, as the parent's doc says, such as in the `order` of a `chain`. Then measure it with `--analyze` (`agent-docs/inspect.md`).
5. `sound-tools . --screenshot screenshot.png` draws the window as the project opens, on macOS only: read the picture. Add `--click x,y` for each click before it, in points from the window's top left, half the picture's pixels: a click on a track's name shows its cards. Add `--play 2` to draw it after the piece played 2 seconds: meters, scopes and steps as they are then. Use that one name, so each picture replaces the last.
