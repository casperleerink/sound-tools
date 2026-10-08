# Writing a tool for this project

When no tool does what the composer asks, write one: an effect, an instrument or a sound source that is part of this project, in TypeScript, with its sound in Hum. It plays as soon as you save the file, with no build, and it is a tool like any other: its records go in a track, the window gives it a card and offers it under "This project", and the runtime writes a doc for it so the next agent can use it.

## Where it goes

One tool per file: `extensions/<name>.ts`, or `.tsx` when it draws its own card. Make the folder if it is not there, then run `sound-tools . --inspect` once: the runtime writes `extensions/sdk.ts`, the SDK with every type, and `extensions/tsconfig.json`. Do not edit them; read `sdk.ts` when you need the exact types. A tool needs no entry in `project.json`.

The runtime needs Bun (https://bun.sh). While the app has the project open, a save loads every file of `extensions/` again within a second, every record of the tool plays the new sound, and the runtime writes the tool's doc, `agent-docs/<name>.md`, and lists it in `AGENTS.md`. `--inspect`, `--analyze` and `--render` load the tools too, but write no docs.

## A tool

```ts
import { hum, knob, tool } from "./sdk";

tool({
  name: "wobble",
  title: "Wobble",
  when: "You want the volume of a sound to pulse",
  doc: "A tremolo: the volume moves up and down `rate` times a second. `depth` is how far it dips, 0 not at all, 1 to silence.",
  state: {
    rate: knob({ min: 0.1, max: 20, default: 4, unit: "hz" }),
    depth: knob({ min: 0, max: 1, default: 0.5 }),
  },
  sound: ({ rate, depth }) => hum`
    wave = 0.5 + 0.5 * sin(phasor(${rate}) * tau)
    out = in * (1 - ${depth} * wave)
  `,
});
```

- `name`: the `tool` of its records and the name of its doc. Lowercase letters, digits, `-` and `_`, and not the name of a built-in tool.
- `title`: what the card and the pickers say.
- `when` and `doc`: what the next agent reads in the map and in `agent-docs/<name>.md`. Say what the tool does to the sound and what each field does musically, also between its ends: is the middle of a knob half as much, or less? The runtime adds where the record goes and a table of the fields, so do not repeat those.
- `kind`: `"effect"` (the default) goes in a track's `effects` and reads `in`. `"instrument"` is what a track plays, its `instrument.json`: the sound runs once per note, `voices` (1 to 8, default 8) at a time. `"source"` is what a track plays too, but one voice that runs all the time, notes or not: a drone, a texture, a generative part; it follows the newest held note.
- `state`: the fields of its record. A field name is also a Hum name: lowercase letters, digits and `_`.
- `controls`: what its card plays and nothing saves, see below.
- `sound`: its sound in Hum, the language of `agent-docs/hum.md`. Read it before you write one.

## The `hum` tag

`sound` is called once, when the tool loads or a choice changes, and returns Hum. A field or a control is a `Param`, not a number, on purpose: a turn of a knob must not run code, so `sound` cannot branch on it, and TypeScript refuses `if (rate > 5)`. Put it in the Hum as `${rate}`, or `${steps}[i]` for a pattern, and let Hum do the math. A `choice` is its value, and `sound` can branch on it.

`${...}` in `hum` takes a `Param`, a number, a string (a name, such as `` `c${i}` ``), other `hum` code, or a list of these. Lines are trimmed, so indent freely. Do not write `param`, `live` or `trigger` lines for the fields and controls: the runtime writes them. So TypeScript builds Hum a hand would repeat, such as a bank of combs from a loop:

```ts
sound: ({ size, combs }) => {
  const times = [29.7, 37.1, 41.1, 43.7, 31.3, 39.9, 45.1, 33.5].slice(0, combs);
  return hum`
    ${times.map((ms, i) => hum`
      history c${i}
      c${i} = lowpass(delay(in + c${i} * ${size}, ${ms}), 5000)
    `)}
    out = mix(in, (${times.map((_, i) => `c${i}`).join(" + ")}) / ${combs}, 0.3)
  `;
},
```

## Fields

| Field | Record holds | In `sound` | A change |
| --- | --- | --- | --- |
| `knob({ min, max, default, unit?, label? })` | a number | a `Param` | glides at once |
| `toggle({ default, label? })` | `true` or `false` | a `Param`, 1 or 0 in Hum | glides at once |
| `choice({ options, default, label? })` | one of `options` | the option itself | runs `sound` again; the new sound fades in over 10 ms |
| `pattern({ length, min, max, default, label? })` | a list of `length` numbers | a list in Hum: `${steps}[i]` | at once |

`unit` is `"hz"`, `"ms"`, `"db"` or `"percent"`: the card shows it, a knob in `"hz"` turns on a log scale, and one in `"percent"` still holds 0 to 1. At most 32 knobs and toggles.

## Controls

What the card plays as a performer plays, which nothing saves and no undo takes back:

- `live({ min, max, default, label? })`: a number the card moves, such as an XY pad. A `Param` in `sound`, gliding over 20 ms; it starts at its default.
- `trigger({ label? })`: a bang: a `Param` that is 1 in Hum for the one sample after the card fires it.

A `watch name = value` line of the Hum shows a value to the card, as `watches[name]`.

## Its card

Without `card`, the card has a knob per knob and live control, a row of steps per pattern, and a button per option, toggle and trigger. For your own, give `card` in a `.tsx` file. It draws from the record and the watches and plays what it needs; it keeps no state of its own.

```tsx
import { Knob, Meter, Pad, Steps, h, type Style, tool } from "./sdk";

const row: Style = { direction: "row", gap: 8, align: "center" };
const button: Style = { paddingX: 8, paddingY: 4, radius: 6, background: "#2a2f3a" };

tool({
  // name, title, when, doc, state, controls and sound
  card: ({ state, watches, update, set, fire }) => (
    <div style={{ gap: 8 }}>
      <div style={row}>
        <Knob path="cutoff" label="Cutoff" min={100} max={8000} default={1200} unit="hz" />
        <Pad x="bend" y="drive" />
        <Meter watch="level" />
      </div>
      <Steps path="steps" playing="step" />
      <div style={button} onClick={() => fire("hit")}>Hit</div>
    </div>
  ),
});
```

| Element | Does |
| --- | --- |
| `<div style onClick>` | A box; `Style` in `sdk.ts` lists every style. Text goes inside. |
| `<Knob path label min max default unit?>` | Turns the number at `path` in the record: a drag is one undo step. With `live="name"` instead of `path` it plays a live control. |
| `<Steps path max? playing?>` | A row of steps on a pattern: a click turns a step on (to `max`, 1 by default) or off. `playing` names a watch whose value is the step that lights up. |
| `<Meter watch label?>` | A bar that shows a watch from 0 to 1. |
| `<Pad x y size?>` | A square for the pointer: across moves the live control `x`, up moves `y`. |

The card gets `state`, the record; `watches`, the last value of each watch, and draws again as they move; `update(label, change)`, which changes the record as one undo step; `set(control, value)`, which moves a live control; and `fire(control)`, which fires a trigger.

## Check your work

1. `bunx tsc -p extensions` checks the types. The first run downloads TypeScript.
2. `problems.txt`, or the problems `sound-tools . --inspect` prints, list what is wrong under `extensions/<file>`: a file that does not load, a tool definition that does not hold, Hum that does not compile, with the line. Fix it and save; the problem goes.
3. Use the tool as the composer will: write its record in a track (`"state": {}` is its defaults; an effect is also named in the track's `effects`). `--inspect` lists each track's instrument and effects with their tools. Then measure it with `agent-docs/inspect.md`.

## Limits

A tool is sound in or out, with notes for an instrument or a source. It cannot be automated yet. For a sound a built-in tool already makes, use the built-in one.
