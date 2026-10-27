import { memo, now, type Color, type FilledStateOfTool } from "./sdk";
import { lengthOf } from "./clip-editor";
import type { Song } from "./song";
import type { track } from "./track";
import { ticksAcross } from "./transport";

// What the parts of the song's page share: its sizes and colours, what a row of the timeline
// holds, and what the view works out once for all of them.
export const BEAT = 960;
// The song shows at least 64 bars of 4/4, and 32 more after what it holds.
const SHOWN = 64 * 4 * BEAT;
const AFTER = 32 * 4 * BEAT;
// Rows of 64 points, headers 176 wide, a ruler of two lines, and bar 1 eight points right of the
// headers. The tempo lane under the ruler, while it shows, is as tall as the ruler's old one line.
export const ROW = 64;
export const HEADER = 176;
export const RULER = 44;
export const TEMPO = 32;
export const MASTER_ROW = 40;
// The row under a track's lanes that adds one.
export const ADD_LANE = 40;
// Points per tick at zoom 1: a bar is 96 points.
export const ACROSS = 0.025;
// The ticks left of bar 1, eight points at zoom 1.
export const LEAD = 320;

export const COLOR = { back: "#121317", text: "#7c808c", clear: "#00000000" } as const;
export const PALETTE = [
  { name: "Blue", colour: "blue" }, { name: "Peach", colour: "peach" }, { name: "Mauve", colour: "mauve" }, { name: "Teal", colour: "teal" },
  { name: "Green", colour: "green" }, { name: "Yellow", colour: "yellow" }, { name: "Pink", colour: "pink" }, { name: "Sky", colour: "sky" },
  { name: "Sapphire", colour: "sapphire" }, { name: "Red", colour: "red" }, { name: "Maroon", colour: "maroon" }, { name: "Lavender", colour: "lavender" },
  { name: "Rosewater", colour: "rosewater" }, { name: "Flamingo", colour: "flamingo" },
] as const;

export const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);

/** A child's name from any text: lowercase letters, digits and `-`. */
export const nameFrom = (text: string) => text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "x";

// Bars: the time signatures are runs of bars, and the last run goes on.
export type Runs = readonly { readonly signature: string; readonly bars: number }[];
const ticksOfBar = (signature: string) => {
  const [count, unit] = signature.split("/").map(Number);
  return ((count ?? 4) * 4 * BEAT) / (unit ?? 4);
};
/** The tick each bar starts at, up to the first at or past `end`. */
export function barStarts(runs: Runs, end: number): number[] {
  const starts = [0];
  let at = 0;
  const all = runs.length > 0 ? runs : [{ signature: "4/4", bars: 1 }];
  all.forEach((run, index) => {
    const goesOn = index === all.length - 1;
    for (let bar = 0; goesOn ? at < end : bar < run.bars; bar++) {
      at += ticksOfBar(run.signature);
      starts.push(at);
    }
  });
  return starts;
}

/** A child of the song: a track, an audio track, a return, the master or its limiter. */
export type Child = Song["children"][number];
/** A track of the song as code reads it. */
type TrackRow = Extract<Child, { tool: typeof track }>;

/** A name or a field in plain words, its unit left out: `osc_1.position` is `Osc 1 position`, `cutoff_hz` is `Cutoff`. */
export const words = (text: string) =>
  text.replace(/_(hz|db|ms|seconds|cents|octaves|semitones)$/, "").replace(/[._-]+/g, " ").trim().replace(/^./, (first) => first.toUpperCase());
const KINDS = { audio: "Audio", ret: "Return" } as const;
/** A name with no title, in words: `t00` is Track 1, `audio-voice` Voice, `audio-1` Audio 1. */
const wordsOf = (name: string) => {
  const track = /^t(\d+)$/.exec(name);
  if (track) return `Track ${Number(track[1]) + 1}`;
  const [, kind, rest = name] = /^(audio|ret)-(.*)$/.exec(name) ?? [];
  if (kind && /^(\d+|[a-z])$/i.test(rest)) return `${KINDS[kind as keyof typeof KINDS]} ${rest.toUpperCase()}`;
  return words(rest);
};
export const titleOf = (each: Child) => ((each.tool === "track" || each.tool === "tape" || each.tool === "bus") && each.state.title) || wordsOf(each.name);
const mixOf = (each: Child) => (each.tool === "track" || each.tool === "tape" || each.tool === "bus" ? each.children.find((child) => child.name === "mix") : undefined);
export const muted = (each: Child) => {
  const mix = mixOf(each);
  return mix?.tool === "fader" && mix.state.mute === true;
};
// A text field holds any text: the colour is checked here, a colour of the theme or `#rrggbb`.
export const colourOf = (each: Child): Color => {
  const colour = each.tool === "track" || each.tool === "tape" || each.tool === "bus" ? each.state.colour : undefined;
  const named = PALETTE.find((each) => each.colour === colour)?.colour;
  return named ?? (colour?.startsWith("#") ? (colour as Color) : "blue");
};
export const lanesOf = (each: Child) =>
  each.tool === "track" || each.tool === "tape"
    ? each.state.automation.flatMap(({ lane, target }) =>
        each.children.some((child) => child.name === lane && child.tool === "lane") ? [{ lane, target }] : [],
      )
    : [];
