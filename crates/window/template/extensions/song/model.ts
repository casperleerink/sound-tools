// What the song is made of and what its code keeps: its fields and children, the one memory
// the view and the keys share, and what each part of it is called and coloured.
import { boolean, choice, list, text, type Color, type Context, type Note, type Part, type ValuesOf } from "../sdk";
import { bus } from "../bus";
import { tape } from "../tape";
import { isMover, track } from "../track";
import type { Clipboard } from "./clips";
import type { NoteLane } from "./notes";
import { SNAP, BEAT } from "./time";

export const parts = { master: "fader", limit: "limiter", quiet: "lane", "audio-*": tape, "ret-*": bus, "*": track } as const;
export const fields = {
  order: list(text(), { max: 128 }),
  solo: list(text(), { max: 64 }),
  sidechains: list({ effect: text(), from: text(), tap: choice({ options: ["pre_fx", "post_fx", "post_fader"], default: "post_fader" }) }, { max: 64 }),
  limit_off: boolean({ default: false }),
};

/** What shows under the timeline, one at a time: a track's or a return's panel, the master's, a clip's notes or the mixer. */
export type Below = { kind: "track"; name: string } | { kind: "master" } | { kind: "editor"; clip: string } | { kind: "mixer" } | null;
/** What starts at a bar of the ruler: a section, a time signature change or a tempo change, at `tick`. */
export type Mark = { kind: "section" | "signature" | "tempo"; bar: number; tick: number };

/** What the song's code keeps and nothing saves: the view draws it, the keys and the edits change it. */
export type Memory = {
  /** The chosen clips and regions, `row/name`: one selection, which the panel and the editor follow. */
  selected: string[];
  below: Below;
  /** The track the keyboard plays and records on, and the one a key for "the track" means beside the mixer. */
  track: string | null;
  /** What the window was last told the composer works on, as `publish` says it. */
  published: string | null;
  /** The tracks whose recorder has its input open. */
  armed: string[];
  /** The tick a recording started at, while it records. */
  recording: number | null;
  clipboard: Clipboard | null;
  /** The track or return whose name is typed, the clip whose name is typed, and the section. */
  renaming: string | null;
  renamingClip: string | null;
  renamingSection: { bar: number; tick: number } | null;
  /** The chosen mark of the ruler, points of a lane and effect of a panel: delete and the keys act on it. */
  mark: Mark | null;
  /** The lane open for editing, whose points take the pointer, and its chosen points. */
  point: { track: string; lane: string; ticks: number[] } | null;
  effect: { track: string; name: string } | null;
  /** The tracks whose automation lanes show under them. */
  lanes: string[];
  tempoLane: boolean;
  returnsOpen: boolean;
  /** Where the ruler's menu opened last, in ticks, and the mark it opened on. */
  rulerMenu: { x: number; mark: Mark | null };
  /** The range the ruler shows to repeat, none until the composer made one, and whether it repeats. */
  repeatRange: { from: number; to: number } | null;
  repeating: boolean;
  /** The grid of drags, nudges and new clips in ticks: 0 is off. */
  snap: number;
  /** How tall the note editor is, in points, and whether it takes all the window but one track. */
  editor: number;
  editorLarge: boolean;
  /** Whether the page showed the panel of its first track, as it does once, when it opens. */
  opened: boolean;
  /** Whether the card of a region shows where in its file it starts and ends. */
  regionMore: boolean;
  /** The chosen notes of the open clip, by `noteKey`: by what they are, so an undo keeps them right. */
  notes: string[];
  /** Where the editor of each clip opened, `row/clip`: the row of the roll at its top and its points per tick, which its view keeps. */
  rolls: Record<string, { top: number; across: number }>;
  /** The lane under the notes, how long a new note is, the copied notes from 0, and what Quantize and Humanize use. */
  lane: NoteLane;
  length: number;
  copied: Note[] | null;
  strength: number;
  swing: number;
  humanized: number;
};

/** How tall the note editor opens, in points. */
export const EDITOR = 352;

export const memory = (): Memory => ({
  selected: [], below: null, track: null, published: null, armed: [], recording: null, clipboard: null, renaming: null, renamingClip: null, renamingSection: null,
  mark: null, point: null, effect: null, lanes: [], tempoLane: false, returnsOpen: false, rulerMenu: { x: 0, mark: null }, repeatRange: null, repeating: false, snap: SNAP, editor: EDITOR, editorLarge: false, opened: false, regionMore: false,
  notes: [], rolls: {}, lane: "velocity", length: BEAT / 2, copied: null, strength: 1, swing: 0, humanized: 0,
});

/** What all code of the song gets: its view, its actions and the edits. */
export type Song = Context<ValuesOf<typeof fields>, {}, Memory, typeof parts>;
/** The song's records as an `update` changes them. */
export type Draft = Part<ValuesOf<typeof fields>, typeof parts>;
/** A child of the song: a track, an audio track, a return, the master or its limiter. */
export type Child = Song["children"][number];

