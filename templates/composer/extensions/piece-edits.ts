import type { Frozen } from "./sdk";
import type { Piece } from "./piece";
import { barsOf, beatOf, changed, clear, idFor, inserted, isEmpty, laidOut, NO_MUSIC, put, removed, retime, runsOf, sliceOf, startsOf, ticksOf, type Bar, type Change, type Music } from "./piece-form";

// What the piece does to its form: each edit changes the sections in its record, moves the music
// of every instrument along, and puts the parts and the clock where the sections now are, as one
// undo step.

/** The piece's record and its children as an update changes them. */
type Draft = Parameters<Parameters<Piece["update"]>[1]>[0];
type DraftTrack = Extract<Draft["children"][number], { tool: "track" }>;
type SectionId = `sec-${string}`;

/** The chosen bars of the open section, from 0, and of which instruments: `null` for the bars themselves, with all their music. */
export type Chosen = { from: number; to: number; rows: readonly string[] | null };
/** What a copy holds: the bars, and the music of each instrument it was taken from. */
export type Copied = { bars: Bar[]; rows: { name: string; music: Music }[] };

const tracksOf = (draft: Draft) => draft.children.filter((child): child is DraftTrack => child.tool === "track");
const partOf = (track: DraftTrack, id: string) => {
  const part = track.children.find((child) => child.name === id);
  return part?.tool === "timeline" ? part : undefined;
};
const sectionOf = (draft: Draft, id: string) => draft.state.sections.find((section) => section.id === id);
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** Changes the music `track` plays in the section `id`: a part that had none gets one when the change leaves it music. */
function withPart(track: DraftTrack, id: string, change: (music: Music) => void) {
  const part = partOf(track, id);
  if (part) return change(part.state);
  const music: Music = { ...NO_MUSIC, controllers: {} };
  change(music);
  if (!isEmpty(music)) track.children.push({ name: id as SectionId, tool: "timeline", state: music });
}

/** Puts every part where its section is, as long as its section. */
function place(draft: Draft) {
  const time = laidOut(draft.state.sections);
  for (const track of tracksOf(draft)) {
    for (const part of track.children) {
      const at = part.tool === "timeline" ? time.places[part.name] : undefined;
      if (part.tool !== "timeline" || !at) continue;
      if (part.state.start !== at.start) part.state.start = at.start;
      if (part.state.length !== at.length) part.state.length = at.length;
    }
  }
  return time;
}

/** Whether the parts and the clock are where the sections of the piece say: not so after its files were changed by hand. */
export function isLaidOut({ state, children, clock }: Pick<Piece, "state" | "children" | "clock">): boolean {
  const time = laidOut(state.sections as Draft["state"]["sections"]);
  if (!same(clock.tempo_map, time.tempo_map) || !same(clock.sections, time.sections)) return false;
  return children.every((track) =>
    track.tool !== "track" || track.children.every((part) => {
      const at = part.tool === "timeline" ? time.places[part.name] : undefined;
      return part.tool !== "timeline" || !at || (part.state.start === at.start && part.state.length === at.length);
    }),
  );
}

/** One change of the piece as one undo step: what `change` does to its record and its instruments, then the parts and the clock laid out again. */
export function edit({ update, clock }: Pick<Piece, "update" | "clock">, label: string, change: (draft: Draft) => void) {
  const laid: { time?: ReturnType<typeof laidOut> } = {};
  update(label, (draft) => {
    change(draft);
    laid.time = place(draft);
  });
  const time = laid.time;
  if (!time || (same(clock.tempo_map, time.tempo_map) && same(clock.sections, time.sections))) return;
  clock.update(label, (song) => {
    song.tempo_map = time.tempo_map;
    song.sections = time.sections;
  });
}

