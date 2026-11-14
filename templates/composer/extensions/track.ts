import { chain } from "./chain";
import { takes, text, tool, type Connection, type Declaration } from "./sdk";

const parts = { synth: takes("notes"), chain, mix: "fader", "send-*": "fader", "sec-*": "timeline" } as const;

/** `wanted`, or `wanted-2` and on when that is taken. */
export function freeName(names: readonly string[], wanted: string): string {
  let name = wanted;
  for (let number = 2; names.includes(name); number++) name = `${wanted}-${number}`;
  return name;
}

export const track = tool({
  name: "track",
  title: "Instrument",
  when: "You change what an instrument of the piece is made of or how it is wired",
  doc: "An instrument of a `piece`: its parts, one `timeline` child per section, named as the section's `id` (`sec-verse`), and what reaches its `notes`, play on its `synth`, any instrument, through the effects of its `chain`, into its fader `mix`, whose `gain_db`, `pan` and `mute` are its volume, pan and mute, and whose level its meter shows. The notes of a part are in ticks from the start of its section; its `start` and `length` are the piece's to set. Each fader `send-<return>` is how loud it sends into that return of the piece, which wires it. `colour` is how the piece draws its notes, `title` what it calls the instrument.",
  state: { colour: text({ default: "blue" }), title: text() },
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => ({
    connections: [
      ...children.filter((child) => child.tool === "timeline").map((part) => ({ from: { child: part.name, notes: "notes" }, to: { child: "synth", notes: "notes" } }) as Connection<typeof parts>),
      // What the instrument hears, such as the keyboard while it is selected, plays its synth too.
      { from: { notes: "notes" }, to: { child: "synth", notes: "notes" } },
      { from: { child: "synth" }, to: { child: "chain" } },
      { from: { child: "chain" }, to: { child: "mix" } },
      { from: { child: "mix" }, to: { port: "audio" } },
    ],
  }),
});
