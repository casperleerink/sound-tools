import { text, tool, type Connection, type Declaration } from "./sdk";

const parts = { synth: "instrument.synth", fx: "filter", mix: "utility", "*": "timeline" } as const;

export const track = tool({
  name: "track",
  title: "Track",
  when: "You change what a track of the song is made of or how it is wired",
  doc: "A track of a `song`: its `timeline` children play on its `synth`, through its `fx` and its `mix`, whose `gain_db`, `pan` and `mute` are the track's volume, pan and mute. Its meter `level` hears the mix. `colour` is how the song draws its clips.",
  state: { colour: text({ default: "#a9b1ff" }) },
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => {
    const clips: Connection<typeof parts>[] = children
      .filter((child) => child.tool === "timeline")
      .map((clip) => ({ from: { child: clip.name, notes: "notes" }, to: { child: "synth", notes: "notes" } }));
    return {
      connections: [
        ...clips,
        { from: { child: "synth" }, to: { child: "fx" } },
        { from: { child: "fx" }, to: { child: "mix" } },
        { from: { child: "mix" }, to: { port: "audio" } },
        { from: { child: "mix" }, to: { meter: "level" } },
      ],
    };
  },
});