/** Changes the bars of the section `id` and moves the music of every instrument in it along: the change, or none when it would leave no bar. */
function rebar(draft: Draft, id: string, change: (bars: Bar[]) => Change): { was: Bar[]; now: Change } | undefined {
  const section = sectionOf(draft, id);
  if (!section) return undefined;
  const was = barsOf(section.bars);
  const now = change(was);
  if (now.bars.length === 0) return undefined;
  section.bars = runsOf(now.bars);
  for (const track of tracksOf(draft)) {
    const part = partOf(track, id);
    if (part) retime(part.state, was, now);
  }
  return { was, now };
}

// Bars

/** Gives the bars `from` to `to` of a section another time signature: a bar made longer pushes what comes after it. */
export const setSignature = (piece: Piece, id: string, from: number, to: number, signature: string) =>
  edit(piece, "Change the time signature", (draft) => void rebar(draft, id, (bars) => changed(bars, from, to, () => ({ signature }))));

/** Gives the bars `from` to `to` of a section the tempo `tempo`, each in its own beat: 80 is 80 dotted quarters a minute in a bar of 6/8. */
export const setTempo = (piece: Piece, id: string, from: number, to: number, tempo: number) =>
  edit(piece, "Change the tempo", (draft) => void rebar(draft, id, (bars) => changed(bars, from, to, (bar) => ({ bpm: tempo * beatOf(bar.signature).quarters }))));

/** Adds `count` empty bars before the bar `at` of a section, or after its last: each as the bar before it, or the first. */
export const addBars = (piece: Piece, id: string, at: number, count = 1) =>
  edit(piece, count === 1 ? "Add a bar" : "Add bars", (draft) =>
    void rebar(draft, id, (bars) => inserted(bars, at, Array.from({ length: count }, () => ({ ...(bars[at - 1] ?? bars[0] ?? { signature: "4/4", bpm: 120 }) })))),
  );

/** Takes the bars `from` to `to` out of a section, with their music: what comes after moves up. A section keeps one bar. */
export const removeBars = (piece: Piece, id: string, from: number, to: number) =>
  edit(piece, from === to ? "Remove a bar" : "Remove bars", (draft) => void rebar(draft, id, (bars) => removed(bars, from, to)));

/** Makes a section `count` bars long: bars added at its end, or taken from it. */
export function setLength(piece: Piece, id: string, count: number) {
  const bars = barsOf(piece.state.sections.find((section) => section.id === id)?.bars ?? []).length;
  if (count > bars) addBars(piece, id, bars, count - bars);
  else if (count >= 1 && count < bars) removeBars(piece, id, count, bars - 1);
}

/** Puts `copied` into the section `id` before the bar `at`: its bars, and its music on the instruments it came from that the piece still has. */
function insertCopied(draft: Draft, id: string, at: number, copied: Copied) {
  const done = rebar(draft, id, (bars) => inserted(bars, at, copied.bars));
  if (!done) return;
  const starts = startsOf(done.now.bars);
  const [from, to] = [starts[at]!, starts[at + copied.bars.length]!];
  for (const track of tracksOf(draft)) {
    const row = copied.rows.find((each) => each.name === track.name);
    if (row) withPart(track, id, (music) => put(music, from, row.music, to - from, starts.at(-1)!));
  }
}

/** What the bars `from` to `to` of a section hold: of the instruments `rows`, or of all. */
export function copyOf({ state, children }: Pick<Piece, "state" | "children">, id: string, { from, to, rows }: Chosen): Copied {
  const bars = barsOf((state.sections.find((section) => section.id === id)?.bars ?? []) as Draft["state"]["sections"][number]["bars"]);
  const starts = startsOf(bars);
  const [start, end] = [starts[from] ?? 0, starts[to + 1] ?? starts.at(-1)!];
  const tracks = children.filter((child) => child.tool === "track" && (rows === null || rows.includes(child.name)));
  return {
    bars: bars.slice(from, to + 1),
    rows: tracks.flatMap((track) => {
      const part = track.tool === "track" ? track.children.find((child) => child.name === id) : undefined;
      return [{ name: track.name, music: part?.tool === "timeline" ? sliceOf(part.state as Frozen<Music> as Music, start, end) : { ...NO_MUSIC } }];
    }),
  };
}

