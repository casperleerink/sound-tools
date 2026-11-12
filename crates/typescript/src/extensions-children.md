# A tool that plays other tools

Read `agent-docs/extensions.md` first. The types of children and connections are in `extensions/sdk/tools.ts`, the records of the built-in tools in `extensions/sdk/records.ts`.

A tool with a `behaviour` is a parent. Its record is `state/<name>/instance.json`, and every record next to it, `state/<name>/<child>.json`, is a child: a built-in device such as `instrument.synth`, `delay` or a `plugin` (enable its extension in `project.json`: `plugin-host` for a plugin), a tool of this project, or one with only `state`: data. The behaviour is a pure function of the record and the children that says how they connect. A parent has one voice.

```ts
import { list, text, tool, type Connection, type Declaration } from "./sdk";

tool({
  name: "band",
  title: "Band",
  when: "You want a sound through a chain of effects that a record names",
  doc: "Plays the child `lead` through the children `effects` names, in order.",
  state: { lead: text({ default: "drone" }), effects: list(text(), { max: 4 }) },
  behaviour: ({ state: { lead, effects } }): Declaration => {
    const chain = [lead, ...effects];
    const connections: Connection[] = chain.slice(1).map((effect, index) => ({
      from: { child: chain[index] },
      to: { child: effect },
    }));
    connections.push({ from: { child: chain[chain.length - 1] }, to: { port: "audio" } });
    return { connections };
  },
});
```

- Give the behaviour the return type `Declaration`: without it TypeScript lets a misspelled key through and blames the whole behaviour.
- A parent whose children are fixed names them in `children`, as `loops` below does: a `const` with `as const`, and the return type `Declaration<typeof children>`. TypeScript then checks each connection, `play`, `send` and child `state`. For a tool of this project, export what `tool` returns (`export const drone = tool({ ... })`), import it and name it: `{ lead: drone, fx: "utility" }`. `takes("notes")` is a slot for any tool that takes notes. `children: {}` allows none.
- A child may be a parent too, at `state/<name>/<child>/instance.json`, at any depth; `update` edits, adds and takes out records at any depth, and one taken out takes all it holds. A name in `children` that ends in `*` stands for every name that starts as it does: `{ "ret-*": bus, "*": track }`.
- Without typed `children`, cast a child's state: `children.find((child) => child.name === "bass")!.state as TimelineState`.
- The behaviour runs in Bun when what it reads changes, and its answer plays a moment later. `--render` and `--analyze` wait for it. A missing child or port, or a behaviour that throws, is a problem of the parent's record; the rest plays, and a parent whose behaviour throws plays what it declared last.

## Connections

- A sound port of a child is `audio` when left out: `{ child: "duck", port: "sidechain" }`. `{ child: "bass/mix" }` is a port of a child of a child, so a parent wires across its children, such as a send of one track into another (write such a path `as const`).
- Leave out `child` for a port of the parent itself: from `{ port: "audio" }` comes its audio input, from `{ notes: "notes" }` its `notes`; to `{ port: "audio" }` goes what it plays, through its own `sound` as `input`; to `{ number }`, `{ bang }` or `{ notes }` an output of that name, which its own parent connects.
- Events connect by the key of their kind at both ends, to the sample:
  - `{ from: { child: "lfo", number: "cutoff" }, to: { child: "filter", number: "cutoff_hz" } }`: the number `lfo` sends with `output` turns `cutoff_hz` of `filter`, in its units. Nothing is saved; without the connection the field glides back to its record.
  - `{ from: { child: "ears", bang: "onset" }, to: { child: "drop", bang: "hit" } }` sends a bang.
  - `{ from: { notes: "notes" }, to: { child: "piano", notes: "notes" } }` plays an instrument from the keyboard.
- A parent plays its children by name: `play(pitch, { child: "synth" })`, `send({ child: "lfo", number: "rate" }, 2)`.

## Blocks

The built-in children a parent plays with each have a doc, listed in `AGENTS.md`: `timeline` (notes in time, once or round after round), `lane` (automation), `region` (an audio file), `recorder` (takes), `fader` and `meter`. A parent's view reads the outputs of each child as `outputs["<child>.<name>"]`.

A clip launcher, with two looping timelines `low` and `high`, an instrument `synth` and a utility `fx` as its children:

```ts
import { h, tool, type Declaration } from "./sdk";

const children = { low: "timeline", high: "timeline", synth: "instrument.synth", fx: "utility" } as const;
const loops = ["low", "high"] as const;

tool({
  name: "loops",
  title: "Loops",
  when: "You want bass loops to launch in time with the bars",
  doc: "Loops the timelines `low` and `high` on `synth` through `fx`; its view starts one and stops the other, or stops both, at the next bar.",
  state: {},
  children,
  behaviour: (): Declaration<typeof children> => ({
    connections: [
      { from: { child: "low", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "high", notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { child: "fx" } },
      { from: { child: "fx" }, to: { port: "audio" } },
    ],
  }),
  view: ({ outputs, send }) => {
    const launch = (chosen?: string) => {
      for (const loop of loops) send({ child: loop, number: "launch" }, loop === chosen ? 1 : 0);
    };
    return h("div", { style: { direction: "row", gap: "sm", padding: "lg" } },
      [...loops, undefined].map((loop) => {
        const plays = () => loop !== undefined && outputs[`${loop}.playing`] === 1;
        const waits = () => loop !== undefined && outputs[`${loop}.queued`] === 1;
        return h("div", {
          style: () => ({ padding: "sm", radius: "md", background: plays() ? "green/40" : waits() ? "fill-strong" : "fill" }),
          onClick: () => launch(loop),
        }, loop ?? "Stop");
      }),
    );
  },
});
```

Its timelines, `state/loops/low.json` and `state/loops/high.json`, loop a bar each: `{"tool": "timeline", "state": {"length": 3840, "loop": true, "plays": false, "notes": [...]}}`.
