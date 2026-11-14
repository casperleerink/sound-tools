import { boolean, choice, h, list, number, Page, text, tool, Transport, type Connection, type Context, type Declaration, type Inputs, type ValuesOf } from "./sdk";
import { bus } from "./bus";
import type { Chosen, Copied } from "./piece-edits";
import { track } from "./track";

// A piece: sections of bars, instruments that play a part in each, a mixer strip per instrument,
// the devices of the chosen instrument, and a note editor for one part.
export const parts = { master: "fader", limit: "limiter", quiet: "lane", "ret-*": bus, "*": track } as const;
const bars = { bars: number({ min: 1, max: 999, default: 1, integer: true }), signature: text({ default: "4/4" }), bpm: number({ min: 10, max: 1000, default: 120 }) };
const fields = {
  sections: list({ id: text(), name: text(), bars: list(bars, { max: 512 }) }, { max: 256 }),
  order: list(text(), { max: 128 }),
  solo: list(text(), { max: 64 }),
  sidechains: list({ effect: text(), from: text(), tap: choice({ options: ["pre_fx", "post_fx", "post_fader"], default: "post_fader" }) }, { max: 64 }),
  limit_off: boolean({ default: false }),
};
/** What the piece's code gets: its view, the parts of its page and its actions. */
export type Piece = Context<ValuesOf<typeof fields>, Inputs, Memory, typeof parts>;

type Memory = {
  /** The section that shows, by its id. */
  open: string | null;
  /** The chosen bars of the open section. */
  chosen: Chosen | null;
  copied: Copied | null;
};

export const piece = tool({
  name: "piece",
  title: "Piece",
  when: "You change the piece's page: its sections and bars, its instruments, mixer strips or note editor",
  doc: "A piece in sections.",
  state: fields,
  children: parts,
  memory: (): Memory => ({ open: null, chosen: null, copied: null }),
  behaviour: ({ state, children }): Declaration<typeof parts> => {
    const tracks = children.filter((child) => child.tool === "track");
    const returns = children.filter((child) => child.tool === "bus");
    const soloed = state.solo.filter((name) => tracks.some((each) => each.name === name));
    const connections: Connection<typeof parts>[] = [
      ...[...tracks, ...returns].map((each) => ({ from: { child: each.name }, to: { child: "master" } })),
      ...(state.limit_off
        ? [{ from: { child: "master" }, to: { port: "audio" } } as const]
        : [{ from: { child: "master" }, to: { child: "limit" } } as const, { from: { child: "limit" }, to: { port: "audio" } } as const]),
    ];
    // What the piece hears, the keyboard while nothing is chosen, plays its first instrument.
    const rank = (name: string) => (state.order.includes(name) ? state.order.indexOf(name) : state.order.length);
    const first = [...tracks].sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1))[0];
    if (first) connections.push({ from: { notes: "notes" }, to: { child: first.name, notes: "notes" } });
    // A solo mutes every other instrument's fader, gliding; a return always plays.
    const soloedOut = (name: string) => soloed.length > 0 && !soloed.includes(name);
    for (const each of tracks) {
      if (soloedOut(each.name)) connections.push({ from: { child: "quiet", number: "value" }, to: { child: `${each.name}/mix` as const, number: "mute" } });
    }
    // A send of an instrument plays into its return: after its fader, or before it for a return that says so. A muted or soloed-out one sends nothing either way.
    for (const each of tracks) {
      const mix = each.children.find((child) => child.name === "mix");
      const silent = soloedOut(each.name) || (mix?.tool === "fader" && mix.state.mute === true);
      for (const back of returns) {
        if (back.tool !== "bus" || !each.children.some((child) => child.name === `send-${back.name}`)) continue;
        const from = back.state.pre_fader ? "chain" : "mix";
        const send = `${each.name}/send-${back.name}` as const;
        connections.push({ from: { child: `${each.name}/${from}` as const }, to: { child: send } });
        connections.push({ from: { child: send }, to: { child: back.name } });
        if (back.state.pre_fader && silent) connections.push({ from: { child: "quiet", number: "value" }, to: { child: send, number: "mute" } });
      }
    }
    // A sidechain keys an effect with an instrument: what plays into its effects, what comes out of them, or what comes out of its fader.
    for (const { effect, from, tap } of state.sidechains) {
      if (!tracks.some((each) => each.name === from)) continue;
      const key = tap === "pre_fx" ? "synth" : tap === "post_fx" ? "chain" : "mix";
      connections.push({ from: { child: `${from}/${key}` as `${string}/${string}` }, to: { child: effect as `${string}/${string}`, port: "sidechain" } });
    }
    return { connections };
  },
  view: () => <Page flush header={<Transport computerKeys tempo={false} />} />,
});