/** The bars `from` to `to` of a section again, right after them, with their music. */
export function duplicateBars(piece: Piece, id: string, from: number, to: number) {
  const copied = copyOf(piece, id, { from, to, rows: null });
  edit(piece, from === to ? "Duplicate a bar" : "Duplicate bars", (draft) => insertCopied(draft, id, to + 1, copied));
}

/** Clears the music of the chosen instruments, or of all, in the bars `from` to `to` of a section: the bars stay. */
export function clearMusic(piece: Piece, id: string, { from, to, rows }: Chosen) {
  edit(piece, "Clear the music", (draft) => {
    const starts = startsOf(barsOf(sectionOf(draft, id)?.bars ?? []));
    for (const track of tracksOf(draft)) {
      const part = partOf(track, id);
      if (part && (rows === null || rows.includes(track.name))) clear(part.state, starts[from] ?? 0, starts[to + 1] ?? starts.at(-1)!);
    }
  });
}

/**
 * Pastes `copied` into the section `id`. On chosen bars it puts its bars in before the first of
 * them, with its music on the instruments it came from. On chosen instruments it writes its
 * music over what they play from the first chosen bar on, the first copied instrument on the
 * first chosen one and the next on those under it, in `order`; a section too short for it gets bars at its end.
 */
export function paste(piece: Piece, id: string, chosen: Chosen, copied: Copied, order: readonly string[]) {
  const rows = chosen.rows;
  if (rows === null) return edit(piece, "Paste bars", (draft) => insertCopied(draft, id, chosen.from, copied));
  edit(piece, "Paste", (draft) => {
    const length = startsOf(copied.bars).at(-1)!;
    const before = startsOf(barsOf(sectionOf(draft, id)?.bars ?? []));
    const at = before[chosen.from] ?? before.at(-1)!;
    // Music that would end past the section gets the bars it needs, each as the last: none of it is cut.
    rebar(draft, id, (bars) => {
      const added: Bar[] = [];
      for (let end = before.at(-1)!; end < at + length && bars.length > 0; end += ticksOf(bars.at(-1)!.signature)) added.push({ ...bars.at(-1)! });
      return inserted(bars, bars.length, added);
    });
    const end = startsOf(barsOf(sectionOf(draft, id)?.bars ?? [])).at(-1)!;
    const first = order.findIndex((name) => rows.includes(name));
    copied.rows.forEach((row, index) => {
      const track = tracksOf(draft).find((each) => each.name === order[first + index]);
      if (first >= 0 && track) withPart(track, id, (music) => put(music, at, row.music, length, end));
    });
  });
}

// Sections

/** Adds a section of eight bars after the section `after`, or at the end, with the time signature and tempo of the bar before it: its id. */
export function addSection(piece: Piece, after: string | null): string {
  const sections = piece.state.sections;
  const at = after === null ? sections.length : sections.findIndex((section) => section.id === after) + 1 || sections.length;
  const before = sections[at - 1]?.bars.at(-1) ?? sections[0]?.bars[0];
  const name = `Section ${sections.length + 1}`;
  const id = idFor(name, sections.map((section) => section.id));
  edit(piece, "Add a section", (draft) => {
    draft.state.sections.splice(at, 0, { id, name, bars: [{ bars: 8, signature: before?.signature ?? "4/4", bpm: before?.bpm ?? 120 }] });
  });
  return id;
}

/** Names a section: no name leaves it as it was. Its id, and so its files, stay. */
export function renameSection(piece: Piece, id: string, name: string) {
  if (name.trim() === "") return;
  edit(piece, "Rename a section", (draft) => {
    const section = sectionOf(draft, id);
    if (section) section.name = name.trim();
  });
}

