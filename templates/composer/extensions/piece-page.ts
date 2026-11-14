import { h, memo, now, type Accessor, type Color, type Style } from "./sdk";
import type { Piece } from "./piece";
import type { Span } from "./piece-edits";
import { barsOf, BEAT, fit, laidOut, NO_MUSIC, partsOf, startsOf, ticksOf, type Part } from "./piece-form";
import type { track } from "./track";

// What the parts of the piece's page share: its sizes and colours, its instruments, the bars of
// the whole piece in one line, and what the view works out once for all of them.

// The band of marks over the rows, rows of 64 points with heads 176 wide. The rows under the
// heads, of the returns and the master, are 40.
export const MARKS = 98;
export const ROW = 64;
export const HEADER = 176;
export const LOW_ROW = 40;
/** Points per tick at zoom 1: a bar of 4/4 is 96 points. */
export const ACROSS = 0.025;
export const ZOOM = [0.1, 8] as const;
/** The ticks before the first bar, and after the last, where the buttons that add a bar and a section are. */
export const LEAD = 480;
export const AFTER = 6 * 4 * 960;

/** A row the composer chooses, an instrument's head, a return's, the master's or a section's: one look while it is chosen, and one under the pointer. */
export const rowLook = (chosen: boolean): Pick<Style, "background" | "hover"> => (chosen ? { background: "selected" } : { hover: { background: "fill" } });
/** How much of a muted instrument shows: its colour, its name and its notes. */
export const DIM = 0.4;
/** The dot before a name: of the colour of its instrument or return, dim while that is muted; hollow for the master, which has none. */
export const Mark = ({ colour, dim }: { colour?: Accessor<Color>; dim?: Accessor<boolean> }) =>
  h("div", {
    style: () => ({
      width: 8, height: 8, radius: "full", keep: true, opacity: dim?.() ? DIM : 1,
      ...(colour ? { background: colour() } : { borderWidth: 1.5, borderColor: "text-secondary" }),
    }),
  });
export const PALETTE = [
  { name: "Blue", colour: "blue" }, { name: "Peach", colour: "peach" }, { name: "Mauve", colour: "mauve" }, { name: "Teal", colour: "teal" },
  { name: "Green", colour: "green" }, { name: "Yellow", colour: "yellow" }, { name: "Pink", colour: "pink" }, { name: "Sky", colour: "sky" },
  { name: "Sapphire", colour: "sapphire" }, { name: "Red", colour: "red" }, { name: "Maroon", colour: "maroon" }, { name: "Lavender", colour: "lavender" },
  { name: "Rosewater", colour: "rosewater" }, { name: "Flamingo", colour: "flamingo" },
] as const;

export const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);

/** A child's name from any text: lowercase letters, digits and `-`. */
export const nameFrom = (text: string) => text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "x";

/** A child of the piece: an instrument, a return, the master or its limiter. */
export type Child = Piece["children"][number];
type TrackRow = Extract<Child, { tool: typeof track }>;

/** A name or a field in plain words, its unit left out: `osc_1.position` is `Osc 1 position`, `cutoff_hz` is `Cutoff`. */
const words = (text: string) =>
  text.replace(/_(hz|db|ms|seconds|cents|octaves|semitones)$/, "").replace(/[._-]+/g, " ").trim().replace(/^./, (first) => first.toUpperCase());
/** A name with no title, in words: `t00` is Instrument 1, `ret-a` Return A. */
const wordsOf = (name: string) => {
  const numbered = /^t(\d+)$/.exec(name);
  if (numbered) return `Instrument ${Number(numbered[1]) + 1}`;
  const [, letter] = /^ret-([a-z0-9]+)$/i.exec(name) ?? [];
  return letter ? `Return ${letter.toUpperCase()}` : words(name);
};
export const titleOf = (each: Child) => ((each.tool === "track" || each.tool === "bus") && each.state.title) || wordsOf(each.name);
export const mixOf = (each: Child) => (each.tool === "track" || each.tool === "bus" ? each.children.find((child) => child.name === "mix") : undefined);
export const muted = (each: Child) => {
  const mix = mixOf(each);
  return mix?.tool === "fader" && mix.state.mute === true;
};
// A text field holds any text: the colour is checked here, a colour of the theme or `#rrggbb`.
export const colourOf = (each: Child): Color => {
  const colour = each.tool === "track" || each.tool === "bus" ? each.state.colour : undefined;
  const named = PALETTE.find((each) => each.colour === colour)?.colour;
  return named ?? (colour?.startsWith("#") ? (colour as Color) : "blue");
};

/** Where each section of the piece is, and what the clock keeps of them. */
export const formOf = (state: Piece["state"]) => laidOut(state.sections as Parameters<typeof laidOut>[0]);
const barsIn = (section: Piece["state"]["sections"][number]) => barsOf(section.bars as Parameters<typeof barsOf>[0]);

/**
 * Every bar of the piece, one after the other through its sections: its time signature and
 * tempo, its section and its place there, from 0. `starts` has the tick each starts at, and
 * after them where the last ends.
 */
