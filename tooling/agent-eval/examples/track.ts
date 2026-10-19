import { chain, chainPlays } from "./chain";
import { text, tool, type Connection, type Declaration } from "./sdk";

const parts = { synth: "instrument.synth", fx: "filter", chain, mix: "fader", cutoff: "lane", rec: "recorder", "send-*": "fader", "*": "timeline" } as const;

/** The child of a track whose sound comes before its fader: the end of its chain when that plays, else its filter. */
export function preFader(track: { readonly children: readonly { name: string; tool: string; state: unknown; children: readonly { name: string }[] }[] }): "chain" | "fx" {
  const effects = track.children.find((child) => child.name === "chain");
  return effects?.tool === "chain" && chainPlays(effects as Parameters<typeof chainPlays>[0]) ? "chain" : "fx";
}

export const track = tool({
  name: "track",
  title: "Track",
  when: "You change what a track of the song is made of or how it is wired",
  doc: "A track of a `song`: its `timeline` children, and what reaches its `notes`, play on its `synth`, through its `fx` and the effects of its `chain`, into its fader `mix`, whose `gain_db`, `pan` and `mute` are the track's volume, pan and mute, and whose level its meter shows. Its lane `cutoff` moves the cutoff of `fx` over the song. Its recorder `rec` records what reaches its `notes` into new clips while the song records. Each fader `send-<return>` is how loud it sends into that return of the song, which wires it. `colour` is how the song draws its clips, `title` what it calls the track.",
  state: { colour: text({ default: "#a9b1ff" }), title: text() },
  children: parts,
  behaviour: ({ children }): Declaration<typeof parts> => {
    const clips: Connection<typeof parts>[] = children
      .filter((child) => child.tool === "timeline")
      .map((clip) => ({ from: { child: clip.name, notes: "notes" }, to: { child: "synth", notes: "notes" } }));
    const through: Connection<typeof parts>[] =
      preFader({ children }) === "chain"
        ? [{ from: { child: "fx" }, to: { child: "chain" } }, { from: { child: "chain" }, to: { child: "mix" } }]
        : [{ from: { child: "fx" }, to: { child: "mix" } }];
    return {
      connections: [
        ...clips,
        { from: { child: "cutoff", number: "value" }, to: { child: "fx", number: "cutoff_hz" } },
        { from: { notes: "notes" }, to: { child: "rec", notes: "notes" } },
        // What the track hears, such as the keyboard while it is selected, plays its synth too.
        { from: { notes: "notes" }, to: { child: "synth", notes: "notes" } },
        { from: { child: "synth" }, to: { child: "fx" } },
        ...through,
        { from: { child: "mix" }, to: { port: "audio" } },
      ],
    };
  },
});
