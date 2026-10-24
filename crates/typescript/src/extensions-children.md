# A tool that plays other tools

Read `agent-docs/extensions.md` first. The types of children and connections are in `extensions/sdk/tools.ts`, the records of the built-in tools in `extensions/sdk/records.ts`.

A tool with a `behaviour` is a parent. Its record is `state/<name>/instance.json`, and every record next to it, `state/<name>/<child>.json`, is a child of any tool: a built-in device such as `instrument.synth`, `delay` or a `plugin` (enable its extension in `project.json`: `plugin-host` for a plugin), a tool of this project, or one with only `state`: data, such as a take a recorder saves. The behaviour is a pure function of the record and the children that says how they connect. A parent has one voice.

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

- It gets `state` and `children`, each `{ name, tool, state }`, every field at its default where a record leaves it out. Give it the return type `Declaration`: without it TypeScript lets a misspelled key through and blames the whole behaviour.
- A parent whose children are fixed names them and their tools in `children`, as `loops` below does: a `const` with `as const`, and the return type `Declaration<typeof children>`. TypeScript then checks each connection, `play`, `send` and child `state`. For a tool of this project, name what `tool` returns: `export const drone = tool({ ... })` in its file, then `import { drone } from "./drone"` and `{ lead: drone, fx: "utility" }`; what its `output` sends is not checked. `takes("notes")` in place of a tool is any tool that takes notes, as `offers("notes")` lists them; `addOffer` puts the one picked in its place. A child it does not name is a problem of the parent's record; `children: {}` allows none.
- A child may be a parent too, at `state/<name>/<child>/instance.json`. Each child's `children` holds its own, at any depth, and `update` edits, adds (with their `children`) and takes out records at any depth; one taken out takes all it holds. A name in `children` that ends in `*` stands for every other name that starts as it does: `{ "ret-*": bus, "*": track }`.
- A child's `name` is the id of its record and stays: keep a title in a field. Without typed `children`, cast a child's state: `children.find((child) => child.name === "bass")!.state as TimelineState`.
- The behaviour runs in Bun when the record or a child changes; its answer plays a moment later, the last one until then. `--render` and `--analyze` wait for it.
- A missing child or port, or a behaviour that throws, is a problem of the parent's record. The rest plays.

## Connections

- A sound port of a child is `audio` when left out, else one its tool has: `{ child: "duck", port: "sidechain" }`, `{ child: "split", port: "low" }`. `{ child: "bass/mix" }` is a port of a child of a child, so a parent wires across its children, such as a send of one track into another (write such a path `as const`).
- Leave out `child` for a port of the parent itself: from `{ port: "audio" }` comes what reaches its audio input, from `{ notes: "notes" }` what reaches its `notes`; to `{ port: "audio" }` goes what it plays, through its own `sound` as `input`; to `{ number }`, `{ bang }` or `{ notes }` an output of that name, which its own parent connects.
- Events connect by the key of their kind at both ends, to the sample:
  - `{ from: { child: "lfo", number: "cutoff" }, to: { child: "filter", number: "cutoff_hz" } }`: the number `lfo` sends with `output` turns `cutoff_hz` of `filter`, in its units: a number field of a tool of this project, or any number a lane can automate. Nothing is saved; without the connection the field glides back. To a number input, it moves that input.
  - `{ from: { child: "ears", bang: "onset" }, to: { child: "drop", bang: "hit" } }` sends a bang input.
  - `{ from: { child: "ears", notes: "hits" }, to: { child: "synth", notes: "notes" } }` plays an instrument. `{ from: { notes: "notes" }, to: { child: "piano", notes: "notes" } }` plays it from the keyboard.
- A parent plays its children by name: `play(pitch, { child: "synth" })` and `send({ child: "synth", notes: "notes" }, event)` reach its `notes`; `send({ child: "lfo", number: "rate" }, 2)` and `send({ child: "drop", bang: "hit" })` its inputs.

## Blocks

The built-in children a parent plays with, each with its doc:

- `timeline` (`agent-docs/timeline.md`) plays notes in time out of its output `notes`: once at their place in the song, or round after round with `"loop": true`. `send({ child: "bass", number: "launch" }, 1)` starts it at the next bar, `0` stops it there. A parent's view reads the outputs of each child as `<child>.<name>`: a looping timeline's `playing` is 1 while it plays.
- `lane` (`agent-docs/lane.md`) moves a number of another child over the song, as automation.
- `region` (`agent-docs/region.md`) plays an audio file at its place in the song.
- `recorder` (`agent-docs/recorder.md`) records notes and the audio input, and hands each take to its parent's `onTake`.
- `fader` (`agent-docs/fader.md`) and `meter` (`agent-docs/meter.md`) turn a sound up or down and show its level.

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
    return h("div", { style: { direction: "row", gap: 8, padding: 16 } },
      [...loops, undefined].map((loop) => {
        const plays = () => loop !== undefined && outputs[`${loop}.playing`] === 1;
        const waits = () => loop !== undefined && outputs[`${loop}.queued`] === 1;
        return h("div", {
          style: () => ({ padding: 8, radius: 6, background: plays() ? "#7c3aed" : waits() ? "#4c1d95" : "#2a2f3a" }),
          onClick: () => launch(loop),
        }, loop ?? "Stop");
      }),
    );
  },
});
```

Its timelines, `state/loops/low.json` and `state/loops/high.json`, loop a bar each, such as `{"tool": "timeline", "state": {"length": 3840, "loop": true, "plays": false, "notes": [...]}}`: `"plays": false` waits for a launch.
