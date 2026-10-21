import { chain } from "./chain";
import { list, takes, text, tool, type ChildTools, type Connection, type Declaration } from "./sdk";

const parts = { synth: takes("notes"), chain, mix: "fader", rec: "recorder", "send-*": "fader", "lane-*": "lane", "*": "timeline" } as const;

/** `"mix.gain_db"` or `"chain/verb.mix"`: the path of the child and the name of its number. */
export function splitTarget(target: string): [child: string, number: string] {
  const dot = target.lastIndexOf(".");
  return [target.slice(0, dot), target.slice(dot + 1)];
}

/** What the lanes of a track move: each lane child `lane` into its number `target`. */
export const automation = list({ lane: text(), target: text() }, { max: 32 });

/** A connection from each lane of `lanes` that is a child into the number it moves. */
export function laneConnections<K extends ChildTools>(lanes: readonly { lane: string; target: string }[], children: readonly { name: string; tool: unknown }[]): Connection<K>[] {
  return lanes
    .filter(({ lane }) => children.some((child) => child.name === lane && child.tool === "lane"))
    .map(({ lane, target }) => {
      // The record names the number: one the child does not have is a problem of the track.
      const [child, number] = splitTarget(target);
      return { from: { child: lane, number: "value" }, to: { child, number } } as Connection<K>;
    });
}

/** `wanted`, or `wanted-2` and on when that is taken. */
export function freeName(names: readonly string[], wanted: string): string {
  let name = wanted;
  for (let number = 2; names.includes(name); number++) name = `${wanted}-${number}`;
  return name;
}

export const track = tool({
  name: "track",
  title: "Track",
  when: "You change what a track of the song is made of or how it is wired",
  doc: "A track of a `song`: its `timeline` children, and what reaches its `notes`, play on its `synth`, any instrument, through the effects of its `chain`, into its fader `mix`, whose `gain_db`, `pan` and `mute` are the track's volume, pan and mute, and whose level its meter shows. Each of `automation` moves a number of a child over the song: the lane child `lane` into `target`, such as `mix.gain_db` or `chain/filter.cutoff_hz`. Its recorder `rec` records what reaches its `notes` into new clips while the song records. Each fader `send-<return>` is how loud it sends into that return of the song, which wires it. `colour` is how the song draws its clips, `title` what it calls the track.",
  state: { colour: text({ default: "blue" }), title: text(), automation },
  children: parts,
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const clips: Connection<typeof parts>[] = children
      .filter((child) => child.tool === "timeline")
      .map((clip) => ({ from: { child: clip.name, notes: "notes" }, to: { child: "synth", notes: "notes" } }));
    return {
      connections: [
        ...clips,
        ...laneConnections<typeof parts>(state.automation, children),
        { from: { notes: "notes" }, to: { child: "rec", notes: "notes" } },
        // What the track hears, such as the keyboard while it is selected, plays its synth too.
        { from: { notes: "notes" }, to: { child: "synth", notes: "notes" } },
        { from: { child: "synth" }, to: { child: "chain" } },
        { from: { child: "chain" }, to: { child: "mix" } },
        { from: { child: "mix" }, to: { port: "audio" } },
      ],
    };
  },
  onTake: ({ update }, take) => {
    if (take.tool !== "timeline") throw new Error(`the take ${take.name} is a region, and a track keeps notes: give its recorder rec no channels, and record sound on an audio track`);
    update("Record", ({ children }) => {
      children.push({ name: freeName(children.map((child) => child.name), take.name), tool: "timeline", state: take.state });
    });
  },
});
