// The devices of a track's or a return's panel: its instrument, which can be changed, and the
// effects of its chain, which can be added, turned off, removed, moved and keyed by a sidechain.
import { freeName } from "../track";
import { isStrip, nameFrom, type Song } from "./model";

/** Where on a track a sidechain takes its sound, and what its select says. */
export const TAPS = [["pre_fx", "Pre FX"], ["post_fx", "Post FX"], ["post_fader", "Post mixer"]] as const;
export type Tap = (typeof TAPS)[number][0];

/** The chain of the track or return `name`. */
export const chainOf = (children: Song["children"], name: string) => {
  const row = children.find((each) => each.name === name);
  const effects = row && isStrip(row) ? row.children.find((child) => child.name === "chain") : undefined;
  return effects?.tool === "chain" ? effects : undefined;
};

/** The chosen effect, while the panel of its track shows and its chain holds it: the one the keys move. */
export function chosenEffect({ memory, children }: Pick<Song, "memory" | "children">) {
  const chosen = memory.effect;
  const shown = chosen !== null && memory.below?.kind === "track" && memory.below.name === chosen.track;
  return shown && chainOf(children, chosen.track)?.state.order.includes(chosen.name) ? chosen : null;
}

/** What a view does to the devices of the track or return `name`: each change one undo step. */
export const effectEdits = (song: Song, name: string) => {
  const { update, memory } = song;
  const order = () => chainOf(song.children, name)?.state.order ?? [];
  /** Changes the chain as one undo step `label`. */
  const editChain = (label: string, change: (effects: { state: { order: string[]; off: string[] }; children: { name: string }[] }) => void) =>
    update(label, ({ children }) => {
      const row = children.find((child) => child.name === name);
      const effects = row?.tool === "track" || row?.tool === "tape" || row?.tool === "bus" ? row.children.find((child) => child.name === "chain") : undefined;
      if (effects?.tool === "chain") change(effects);
    });
  /** Takes the sidechain off the effect at `path`, in the step `label`: it goes, or another device takes its place. */
  const unkey = (path: string, label: string) => {
    if (song.state.sidechains.some((each) => each.effect === path)) update(label, (draft) => (draft.state.sidechains = draft.state.sidechains.filter((each) => each.effect !== path)));
  };
  const edits = {
    /** Chooses `effect`: enter on its card, its title's menu or adding it; the keys then move it. */
    choose: (effect: string) => (memory.effect = { track: name, name: effect }),
    /** An effect dragged onto another goes before it; onto the instrument, `before` its first; onto Add effect, `null`, last. */
    moveBefore(moved: string, before: string | null) {
      if (!order().includes(moved) || moved === before) return;
      editChain("Move an effect", (effects) => {
        const rest = effects.state.order.filter((each) => each !== moved);
        rest.splice(before === null ? rest.length : Math.max(rest.indexOf(before), 0), 0, moved);
        effects.state.order = rest;
      });
    },
    /** Move earlier, Move later: a step in the chain; at its end it stays. */
    step(effect: string, by: -1 | 1) {
      const at = order().indexOf(effect);
      const other = order()[at + by];
      if (at >= 0 && other !== undefined) edits.moveBefore(effect, by < 0 ? other : (order()[at + 2] ?? null));
    },
    /** Adds the offer `key` last in the chain, under a free name, chosen. */
    add(key: string) {
      const effect = freeName(chainOf(song.children, name)?.children.map((child) => child.name) ?? [], nameFrom(key) || "effect");
      song.addOffer(key, `${name}/chain/${effect}`, "Add an effect");
      editChain("Add an effect", (effects) => (effects.state.order = [...effects.state.order, effect]));
      edits.choose(effect);
    },
    /** Another device in place of the instrument, `"Change the instrument"`, or of `effect`, `"Change an effect"`: an effect's sidechain goes. */
    replace(key: string, effect?: string) {
      const path = effect === undefined ? `${name}/synth` : `${name}/chain/${effect}`;
      const label = effect === undefined ? "Change the instrument" : "Change an effect";
      if (effect !== undefined) edits.choose(effect);
      unkey(path, label);
      song.addOffer(key, path, label);
    },
    remove(effect: string) {
      unkey(`${name}/chain/${effect}`, "Remove an effect");
      editChain("Remove an effect", (effects) => {
        effects.state.order = effects.state.order.filter((each) => each !== effect);
        effects.state.off = effects.state.off.filter((each) => each !== effect);
        effects.children = effects.children.filter((child) => child.name !== effect);
      });
      if (memory.effect?.track === name && memory.effect.name === effect) memory.effect = null;
    },
    /** The power of an effect: off, the chain passes it by. */
    toggle(effect: string) {
      const off = chainOf(song.children, name)?.state.off.includes(effect) ?? false;
      editChain(off ? "Turn an effect on" : "Turn an effect off", (effects) => {
        effects.state.off = off ? effects.state.off.filter((each) => each !== effect) : [...effects.state.off, effect];
      });
    },
    /** Keys `effect` with what the track `from` plays, or nothing: its own track before its effects, another where it was, or after its effects. */
    sidechain(effect: string, from: string | null) {
      const path = `${name}/chain/${effect}`;
      const keyed = song.state.sidechains.find((each) => each.effect === path);
      update(from === null ? "Turn off sidechain" : keyed ? "Change sidechain" : "Turn on sidechain", (draft) => {
        const rest = draft.state.sidechains.filter((each) => each.effect !== path);
        const tap: Tap = from === name ? "pre_fx" : (keyed?.tap ?? "post_fx");
        draft.state.sidechains = from === null ? rest : [...rest, { effect: path, from, tap }];
      });
    },
    tap(effect: string, tap: Tap) {
      const path = `${name}/chain/${effect}`;
      update("Change sidechain tap", (draft) => (draft.state.sidechains = draft.state.sidechains.map((each) => (each.effect === path ? { ...each, tap } : each))));
    },
  };
  return edits;
};
