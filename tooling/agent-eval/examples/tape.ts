import { text, tool, type Connection, type Declaration } from "./sdk";

const parts = { rec: "recorder", mix: "fader", "*": "region" } as const;

export const tape = tool({
  name: "tape",
  title: "Tape",
  when: "You change what an audio track of the song is made of or how it is wired",
  doc: "An audio track of a `song`: its `region` children play through its fader `mix`, whose `gain_db`, `pan` and `mute` are the track's, and whose level its meter shows. Its recorder `rec` records the audio input into new regions next to it while the song records. `colour` is how the song draws its regions.",
  state: { colour: text({ default: "#7cc7ff" }) },
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => {
    const regions: Connection<typeof parts>[] = children
      .filter((child) => child.tool === "region")
      .map((region) => ({ from: { child: region.name }, to: { child: "mix" } }));
    return { connections: [...regions, { from: { child: "mix" }, to: { port: "audio" } }] };
  },
});
