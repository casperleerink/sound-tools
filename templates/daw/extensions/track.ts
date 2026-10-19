import { chain } from "./chain";
import { freeName } from "./tape";
import { list, takes, text, tool, type Connection, type Declaration } from "./sdk";

const parts = { synth: takes("notes"), fx: "filter", chain, mix: "fader", rec: "recorder", "send-*": "fader", "lane-*": "lane", "*": "timeline" } as const;

/** `"fx.cutoff_hz"` or `"chain/verb.mix"`: the path of the child and the name of its number. */
export function splitTarget(target: string): [child: string, number: string] {
  const dot = target.lastIndexOf(".");
  return [target.slice(0, dot), target.slice(dot + 1)];
}

export const track = tool({
  name: "track",
  title: "Track",
  when: "You change what a track of the song is made of or how it is wired",
  doc: "A track of a `song`: its `timeline` children, and what reaches its `notes`, play on its `synth`, any instrument, through its `fx` and the effects of its `chain`, into its fader `mix`, whose `gain_db`, `pan` and `mute` are the track's volume, pan and mute, and whose level its meter shows. Each of `automation` moves a number of a child over the song: the lane child `lane` into `target`, such as `fx.cutoff_hz` or `chain/verb.mix`. Its recorder `rec` records what reaches its `notes` into new clips while the song records. Each fader `send-<return>` is how loud it sends into that return of the song, which wires it. `colour` is how the song draws its clips, `title` what it calls the track.",
  state: { colour: text({ default: "#a9b1ff" }), title: text(), automation: list({ lane: text(), target: text() }, { max: 32 }) },
  children: parts,
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const clips: Connection<typeof parts>[] = children
      .filter((child) => child.tool === "timeline")
      .map((clip) => ({ from: { child: clip.name, notes: "notes" }, to: { child: "synth", notes: "notes" } }));
    const through: Connection<typeof parts>[] = [{ from: { child: "fx" }, to: { child: "chain" } }, { from: { child: "chain" }, to: { child: "mix" } }];
    const lanes: Connection<typeof parts>[] = state.automation
      .filter(({ lane }) => children.some((child) => child.name === lane && child.tool === "lane"))
      .map(({ lane, target }) => {
        // The record names the number: one the child does not have is a problem of the track.
        const [child, number] = splitTarget(target);
        return { from: { child: lane, number: "value" }, to: { child, number } } as Connection<typeof parts>;
      });
    return {
      connections: [
        ...clips,
        ...lanes,
        { from: { notes: "notes" }, to: { child: "rec", notes: "notes" } },
        // What the track hears, such as the keyboard while it is selected, plays its synth too.
        { from: { notes: "notes" }, to: { child: "synth", notes: "notes" } },
        { from: { child: "synth" }, to: { child: "fx" } },
        ...through,
        { from: { child: "mix" }, to: { port: "audio" } },
      ],
    };
  },
  onTake: ({ update }, take) => {
    if (take.tool !== "timeline") return;
    update("Record", ({ children }) => {
      children.push({ name: freeName(children.map((child) => child.name), take.name), tool: "timeline", state: take.state });
    });
  },
});
