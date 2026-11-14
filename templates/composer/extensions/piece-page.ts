import { h, memo, type Accessor, type Color, type Style } from "./sdk";
import type { Piece } from "./piece";
import { barsOf, BEAT, laidOut, NO_MUSIC, partsOf, startsOf, ticksOf, type Bar, type Part } from "./piece-form";
import type { track } from "./track";

// What the parts of the piece's page share: its sizes and colours, its instruments, the open
// section and its bars, and what the view works out once for all of them.

// The sections column, the section head, the bar strip over the rows, and rows of 64 points with
// heads 176 wide. The rows under the heads, of the returns and the master, are 40.
export const SECTIONS = 200;
export const SECTION_HEAD = 72;
export const STRIP = 44;
export const ROW = 64;
export const HEADER = 176;
export const LOW_ROW = 40;
/** The room after the last bar of the open section, for the button that adds one. */
export const AFTER = 48;
/** How wide a bar of 4/4 is at least and at most, in points. */
const [NARROWEST, WIDEST] = [36, 200];

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

/** The section `memory.open` names, else the first. */
export const openSectionOf = ({ state, memory }: Pick<Piece, "state" | "memory">) => state.sections.find((section) => section.id === memory.open) ?? state.sections[0];
/** Where each section of the piece is, and what the clock keeps of them. */
export const formOf = (state: Piece["state"]) => laidOut(state.sections as Parameters<typeof laidOut>[0]);
export const barsIn = (section: Piece["state"]["sections"][number]) => barsOf(section.bars as Parameters<typeof barsOf>[0]);

/** The open section: its id, its bars, the tick each starts at from its start and where its last ends, and where it is in the piece. */
export function openOf(piece: Pick<Piece, "state" | "memory">) {
  const section = openSectionOf(piece);
  const place = section && formOf(piece.state).places[section.id];
  if (!section || !place) return undefined;
  const bars = barsIn(section);
  return { id: section.id, bars, starts: startsOf(bars), place };
}
/** Shows the section `id`, no bar chosen; the note editor goes on with its instrument's part there, or closes while it has none. */
export function showSection({ memory, children }: Pick<Piece, "memory" | "children">, id: string) {
  memory.open = id;
  memory.chosen = null;
  const below = memory.below;
  if (below?.kind === "editor" && !partAt(children.find((child) => child.name === below.track), id)) memory.below = { kind: "track", name: below.track };
}
/** Opens the section `id`, no bar chosen, and puts the playhead on its first bar. */
export function openAt(piece: Pick<Piece, "state" | "memory" | "clock" | "children">, id: string) {
  showSection(piece, id);
  piece.clock.jump(formOf(piece.state).places[id]?.start ?? 0);
}
/** Chooses the bars `from` to `to` of the open section, whole or of the instruments `rows`, and puts the playhead on the first. */
export function chooseBars(piece: Pick<Piece, "state" | "memory" | "clock">, from: number, to: number, rows: readonly string[] | null = null) {
  const open = openOf(piece);
  if (!open) return;
  const last = open.bars.length - 1;
  const [first, end] = [clamp(Math.min(from, to), 0, last), clamp(Math.max(from, to), 0, last)];
  piece.memory.chosen = { from: first, to: end, rows: rows && [...rows] };
  // Chosen notes would take the keys of the chosen bars.
  if (piece.memory.notes.size > 0) piece.memory.notes = new Set();
  piece.clock.jump(open.place.start + open.starts[first]!);
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
/** Shows the notes of the part the instrument `name` plays in the open section below. */
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

/** The part the note editor shows: of its instrument in the open section, or, while it has none, an empty one where the section is. */
export function editedPart(piece: Pick<Piece, "state" | "children" | "memory">) {
  const below = piece.memory.below;
  const section = openSectionOf(piece);
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
    if (part?.tool === "timeline") change(part.state);
  });

/** How long `bars` play, in seconds, each at its own tempo. */
export const secondsOf = (bars: readonly Bar[]) => bars.reduce((sum, { signature, bpm }) => sum + ((ticksOf(signature) / BEAT) * 60) / bpm, 0);
/** A time as a composer reads it: `1:21`. */
export const timeText = (seconds: number) => {
  const whole = Math.round(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
};

/** From which tick on a beat is how many ticks long: one for each run of bars of one time signature. */
export function beatRuns(runs: readonly { readonly signature: string; readonly bars: number }[]): { from: number; ticks: number }[] {
  let from = 0;
  return runs.map(({ signature, bars }) => {
    const run = { from, ticks: (4 * BEAT) / partsOf(signature)[1] };
    from += bars * ticksOf(signature);
    return run;
  });
}

/** The piece's view, with what its parts read, worked out once: its instruments, its sections, the open one and its bars. */
export function pageOf(piece: Piece & { size: { readonly width: number } }) {
  const { state, children, memory, update, size } = piece;
  const rows = memo(() => instrumentsOf(piece));
  const returns = () => children.filter((child) => child.tool === "bus");
  /** What has a panel: the instruments and the returns. */
  const strips = () => [...rows(), ...returns()];
  /** How tall the returns show under the heads, while the piece has any: the row that folds them, and open, a row each, four and a half before they scroll, which says there are more. */
  const returnsHeight = () => (returns().length === 0 ? 0 : LOW_ROW + (memory.returnsOpen ? Math.min(returns().length, 4.5) * LOW_ROW : 0));

  const sections = () => state.sections;
  /** Where each section is, and the tempo map and section names the clock keeps of them. `time` is the context's own, the clock of the sound. */
  const form = memo(() => formOf(state));
  // A memo of the id, so what reads it runs again only when another section opens.
  const openId = memo(() => openSectionOf(piece)?.id ?? null);
  const openSection = () => state.sections.find((section) => section.id === openId());
  const place = () => form().places[openId() ?? ""];
  /** The bars of the open section, each with its time signature and tempo. */
  const bars = memo(() => {
    const section = openSection();
    return section ? barsIn(section) : [];
  });
  /** The tick each bar of the open section starts at, from its start, and where its last ends. */
  const starts = memo(() => startsOf(bars()));
  /** The tick each bar of the piece starts at, and where its last ends. */
  const pieceStarts = memo(() => startsOf(state.sections.flatMap(barsIn)));
  /** Points per tick: the bars of the open section fill the room right of the heads, each as wide as it is long, between the narrowest and the widest bar. */
  const scale = () => clamp((size.width - SECTIONS - HEADER - AFTER) / Math.max(starts().at(-1) ?? 0, 1), NARROWEST / (4 * BEAT), WIDEST / (4 * BEAT));
  /** The bar of the open section at `x` points, from 0, or none past its end. */
  const barAt = (x: number) => {
    const ticks = x / scale();
    return ticks < 0 || ticks >= (starts().at(-1) ?? 0) ? null : Math.max(starts().findLastIndex((start) => start <= ticks), 0);
  };
  const open = (id: string) => openAt(piece, id);

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

  // The window's own context, not a copy: it reads `time` anew each time.
  return Object.assign(piece, {
    rows, returns, strips, returnsHeight,
    sections, form, openId, openSection, place, bars, starts, pieceStarts, scale, barAt, open, partAt,
    chosenTrack, choose, openPanel, openEditor, openMaster, toggleMute, toggleSolo,
  });
}
/** What every part of the piece's page gets. */
export type Page = ReturnType<typeof pageOf>;
