import { batch, type Frozen } from "./sdk";
import type { Piece } from "./piece";
import { barsOf, beatOf, changed, clear, endOf, fit, formText, held, idFor, inserted, isEmpty, laidOut, NO_MUSIC, paced, put, removed, retime, runsOf, slideOf, sliceOf, startsOf, ticksOf, type Bar, type Change, type Fermata, type Music } from "./piece-form";

// What the piece does to its form: each edit changes the sections in its record, moves the music
// of every instrument along, and puts the parts and the clock where the sections now are, as one
// undo step.

/** The piece's record and its children as an update changes them. */
type Draft = Parameters<Parameters<Piece["update"]>[1]>[0];
type DraftTrack = Extract<Draft["children"][number], { tool: "track" }>;
type SectionId = `sec-${string}`;

/** Bars `from` to `to` of the section `id`, counted from 0 in it: what an edit of bars works on, a choice cut at the section lines. */
export type Span = { id: string; from: number; to: number };
/** The chosen bars, counted from 0 through the whole piece, and of which instruments: `null` for the bars themselves, with all their music. `anchor` is the end the choice started at, which stays as shift takes more in or lets some go. */
export type Chosen = { from: number; to: number; rows: readonly string[] | null; anchor: number };
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
      // A section made shorter by hand: its part would not load with a note past its end.
      fit(part.state, at.length);
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

/** One change of the piece as one undo step: what `change` does to its record and its instruments, then the parts and the clock laid out again. With `memory`, it is the composer's, which ends a notice. */
export function edit({ update, clock, memory }: Pick<Piece, "update" | "clock"> & Partial<Pick<Piece, "memory">>, label: string, change: (draft: Draft) => void) {
  // Also after an `await`: a view that saw the record change before the clock would lay the piece out again, a second undo step.
  batch(() => {
    if (memory) memory.notice = null;
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
  });
}