/** Moves a section before the section `before`, or to the end: its music goes with it. */
export function moveSection(piece: Piece, id: string, before: string | null) {
  if (id === before) return;
  edit(piece, "Move a section", (draft) => {
    const sections = draft.state.sections;
    const moved = sectionOf(draft, id);
    if (!moved) return;
    const rest = sections.filter((section) => section.id !== id);
    const at = before === null ? rest.length : rest.findIndex((section) => section.id === before);
    rest.splice(at < 0 ? rest.length : at, 0, moved);
    draft.state.sections = rest;
  });
}

/** A section again, right after it, with the music of every instrument: the id of the copy. */
export function duplicateSection(piece: Piece, id: string): string | undefined {
  const sections = piece.state.sections;
  const source = sections.find((section) => section.id === id);
  if (!source) return undefined;
  const copy = idFor(source.name, sections.map((section) => section.id));
  edit(piece, "Duplicate a section", (draft) => {
    const at = draft.state.sections.findIndex((section) => section.id === id);
    const from = draft.state.sections[at];
    if (!from) return;
    draft.state.sections.splice(at + 1, 0, { id: copy, name: from.name, bars: from.bars.map((run) => ({ ...run })) });
    const end = startsOf(barsOf(from.bars)).at(-1)!;
    for (const track of tracksOf(draft)) {
      const part = partOf(track, id);
      if (part) track.children.push({ name: copy as SectionId, tool: "timeline", state: sliceOf(part.state, 0, end) });
    }
  });
  return copy;
}

/** Deletes a section with its music; the piece keeps one. */
export function deleteSection(piece: Piece, id: string) {
  if (piece.state.sections.length <= 1) return;
  edit(piece, "Delete a section", (draft) => {
    draft.state.sections = draft.state.sections.filter((section) => section.id !== id);
    for (const track of tracksOf(draft)) track.children = track.children.filter((child) => child.name !== id);
  });
}

/** Starts a new section at the bar `bar` of the section `id`, from 0: that bar and those after it, with their music, leave it. Its id, or none at the first bar. */
export function splitSection(piece: Piece, id: string, bar: number): string | undefined {
  const sections = piece.state.sections;
  const source = sections.find((section) => section.id === id);
  if (!source || bar <= 0 || bar >= barsOf(source.bars as Draft["state"]["sections"][number]["bars"]).length) return undefined;
  const name = `Section ${sections.length + 1}`;
  const next = idFor(name, sections.map((section) => section.id));
  edit(piece, "Start a section", (draft) => {
    const at = draft.state.sections.findIndex((section) => section.id === id);
    const section = draft.state.sections[at];
    if (!section) return;
    const bars = barsOf(section.bars);
    const starts = startsOf(bars);
    const [cut, end] = [starts[bar]!, starts.at(-1)!];
    section.bars = runsOf(bars.slice(0, bar));
    draft.state.sections.splice(at + 1, 0, { id: next, name, bars: runsOf(bars.slice(bar)) });
    for (const track of tracksOf(draft)) {
      const part = partOf(track, id);
      if (!part) continue;
      const rest = sliceOf(part.state, cut, end);
      clear(part.state, cut, end);
      if (!isEmpty(rest)) track.children.push({ name: next as SectionId, tool: "timeline", state: rest });
    }
  });
  return next;
}

/** Joins a section with the one after it: one section under its name, with the bars and the music of both. */
export function joinSection(piece: Piece, id: string) {
  edit(piece, "Join sections", (draft) => {
    const at = draft.state.sections.findIndex((section) => section.id === id);
    const [section, next] = [draft.state.sections[at], draft.state.sections[at + 1]];
    if (!section || !next) return;
    const [length, added] = [startsOf(barsOf(section.bars)).at(-1)!, startsOf(barsOf(next.bars)).at(-1)!];
    section.bars = runsOf([...barsOf(section.bars), ...barsOf(next.bars)]);
    draft.state.sections.splice(at + 1, 1);
    for (const track of tracksOf(draft)) {
      const joined = partOf(track, next.id);
      if (joined) withPart(track, id, (music) => put(music, length, joined.state, added, length + added));
      track.children = track.children.filter((child) => child.name !== next.id);
    }
  });
}