export function barsOfPiece(state: Pick<Piece["state"], "sections">) {
  const bars = state.sections.flatMap((section) => barsIn(section).map((bar, at) => ({ ...bar, id: section.id, at })));
  return { bars, starts: startsOf(bars) };
}
type PieceBars = ReturnType<typeof barsOfPiece>;
/** The sections the bars `from` to `to` of the piece cover, each with its own bars of them, from 0 in it. */
export function spansOf({ bars }: PieceBars, from: number, to: number): Span[] {
  const spans: Span[] = [];
  for (const { id, at } of bars.slice(Math.max(from, 0), to + 1)) {
    const last = spans.at(-1);
    if (last?.id === id) last.to = at;
    else spans.push({ id, from: at, to: at });
  }
  return spans;
}
/** The bar of the piece `tick` is in: the last past the end, the first before the start. */
export const barAtTick = ({ starts }: PieceBars, tick: number) => clamp(starts.findLastIndex((start) => start <= tick), 0, Math.max(starts.length - 2, 0));
/** The bars of the piece the section `id` holds, from its first to its last. */
export function barsOfSection({ bars }: PieceBars, id: string) {
  const [from, to] = [bars.findIndex((bar) => bar.id === id), bars.findLastIndex((bar) => bar.id === id)];
  return from < 0 ? undefined : { from, to };
}

/**
 * Puts the playhead on `tick` of the piece while it is stopped. While it plays, a choice does not
 * move what is heard, and the page turns no more with the playhead until the next play.
 */
function goTo({ clock, memory }: Pick<Piece, "clock" | "memory">, tick: number) {
  if (clock.playing) memory.follows = false;
  else clock.jump(tick);
}
/** Chooses the bars `from` to `to` of the piece, whole or of the instruments `rows`, and puts the playhead on the first. Its section's part is the one the note editor shows. */
export function chooseBars(piece: Pick<Piece, "state" | "memory" | "clock">, from: number, to: number, rows: readonly string[] | null = null) {
  const all = barsOfPiece(piece.state);
  const last = all.bars.length - 1;
  if (last < 0) return;
  const [first, end] = [clamp(Math.min(from, to), 0, last), clamp(Math.max(from, to), 0, last)];
  piece.memory.chosen = { from: first, to: end, rows: rows && [...rows] };
  piece.memory.part = all.bars[first]!.id;
  piece.memory.noteBar = null;
  piece.memory.notice = null;
  // Chosen notes would take the keys of the chosen bars.
  if (piece.memory.notes.size > 0) piece.memory.notes = new Set();
  goTo(piece, all.starts[first]!);
}

/** The instruments, in `order`: a row each. */
export const instrumentsOf = ({ state, children }: Pick<Piece, "state" | "children">) => {
  const rank = (name: string) => {
    const at = state.order.indexOf(name);
    return at < 0 ? state.order.length : at;
  };
  return children.filter((child): child is TrackRow => child.tool === "track").sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : 1));
};
/** Chooses the instrument or return `name`: the keyboard plays it. */
function chooseTrack({ memory, select }: Pick<Piece, "memory" | "select">, name: string | null) {
  memory.track = name;
  memory.effect = null;
  select(name);
}
/** Shows the panel of the instrument or return `name` below, and gives it the keyboard. */
export function showPanel(piece: Pick<Piece, "memory" | "select">, name: string) {
  chooseTrack(piece, name);
  const below = piece.memory.below;
  // One that shows already stays as it is, with its scroll and its cards.
  if (below?.kind !== "track" || below.name !== name) piece.memory.below = { kind: "track", name };
}
/** Shows the notes of the part the instrument `name` plays in the section of the chosen bars below. */
export function showEditor(piece: Pick<Piece, "memory" | "select">, name: string) {
  chooseTrack(piece, name);
  piece.memory.below = { kind: "editor", track: name };
  piece.memory.notes = new Set();
}
/** Chooses the instrument `name` as a press on its row does: the editor, while it shows, shows its notes, else its panel shows. */
export const showInstrument = (piece: Pick<Piece, "memory" | "select">, name: string) =>
  piece.memory.below?.kind === "editor" ? showEditor(piece, name) : showPanel(piece, name);

/** The part `track` plays in the section `id`: its `timeline` child, or none while it has no music there. */
export const partAt = (track: Child | undefined, id: string) => {
  const part = track?.tool === "track" ? track.children.find((child) => child.name === id) : undefined;
  return part?.tool === "timeline" ? part : undefined;
};

/** The section whose part the note editor shows: of the first chosen bar, else the first. */
export const partSectionOf = ({ state, memory }: Pick<Piece, "state" | "memory">) => state.sections.find((section) => section.id === memory.part) ?? state.sections[0];