/** An edit of the bars of the sections `ids` that tells the composer, quietly, of the notes it removed. */
function editTelling(piece: Piece, label: string, ids: readonly string[], change: (draft: Draft) => void) {
  const notesIn = (draft: Draft) => tracksOf(draft).reduce((sum, track) => sum + ids.reduce((count, id) => count + (partOf(track, id)?.state.notes.length ?? 0), 0), 0);
  const told = { removed: 0, form: "" };
  edit(piece, label, (draft) => {
    const before = notesIn(draft);
    change(draft);
    told.removed = before - notesIn(draft);
    told.form = formText(draft.state.sections);
  });
  if (told.removed > 0) {
    const text = told.removed === 1 ? "1 note did not fit and was removed. Undo brings it back." : `${told.removed} notes did not fit and were removed. Undo brings them back.`;
    piece.memory.notice = { text, form: told.form };
  }
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

/** Gives the bars of `spans` another time signature: a bar made longer pushes what comes after it. */
export const setSignature = (piece: Piece, spans: readonly Span[], signature: string) =>
  editTelling(piece, "Change the time signature", spans.map((span) => span.id), (draft) => {
    for (const { id, from, to } of spans) rebar(draft, id, (bars) => changed(bars, from, to, (bar) => held({ ...bar, signature }, bar.fermata)));
  });

/** Gives the bars of `spans` the tempo `tempo`, steady, each in its own beat: 80 is 80 dotted quarters a minute in a bar of 6/8. */
export const setTempo = (piece: Piece, spans: readonly Span[], tempo: number) =>
  edit(piece, "Change the tempo", (draft) => {
    for (const { id, from, to } of spans) rebar(draft, id, (bars) => changed(bars, from, to, (bar) => paced(bar, tempo * beatOf(bar.signature).quarters)));
  });

/** Every bar of the piece as a draft has it, in order, with its section and its place there. */
const barsIn = (draft: Draft) => draft.state.sections.flatMap((section) => barsOf(section.bars).map((bar, at) => ({ ...bar, id: section.id, at })));
/** Where the bar `at` of the section `id` is among the bars of the piece. */
const placeOf = (all: ReturnType<typeof barsIn>, id: string, at: number) => all.findIndex((bar) => bar.id === id && bar.at === at);

/** Puts the bars `from` to `to` of the piece on one straight line of tempo, from `start` to `end`, across section lines. */
function slideOver(draft: Draft, from: number, to: number, start: number, end: number) {
  const bars = barsIn(draft).slice(from, to + 1);
  const ends = startsOf(bars);
  const at = (bar: number) => start + (end - start) * (ends[bar]! / ends.at(-1)!);
  for (const id of new Set(bars.map((bar) => bar.id))) {
    const first = bars.findIndex((bar) => bar.id === id);
    const [low, high] = [bars[first]!.at, bars.findLast((bar) => bar.id === id)!.at];
    rebar(draft, id, (each) => changed(each, low, high, (bar, index) => paced(bar, at(first + index - low), at(first + index - low + 1))));
  }
}

/**
 * Has the tempo move over the bars of `spans` in a straight line, from the tempo the first starts
 * at to `tempo` at the end of the last, in the beat of the last: a ritardando or an accelerando.
 * With `null` they are steady again, at the tempo the first starts at.
 */
export const setSlide = (piece: Piece, spans: readonly Span[], tempo: number | null) =>
  edit(piece, tempo === null ? "Keep the tempo steady" : "Change the tempo gradually", (draft) => {
    const all = barsIn(draft);
    const [first, last] = [spans[0], spans.at(-1)];
    const [from, to] = first && last ? [placeOf(all, first.id, first.from), placeOf(all, last.id, last.to)] : [-1, -1];
    const [start, end] = [all[from], all[to]];
    if (start && end) slideOver(draft, from, to, start.bpm, tempo === null ? start.bpm : tempo * beatOf(end.signature).quarters);
  });

/** Puts a fermata on the bar `bar` of each section named, or with `null` takes it off: one undo step, so one moves from a bar to another. */
export const setFermatas = (piece: Piece, label: string, changes: readonly { id: string; bar: number; fermata: Fermata | null }[]) =>
  edit(piece, label, (draft) => {
    for (const { id, bar, fermata } of changes) rebar(draft, id, (bars) => changed(bars, bar, bar, (each) => held(each, fermata ?? undefined)));
  });

/**
 * Adds `count` empty bars before the bar `at` of a section, or after its last: each as the bar
 * before it ends, or the first starts, with no fermata. Between two bars of one rit. or accel.,
 * also across a section line, they join it: its line is spread over them too.
 */
export const addBars = (piece: Piece, id: string, at: number, count = 1) =>
  edit(piece, count === 1 ? "Add a bar" : "Add bars", (draft) => {
    const all = barsIn(draft);
    const next = at === 0 ? placeOf(all, id, 0) : placeOf(all, id, at - 1) + 1;
    const slide = next > 0 ? slideOf(all, next - 1, next) : undefined;
    const done = rebar(draft, id, (bars) => {
      const like = bars[at - 1] ?? bars[0];
      const bar: Bar = like ? { signature: like.signature, bpm: bars[at - 1] ? endOf(like) : like.bpm } : { signature: "4/4", bpm: 120 };
      return inserted(bars, at, Array.from({ length: count }, () => ({ ...bar })));
    });
    if (done && slide) slideOver(draft, slide.from, slide.to + count, slide.start, slide.end);
  });

/**
 * Takes the bars of `spans` out, with their music: what comes after moves up, and a section that
 * loses all its bars goes. The piece keeps one bar. From inside one rit. or accel., its line is
 * spread over the bars it keeps.
 */
export function removeBars(piece: Piece, spans: readonly Span[]) {
  const count = spans.reduce((sum, { from, to }) => sum + to - from + 1, 0);
  edit(piece, count === 1 ? "Remove a bar" : "Remove bars", (draft) => {
    const emptied = spans.filter(({ id, from, to }) => from === 0 && to >= barsOf(sectionOf(draft, id)?.bars ?? []).length - 1).map((span) => span.id);
    if (emptied.length >= draft.state.sections.length) return;
    const all = barsIn(draft);
    const [first, last] = [spans[0], spans.at(-1)];
    const [from, to] = first && last ? [placeOf(all, first.id, first.from), placeOf(all, last.id, last.to)] : [-1, -1];
    const slide = from > 0 && to >= from ? slideOf(all, from - 1, to + 1) : undefined;
    for (const { id, from, to } of spans) if (!emptied.includes(id)) rebar(draft, id, (bars) => removed(bars, from, to));
    draft.state.sections = draft.state.sections.filter((section) => !emptied.includes(section.id));
    for (const track of tracksOf(draft)) track.children = track.children.filter((child) => !emptied.includes(child.name));
    if (slide) slideOver(draft, slide.from, slide.to - count, slide.start, slide.end);
  });
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

const noMusic = (): Music => structuredClone(NO_MUSIC);

/** What the bars of `spans` hold, one after the other: of the instruments `rows`, or of all. */
export function copyOf({ state, children }: Pick<Piece, "state" | "children">, spans: readonly Span[], rows: readonly string[] | null): Copied {
  const tracks = children.filter((child) => child.tool === "track" && (rows === null || rows.includes(child.name)));
  const music = tracks.map(noMusic);
  const bars: Bar[] = [];
  for (const { id, from, to } of spans) {
    const all = barsOf((state.sections.find((section) => section.id === id)?.bars ?? []) as Draft["state"]["sections"][number]["bars"]);
    const starts = startsOf(all);
    const [start, end, at] = [starts[from] ?? 0, starts[to + 1] ?? starts.at(-1)!, startsOf(bars).at(-1)!];
    tracks.forEach((track, index) => {
      const part = track.tool === "track" ? track.children.find((child) => child.name === id) : undefined;
      if (part?.tool === "timeline") put(music[index]!, at, sliceOf(part.state as Frozen<Music> as Music, start, end), end - start, at + end - start);
    });
    bars.push(...all.slice(from, to + 1));
  }
  return { bars, rows: tracks.map((track, index) => ({ name: track.name, music: music[index]! })) };
}

/** The bars of `spans` again, right after the last of them, with their music. */
export function duplicateBars(piece: Piece, spans: readonly Span[]) {
  const last = spans.at(-1);
  if (!last) return;
  const copied = copyOf(piece, spans, null);
  edit(piece, copied.bars.length === 1 ? "Duplicate a bar" : "Duplicate bars", (draft) => insertCopied(draft, last.id, last.to + 1, copied));
}

/** Clears the music of the instruments `rows`, or of all, in the bars of `spans`: the bars stay. */
export function clearMusic(piece: Piece, spans: readonly Span[], rows: readonly string[] | null) {
  edit(piece, "Clear the music", (draft) => {
    for (const { id, from, to } of spans) {
      const starts = startsOf(barsOf(sectionOf(draft, id)?.bars ?? []));
      for (const track of tracksOf(draft)) {
        const part = partOf(track, id);
        if (part && (rows === null || rows.includes(track.name))) clear(part.state, starts[from] ?? 0, starts[to + 1] ?? starts.at(-1)!);
      }
    }
  });
}

/**
 * Pastes `copied` at the bar `bar` of the section `id`. As bars, with `rows` null, it puts its
 * bars in before that bar, with their music on the instruments it came from. On the instruments
 * `rows` it writes its music over what they play from that bar on, the first copied instrument
 * on the first of them and the next on those under it, in `order`, through the sections after
 * it, each keeping its own part; the last section gets bars at its end where the piece is too short.
 */
export function paste(piece: Piece, { id, bar }: { id: string; bar: number }, rows: readonly string[] | null, copied: Copied, order: readonly string[]) {
  if (rows === null) return edit(piece, "Paste bars", (draft) => insertCopied(draft, id, bar, copied));
  edit(piece, "Paste", (draft) => {
    const length = startsOf(copied.bars).at(-1)!;
    const place = laidOut(draft.state.sections).places[id];
    if (!place) return;
    const start = place.start + (startsOf(barsOf(sectionOf(draft, id)?.bars ?? []))[bar] ?? place.length);
    const last = draft.state.sections.at(-1);
    // Music that would end past the piece gets the bars it needs, each as the last ends, with no fermata: none of it is cut.
    if (last) {
      rebar(draft, last.id, (bars) => {
        const added: Bar[] = [];
        const end = bars.at(-1);
        for (let tick = laidOut(draft.state.sections).end; end && tick < start + length; tick += ticksOf(end.signature)) added.push({ signature: end.signature, bpm: endOf(end) });
        return inserted(bars, bars.length, added);
      });
    }
    const first = order.findIndex((name) => rows.includes(name));
    if (first < 0) return;
    for (const [section, at] of Object.entries(laidOut(draft.state.sections).places)) {
      const [from, to] = [Math.max(start, at.start), Math.min(start + length, at.start + at.length)];
      if (to <= from) continue;
      copied.rows.forEach((row, index) => {
        const track = tracksOf(draft).find((each) => each.name === order[first + index]);
        if (track) withPart(track, section, (music) => put(music, from - at.start, sliceOf(row.music, from - start, to - start), to - from, at.length));
      });
    }
  });
}

// Sections

/** The ids a new section may not take: those of the sections, and the names of what the instruments hold, such as a part left from a section that is gone. */
const takenBy = ({ state, children }: Pick<Piece, "state" | "children">) => [
  ...state.sections.map((section) => section.id),
  ...children.flatMap((track) => (track.tool === "track" ? track.children.map((child) => child.name) : [])),
];
/** `Section 3` for the third: the first such name no section has. */
function freeNameOf(sections: readonly { readonly name: string }[]): string {
  for (let number = sections.length + 1; ; number++) {
    if (!sections.some((section) => section.name === `Section ${number}`)) return `Section ${number}`;
  }
}

/** Adds a section of eight bars after the section `after`, or at the end, with the time signature and tempo of the bar before it: its id. */
export function addSection(piece: Piece, after: string | null): string {
  const sections = piece.state.sections;
  const at = after === null ? sections.length : sections.findIndex((section) => section.id === after) + 1 || sections.length;
  const last = barsOf(sections[at - 1]?.bars ?? []).at(-1);
  const [signature, bpm] = last ? [last.signature, endOf(last)] : ["4/4", 120];
  const name = freeNameOf(sections);
  const id = idFor(name, takenBy(piece));
  edit(piece, "Add a section", (draft) => {
    draft.state.sections.splice(at, 0, { id, name, bars: runsOf(Array.from({ length: 8 }, () => ({ signature, bpm }))) });
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
  const copy = idFor(source.name, takenBy(piece));
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
  const name = freeNameOf(sections);
  const next = idFor(name, takenBy(piece));
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
