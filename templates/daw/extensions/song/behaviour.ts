// What the song plays: every track and return into the master, the solos, the sends and the
// sidechains. Both pages of a song, the tracks and the score, play the same.
import type { Connection, Declaration, Parent } from "../sdk";
import type { fields, parts } from "./model";
import { notesTrackOf } from "./tracks";

export const behaviour = ({ state, children }: Parent<typeof fields, typeof parts>): Declaration<typeof parts> => {
  const tracks = children.filter((child) => child.tool === "track" || child.tool === "tape");
  const returns = children.filter((child) => child.tool === "bus");
  const soloed = state.solo.filter((name) => tracks.some((each) => each.name === name));
  const connections: Connection<typeof parts>[] = [
    ...[...tracks, ...returns].map((each) => ({ from: { child: each.name }, to: { child: "master" } })),
    ...(state.limit_off
      ? [{ from: { child: "master" }, to: { port: "audio" } } as const]
      : [{ from: { child: "master" }, to: { child: "limit" } } as const, { from: { child: "limit" }, to: { port: "audio" } } as const]),
  ];
  // What the song hears, the keyboard while nothing is chosen, plays its first track.
  const first = notesTrackOf(children, state.order, null);
  if (first) connections.push({ from: { notes: "notes" }, to: { child: first, notes: "notes" } });
  // A solo mutes every other track's fader, gliding, so a lane that moves its volume plays on under it; a return always plays.
  const soloedOut = (name: string) => soloed.length > 0 && !soloed.includes(name);
  for (const each of tracks) {
    if (soloedOut(each.name)) connections.push({ from: { child: "quiet", number: "value" }, to: { child: `${each.name}/mix` as const, number: "mute" } });
  }
  // A send of a track plays into its return: after the track's fader, or before it for a return that says so. A muted or soloed-out track sends nothing either way.
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
  // A sidechain keys an effect with a track: what plays into its effects, what comes out of them, or what comes out of its fader.
  for (const { effect, from, tap } of state.sidechains) {
    const source = tracks.find((each) => each.name === from);
    if (!source) continue;
    const before = source.children.filter((child) => child.tool === "region" || child.name === "synth").map((child) => child.name);
    for (const key of tap === "pre_fx" ? before : [tap === "post_fx" ? "chain" : "mix"]) {
      connections.push({ from: { child: `${from}/${key}` as `${string}/${string}` }, to: { child: effect as `${string}/${string}`, port: "sidechain" } });
    }
  }
  return { connections };
};