export const clipsOf = (each: Child) => (each.tool === "track" ? each.children.filter((child) => child.tool === "timeline") : []);
export const regionsOf = (each: Child) => (each.tool === "tape" ? each.children.filter((child) => child.tool === "region") : []);
/** The channels of the audio input the recorder of an audio track records. */
export const inputOf = (each: Child | undefined): readonly number[] => {
  const rec = each?.tool === "tape" ? each.children.find((child) => child.name === "rec") : undefined;
  return rec?.tool === "recorder" ? [...rec.state.channels] : [];
};

/** A row of the timeline, a track or an audio track, by its name and its tool. */
export type RowKind = { name: string; tool: string };
/** The tool of what a row of `tool` holds: a track's clips, an audio track's regions. */
export const ITEMS: Record<string, "timeline" | "region"> = { track: "timeline", tape: "region" };

/** The rows of the timeline, tracks and audio tracks, in `order`. */
export function rowsOf(children: readonly RowKind[], order: readonly string[]): RowKind[] {
  const rank = (name: string) => (order.includes(name) ? order.indexOf(name) : order.length);
  return children
    .filter((child) => child.tool in ITEMS)
    .map(({ name, tool }) => ({ name, tool }))
    .sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1));
}

/** How long the clip or the region `key` names is on the timeline, in ticks: a region to its last warp point, else as long as its file plays, through the tempo changes. */
export function lengths({ children, outputs, clock }: Pick<Song, "children" | "outputs" | "clock">) {
  return (key: string) => {
    const [row, name] = key.split("/");
    const owner = children.find((child) => child.name === row);
    const item = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((child) => child.name === name) : undefined;
    if (item?.tool === "timeline") return lengthOf(item.state);
    if (item?.tool === "region") return item.state.warp?.at(-1)?.ticks ?? ticksAcross(clock.tempo_map.tempo_changes, item.state.start, outputs[`${key}.seconds`] ?? 0);
    return 0;
  };
}

/** The tool of the clip or the region `key` names, `row/name`. */
export function itemTool(children: Song["children"], key: string): "timeline" | "region" | undefined {
  const [row, name] = key.split("/");
  const owner = children.find((child) => child.name === row);
  const item = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.find((child) => child.name === name) : undefined;
  return item?.tool === "timeline" || item?.tool === "region" ? item.tool : undefined;
}

