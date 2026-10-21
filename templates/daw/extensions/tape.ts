import { chain } from "./chain";
import { text, tool, type Connection, type Declaration } from "./sdk";
import { automation, freeName, laneConnections } from "./track";

const parts = { rec: "recorder", chain, mix: "fader", "send-*": "fader", "lane-*": "lane", "*": "region" } as const;

export const tape = tool({
  name: "tape",
  title: "Tape",
  when: "You change what an audio track of the song is made of or how it is wired",
  doc: "An audio track of a `song`: its `region` children play through the effects of its `chain` into its fader `mix`, whose `gain_db`, `pan` and `mute` are the track's, and whose level its meter shows. Each of `automation` moves a number of a child over the song, as a track's does. Each fader `send-<return>` is how loud it sends into that return of the song, which wires it. Its recorder `rec` records the audio input into new regions next to it while the song records. `colour` is how the song draws its regions, `title` what it calls the track.",
  state: { colour: text({ default: "teal" }), title: text(), automation },
  children: parts,
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const regions: Connection<typeof parts>[] = children
      .filter((child) => child.tool === "region")
      .map((region) => ({ from: { child: region.name }, to: { child: "chain" } }));
    return {
      connections: [...regions, ...laneConnections<typeof parts>(state.automation, children), { from: { child: "chain" }, to: { child: "mix" } }, { from: { child: "mix" }, to: { port: "audio" } }],
    };
  },
  onTake: ({ update }, take) => {
    if (take.tool !== "region") throw new Error(`the take ${take.name} is a timeline, and an audio track keeps sound: record notes on a track`);
    update("Record", ({ children }) => {
      children.push({ name: freeName(children.map((child) => child.name), take.name), tool: "region", state: take.state });
    });
  },
});