/** The part the note editor shows: of its instrument in its section, or, while it has none, an empty one where the section is. */
export function editedPart(piece: Pick<Piece, "state" | "children" | "memory">) {
  const below = piece.memory.below;
  const section = partSectionOf(piece);
  if (below?.kind !== "editor" || !section) return undefined;
  const owner = piece.children.find((child) => child.name === below.track);
  const part = partAt(owner, section.id)?.state;
  if (part || owner?.tool !== "track") return part;
  const at = formOf(piece.state).places[section.id];
  return at ? { ...emptyPart(at.start, at.length), loop: false, plays: true, quantize: "bar" as const, title: "" } : undefined;
}
const emptyPart = (start: number, length: number) => ({ ...structuredClone(NO_MUSIC), start, length });

/** Changes the part `track` plays in the section `id` as one undo step: a part it has none of yet is made where its section is. */
export const editPart = ({ update }: Pick<Piece, "update">, track: string, id: string, label: string, change: (part: Part) => void) =>
  update(label, ({ state, children }) => {
    const owner = children.find((child) => child.name === track);
    if (owner?.tool !== "track") return;
    const at = laidOut(state.sections).places[id];
    if (!owner.children.some((child) => child.name === id) && at) owner.children.push({ name: id as `sec-${string}`, tool: "timeline", state: emptyPart(at.start, at.length) });
    const part = owner.children.find((child) => child.name === id);
    if (part?.tool !== "timeline") return;
    change(part.state);
    if (at) fit(part.state, at.length);
  });

/** From which tick on a beat is how many ticks long: one for each run of bars of one time signature. */
export function beatRuns(runs: readonly { readonly signature: string; readonly bars: number }[]): { from: number; ticks: number }[] {
  let from = 0;
  return runs.map(({ signature, bars }) => {
    const run = { from, ticks: (4 * BEAT) / partsOf(signature)[1] };
    from += bars * ticksOf(signature);
    return run;
  });
}

/** The piece's view, with what its parts read, worked out once: its instruments, its sections and its bars. */
export function pageOf(piece: Piece & { size: { readonly width: number } }) {
  const { state, children, memory, update } = piece;
  const rows = memo(() => instrumentsOf(piece));
  const returns = () => children.filter((child) => child.tool === "bus");
  /** What has a panel: the instruments and the returns. */
  const strips = () => [...rows(), ...returns()];
  /** How tall the returns show under the heads, while the piece has any: the row that folds them, and open, a row each, four and a half before they scroll, which says there are more. */
  const returnsHeight = () => (returns().length === 0 ? 0 : LOW_ROW + (memory.returnsOpen ? Math.min(returns().length, 4.5) * LOW_ROW : 0));

  const sections = () => state.sections;
  /** Where each section is, and the tempo map and section names the clock keeps of them. */
  const form = memo(() => formOf(state));
  /** Every bar of the piece, and the tick each starts at. */
  const all = memo(() => barsOfPiece(state));
  const starts = () => all().starts;
  const end = () => all().starts.at(-1) ?? 0;
  /** The bar of the piece at the tick `x`, or none outside it. */
  const barAt = (x: number) => (x < 0 || x >= end() ? null : barAtTick(all(), x));

  const panelOf = () => (memory.below?.kind === "track" ? memory.below.name : null);
  /** The instrument of what shows below: its panel, or the notes of its part. */
  const trackBelow = () => (memory.below?.kind === "editor" ? memory.below.track : panelOf());
  /** The one chosen instrument or return, which its head and its strip show: of what shows below, else, beside the mixer, the one the keyboard plays, as a click on a strip's name chooses it. */
  const chosenTrack = () => trackBelow() ?? (memory.below?.kind === "mixer" ? memory.track : null);
  const choose = (name: string | null) => chooseTrack(piece, name);
  const openPanel = (name: string) => showPanel(piece, name);
  const openEditor = (name: string) => showEditor(piece, name);
  const openMaster = () => {
    if (memory.below?.kind !== "master") memory.below = { kind: "master" };
  };
  const toggleMute = (name: string) =>
    update(children.find((child) => child.name === name)?.tool === "bus" ? "Mute a return" : "Mute an instrument", ({ children }) => {
      const owner = children.find((child) => child.name === name);
      const mix = owner?.tool === "track" || owner?.tool === "bus" ? owner.children.find((child) => child.name === "mix") : undefined;
      if (mix?.tool === "fader") mix.state.mute = !mix.state.mute;
    });
  const toggleSolo = (name: string) =>
    update("Solo an instrument", ({ state }) => {
      state.solo = state.solo.includes(name) ? state.solo.filter((each) => each !== name) : [...state.solo, name];
    });

  const space = now.space("rows");

  // The window's own context, not a copy: it reads `time` anew each time.
  return Object.assign(piece, {
    rows, returns, strips, returnsHeight,
    sections, form, all, starts, end, barAt, partAt,
    chosenTrack, choose, openPanel, openEditor, openMaster, toggleMute, toggleSolo,
    space,
  });
}
/** What every part of the piece's page gets. */
export type Page = ReturnType<typeof pageOf>;