/** The song's view, with what its parts read, worked out once: its rows, its bars, the chosen clips. */
export function pageOf(song: Song) {
  const { state, children, memory, update, clock, select, outputs } = song;
  const rank = (name: string) => {
    const at = state.order.indexOf(name);
    return at < 0 ? state.order.length : at;
  };
  // The rows of the timeline: the tracks and the audio tracks, in `order`.
  const rows = memo(() =>
    children.filter((child) => child.tool === "track" || child.tool === "tape").slice().sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1)),
  );
  const tracks = () => rows().filter((each): each is TrackRow => each.tool === "track");
  const returns = () => children.filter((child) => child.tool === "bus");
  /** What has a panel: the tracks and the returns. */
  const strips = () => [...rows(), ...returns()];
  type Row = { each: Child; lane: { lane: string; target: string } | null; add?: true };
  // A row of the timeline for each track, and for a track that shows its lanes, one for each
  // lane and one, shorter, to add a lane.
  const layout = memo((): Row[] =>
    rows().flatMap((each) => [
      { each, lane: null },
      ...(memory.lanes.has(each.name) ? [...lanesOf(each).map((lane) => ({ each, lane })), { each, lane: null, add: true as const }] : []),
    ]),
  );
  // Where each row starts, in rows of a track, and where they all end.
  const heightOf = (row: Row) => (row.add ? ADD_LANE / ROW : 1);
  const tops = memo(() => layout().reduce<number[]>((all, row, index) => [...all, (all[index] ?? 0) + heightOf(row)], [0]));
  const topOf = (index: number) => tops()[index] ?? 0;
  /** The row at `y`, as its place in the layout. */
  const rowAt = (y: number) => Math.max(tops().findIndex((top) => top > y) - 1, 0);
  const runs = () => clock.tempo_map.time_signatures;
  // As far as the song shows: past what it holds, and past where it repeats.
  const end = memo(() => {
    const clips = rows().flatMap((each) => clipsOf(each).map((clip) => clip.state.start + lengthOf(clip.state)));
    const regions = rows().flatMap((each) => regionsOf(each).map((region) => region.state.start));
    const lanes = rows().flatMap((each) => (each.tool === "track" || each.tool === "tape" ? each.children : []).flatMap((child) => (child.tool === "lane" ? child.state.points.map((point) => point.tick) : [])));
    return Math.max(SHOWN, ...clips, ...regions, ...lanes, clock.repeat?.to ?? 0) + AFTER;
  });
  const starts = memo(() => barStarts(runs(), end()));
  /** The bar `tick` is in, from 1. */
  const barOf = (tick: number) => Math.max(starts().findIndex((start) => start > tick), 1);

  const clipAt = (key: string) => {
    const [trackName, clipName] = key.split("/");
    const clip = tracks().find((each) => each.name === trackName)?.children.find((child) => child.name === clipName);
    return clip?.tool === "timeline" ? clip : undefined;
  };
  const panelOf = () => (memory.below?.kind === "track" ? memory.below.name : null);
  const lengthAt = lengths({ children, outputs, clock });
  /** The clip or the region `key` names, `row/name`. */
  const itemAt = (key: string) => {
    const [rowName, name] = key.split("/");
    const item = rows().find((each) => each.name === rowName)?.children.find((child) => child.name === name);
    return item?.tool === "timeline" || item?.tool === "region" ? item : undefined;
  };
  /** Chooses the clips and regions `keys` names, `row/name`: the keyboard plays the track of the first, or `track`, and Export selection exports their bars. */
  const choose = (keys: Set<string>, track: string | null = null) => {
    memory.selected = keys;
    memory.point = null;
    memory.mark = null;
    memory.track = ([...keys][0] ?? track)?.split("/")[0] ?? null;
    const spans = [...keys].flatMap((key) => {
      const start = itemAt(key)?.state.start;
      return start === undefined ? [] : [{ from: start, to: start + lengthAt(key) }];
    });
    const span = spans.length === 0 ? undefined : { from: Math.min(...spans.map((each) => each.from)), to: Math.max(...spans.map((each) => each.to)) };
    select([...keys][0] ?? track, { span });
  };
  /** Shows the panel of the track `name` under the timeline, and gives it the keyboard. */
  const openPanel = (name: string) => {
    choose(new Set(), name);
    memory.below = { kind: "track", name };
  };
  /** Changes the clips `keys` names, `track/clip`, in one undo step. */
  const editClips = (label: string, keys: Iterable<string>, change: (state: FilledStateOfTool<"timeline">) => void) =>
    update(label, ({ children }) => {
      for (const key of keys) {
        const [trackName, clipName] = key.split("/");
        const row = children.find((each) => each.name === trackName);
        const clip = row?.tool === "track" ? row.children.find((child) => child.name === clipName) : undefined;
        if (clip?.tool === "timeline") change(clip.state);
      }
    });
  const toggleMute = (name: string) =>
    update("Mute a track", ({ children }) => {
      const owner = children.find((child) => child.name === name);
      const mix = owner?.tool === "track" || owner?.tool === "tape" || owner?.tool === "bus" ? owner.children.find((child) => child.name === "mix") : undefined;
      if (mix?.tool === "fader") mix.state.mute = !mix.state.mute;
    });
  const toggleSolo = (name: string) =>
    update("Solo a track", ({ state }) => {
      state.solo = state.solo.includes(name) ? state.solo.filter((each) => each !== name) : [...state.solo, name];
    });

  const timeline = now.space("timeline");
  // The playhead's line covers the point right of where it is, as the window's did.
  const playhead = now.tick.plus(timeline.pointX.times(0.5));
  const handle = timeline.pointX.times(6);

  // The window's own context, not a copy: it reads `time` anew each time.
  return Object.assign(song, {
    rows, tracks, returns, strips, layout, tops, topOf, rowAt, runs, end, starts, barOf,
    clipAt, panelOf, lengthAt, itemAt, choose, openPanel, editClips, toggleMute, toggleSolo,
    timeline, playhead, handle,
  });
}
/** What every part of the song's page gets. */
export type Page = ReturnType<typeof pageOf>;