type Rows = { tool: typeof track | typeof tape };
type Strips = { tool: typeof track | typeof tape | typeof bus };
export const isRow = <C extends { tool: unknown }>(child: C): child is Extract<C, Rows> => child.tool === "track" || child.tool === "tape";
export const isStrip = <C extends { tool: unknown }>(child: C): child is Extract<C, Strips> => isRow(child) || child.tool === "bus";

/** `children` in the order `order` names them, then the others by name. */
export function ordered<T extends { name: string }>(children: readonly T[], order: readonly string[]): T[] {
  const rank = (name: string) => (order.includes(name) ? order.indexOf(name) : order.length);
  return [...children].sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1));
}
/** A row of the timeline by its name and its tool. */
export type RowKind = { name: string; tool: "track" | "tape" };
/** The rows of the timeline, tracks and audio tracks, in `order`. */
export const rowsOf = (children: readonly { name: string; tool: unknown }[], order: readonly string[]): RowKind[] =>
  ordered(children.flatMap(({ name, tool }) => (tool === "track" || tool === "tape" ? [{ name, tool }] : [])), order);
/** The tool of what a row holds: a track's clips, an audio track's regions. */
export const itemsOf = (row: { tool: unknown }): "timeline" | "region" => (row.tool === "tape" ? "region" : "timeline");

/** The colours a track can have, in the order new tracks take them; the theme names each. */
export const PALETTE = [
  { name: "Blue", colour: "blue" }, { name: "Peach", colour: "peach" }, { name: "Mauve", colour: "mauve" }, { name: "Teal", colour: "teal" },
  { name: "Green", colour: "green" }, { name: "Yellow", colour: "yellow" }, { name: "Pink", colour: "pink" }, { name: "Sky", colour: "sky" },
  { name: "Sapphire", colour: "sapphire" }, { name: "Red", colour: "red" }, { name: "Maroon", colour: "maroon" }, { name: "Lavender", colour: "lavender" },
  { name: "Rosewater", colour: "rosewater" }, { name: "Flamingo", colour: "flamingo" },
] as const satisfies readonly { name: string; colour: Color }[];
/** The colour of a track or return: one of the palette, or `#rrggbb`; blue for any other text its field holds. */
export const colourOf = (each: { state: { colour?: string } }): Color => {
  const colour = each.state.colour;
  const named = PALETTE.find((one) => one.colour === colour)?.colour;
  return named ?? (colour?.startsWith("#") ? (colour as Color) : "blue");
};

/** A name or a field in plain words, its unit left out: `osc_1.position` is `Osc 1 position`, `cutoff_hz` is `Cutoff`. */
export const words = (text: string) =>
  text.replace(/_(hz|db|ms|seconds|cents|octaves|semitones)$/, "").replace(/[._-]+/g, " ").trim().replace(/^./, (first) => first.toUpperCase());
const KINDS = { audio: "Audio", ret: "Return" } as const;
/** A name with no title, in words: `t00` is Track 1, `audio-voice` Voice, `audio-1` Audio 1, `ret-a` Return A. */
const wordsOf = (name: string) => {
  const numbered = /^t(\d+)$/.exec(name);
  if (numbered) return `Track ${Number(numbered[1]) + 1}`;
  const [, kind, rest = name] = /^(audio|ret)-(.*)$/.exec(name) ?? [];
  if (kind && /^(\d+|[a-z])$/i.test(rest)) return `${KINDS[kind as keyof typeof KINDS]} ${rest.toUpperCase()}`;
  return words(rest);
};
/** What the song calls a child: its title, else its name in words. */
export const titleOf = (each: Child) => (isStrip(each) && each.state.title) || wordsOf(each.name);

/** A child's name from any text: lowercase letters, digits and `-`. */
export const nameFrom = (text: string) => text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "x";

/** The fader of a track, an audio track or a return. */
export const mixOf = (each: Child) => {
  const mix = isStrip(each) ? each.children.find((child) => child.name === "mix") : undefined;
  return mix?.tool === "fader" ? mix : undefined;
};
export const muted = (each: Child) => mixOf(each)?.state.mute === true;
/** The lanes of a track or an audio track whose child moves its number. */
export const lanesOf = (each: Child) =>
  isRow(each) ? each.state.automation.filter(({ lane }) => each.children.some((child) => child.name === lane && isMover(child.tool))) : [];
/** The sends of a track or an audio track, `send-<return>`. */
export const sendsOf = (each: Child) => (isRow(each) ? each.children.filter((child) => child.name.startsWith("send-")).map((child) => child.name) : []);

/** The clip or the region `key` names, `row/name`. */
export function itemAt(children: readonly Child[], key: string) {
  const [row, name] = key.split("/");
  const owner = children.find((child) => child.name === row);
  const item = owner && isRow(owner) ? owner.children.find((child) => child.name === name) : undefined;
  return item?.tool === "timeline" || item?.tool === "region" ? item : undefined;
}
/** The row a key `row/name` is on. */
export const rowOfKey = (key: string) => key.slice(0, key.indexOf("/"));
