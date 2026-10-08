# Writing a tool for this project

When no tool does what the composer asks, write one: an effect that is part of this project, in TypeScript, with its sound in Hum. It plays as soon as you save the file, with no build, and it is a tool like any other: its records go in a track's `effects`, the window gives it a card and offers it under "This project", and the runtime writes a doc for it so the next agent can use it.

## Where it goes

One tool per file: `extensions/<name>.ts`, or `.tsx` when it draws its own card. Make the folder if it is not there, then run `sound-tools . --inspect` once: the runtime writes `extensions/sdk.ts`, the SDK with every type, and `extensions/tsconfig.json` into it. Do not edit them; read `sdk.ts` when you need the exact types. A tool needs no entry in `project.json`.

The runtime needs Bun (https://bun.sh) to run the tools. While the app has the project open, a save loads every file of `extensions/` again within a second, every record of the tool plays the new sound, and the runtime writes the tool's doc, `agent-docs/<name>.md`, and lists it in `AGENTS.md`. `--inspect`, `--analyze` and `--render` load the tools too, but write no docs.

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
- `title`: what the card and the effect picker say.
- `when` and `doc`: what the next agent reads in the map and in `agent-docs/<name>.md`. Say what the tool does to the sound and what each field does musically. The runtime adds where the record goes and a table of the fields, so do not repeat those.
- `state`: the fields of its record. A field name is also a Hum name: lowercase letters, digits and `_`.
- `sound`: its sound in Hum. It runs once per sample on each channel apart, reads the sample that comes in as `in` and sets `out`. The whole language is in `agent-docs/script.md`, under "Hum, the language".

## Fields

| Field | Record holds | In `sound` | A change |
| --- | --- | --- | --- |
| `knob({ min, max, default, unit?, label? })` | a number | a `Param` | glides at once; runs no code |
| `toggle({ default, label? })` | `true` or `false` | a `Param`, 1 or 0 in Hum | glides at once; runs no code |
| `choice({ options, default, label? })` | one of `options` | the option itself | runs `sound` again; the new sound fades in over 10 ms |

`unit` is `"hz"`, `"ms"`, `"db"` or `"percent"`: the card shows it, and a knob in `"hz"` turns on a log scale. A knob in `"percent"` still holds 0 to 1; the card shows it as 0 to 100 %.

A knob is a `Param`, not a number, on purpose: a turn of a knob must not run code, so `sound` cannot branch on it. TypeScript refuses `if (rate > 5)`. Put a `Param` in the Hum, as `${rate}`, and let Hum do the math. A `choice` decides what the sound is made of, such as how many delays there are, and `sound` can use it as any value.

At most 32 knobs and toggles.

## The `hum` tag

`${...}` in `hum` takes:

- a `Param`: its name in Hum.
- a number: written as Hum reads it.
- a string: put in as it is, for a name such as `` `c${i}` ``.
- other `hum` code, or a list of any of these, one after the other.

Lines are trimmed, so indent freely. Do not write `param` lines for the fields: the runtime writes them.

So TypeScript can build Hum that Hum cannot write itself, such as a bank of filters from a loop:

```ts
import { choice, hum, knob, tool } from "./sdk";

const TIMES = [29.7, 37.1, 41.1, 43.7, 31.3, 39.9, 45.1, 33.5];

tool({
  name: "combs",
  title: "Combs",
  when: "You want a metallic, ringing room around a sound",
  doc: "A bank of feedback combs: a small, ringing room. `size` is how long it rings; `combs` how dense it is.",
  state: {
    size: knob({ min: 0, max: 0.95, default: 0.8 }),
    combs: choice({ options: [2, 4, 8], default: 4 }),
    blend: knob({ min: 0, max: 1, default: 0.3 }),
  },
  sound: ({ size, combs, blend }) => {
    const times = TIMES.slice(0, combs);
    return hum`
      ${times.map((ms, i) => hum`
        history c${i}
        c${i} = lowpass(delay(in + c${i} * ${size}, ${ms}), 5000)
      `)}
      wet = (${times.map((_, i) => `c${i}`).join(" + ")}) / ${combs}
      out = mix(in, wet, ${blend})
    `;
  },
});
```

## A card of its own

Without one, the card has a knob per knob and a button per option and per toggle. For your own, give `card` in a `.tsx` file. It draws from the record and changes it; it keeps no state.

```tsx
import { Knob, type Style, h, hum, knob, tool } from "./sdk";

const column: Style = { gap: 8 };
const button: Style = { paddingX: 8, paddingY: 4, radius: 6, background: "#2a2f3a" };

tool({
  // name, title, when, doc, state and sound as above
  card: ({ state, update }) => (
    <div style={column}>
      <Knob path="rate" label="Rate" min={0.1} max={20} default={4} unit="hz" />
      <div style={button} onClick={() => update("Slow down", (next) => { next.rate = 1; })}>
        Slow
      </div>
    </div>
  ),
});
```

`Knob` turns the number at `path` in the record. `update(label, change)` changes the record as one undo step. A `div` takes `style` and `onClick`; `Style` in `sdk.ts` lists every style.

## Check your work

1. `bunx tsc -p extensions` checks the types. The first run downloads TypeScript.
2. `problems.txt` lists what is wrong under `extensions/<file>`: a file that does not load, a tool definition that does not hold, or Hum that does not compile, with the line. Fix it and save; the problem goes.
3. Use the tool as the composer will: write its record in a track, `"state": {}` for its defaults, and name it in the track's `effects`. `--inspect` lists each effect of a track with its tool, or says it is not loaded. Then measure it with `agent-docs/inspect.md`.

## Limits

A tool is an effect: sound in, sound out, each channel apart. It cannot play notes, follow the tempo, or be automated yet. For an effect a built-in tool already is, use the built-in one.
