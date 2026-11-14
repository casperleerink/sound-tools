import type { FilledStateOfTool, TempoMap } from "./sdk";

// The form of a piece: sections of bars, each bar with its own time signature and tempo, and the
// music of an instrument in one section, in ticks from the start of that section. Nothing here
// reads the window or a record: it is what the piece's edits are made of.

export const BEAT = 960;

/** A bar: its time signature, such as `3/4`, and how fast it plays. */
export type Bar = { signature: string; bpm: number };
/** Bars in a row that are alike, as the piece's record holds them. */
export type Run = Bar & { bars: number };
/** A section as the piece's record holds it. */
export type Section = { id: string; name: string; bars: Run[] };
/** What one instrument plays in one section. */
export type Part = FilledStateOfTool<"timeline">;
/** A stretch of a part, in ticks from where it was taken: what a copy holds. */
export type Music = Pick<Part, "notes" | "pedal" | "bend" | "mod_wheel" | "pressure" | "controllers">;

export const NO_MUSIC: Music = { notes: [], pedal: [], bend: [], mod_wheel: [], pressure: [], controllers: {} };

/** The beats and the note value of a time signature, `4/4` for one that reads as none. */
export function partsOf(signature: string): [beats: number, note: number] {
  const [, beats, note] = /^(\d+)\/(1|2|4|8|16|32)$/.exec(signature) ?? [];
  return beats && Number(beats) > 0 ? [Number(beats), Number(note)] : [4, 4];
}
/** How many ticks a bar of `signature` is long. */
export function ticksOf(signature: string): number {
  const [beats, note] = partsOf(signature);
  return (beats * 4 * BEAT) / note;
}

/**
 * The beat a bar's tempo is said in, as a score says it: a dotted quarter in 6/8, 9/8 and 12/8,
 * else a quarter. How many quarters long it is, and its sign. A bar keeps its `bpm` in quarters.
 */
export function beatOf(signature: string): { quarters: number; sign: string } {
  const [beats, note] = partsOf(signature);
  return note === 8 && beats >= 6 && beats % 3 === 0 ? { quarters: 1.5, sign: "♩." } : { quarters: 1, sign: "♩" };
}
/** The tempo of a bar in its own beat, to a tenth. */
export const tempoOf = (bar: Bar) => Math.round((bar.bpm / beatOf(bar.signature).quarters) * 10) / 10;

export const barsOf = (runs: readonly Run[]): Bar[] =>
  runs.flatMap(({ bars, signature, bpm }) => Array.from({ length: Math.max(Math.round(bars), 0) }, () => ({ signature, bpm })));

export function runsOf(bars: readonly Bar[]): Run[] {
  const runs: Run[] = [];
  for (const { signature, bpm } of bars) {
    const last = runs.at(-1);
    if (last && last.signature === signature && last.bpm === bpm) last.bars += 1;
    else runs.push({ bars: 1, signature, bpm });
  }
  return runs;
}

/** The tick each of `bars` starts at, from the first, and after them the tick the last ends at. */
export function startsOf(bars: readonly Bar[]): number[] {
  const starts = [0];
  for (const bar of bars) starts.push(starts[starts.length - 1]! + ticksOf(bar.signature));
  return starts;
}

/** Where a section is in the piece: the tick it starts at, how long it is, and the number of its first bar, from 1. */
export type Place = { start: number; length: number; bar: number; bars: number };

/**
 * The piece in time, worked out from its sections: where each is, and what the clock keeps of
 * them, the time signatures as runs, a tempo change where the tempo changes, and a name at the
 * first bar of each section.
 */
export function laidOut(sections: readonly Section[]): { places: Record<string, Place>; end: number; tempo_map: TempoMap; sections: { bar: number; name: string }[] } {
  const places: Record<string, Place> = {};
  const signatures: TempoMap["time_signatures"] = [];
  const tempos: TempoMap["tempo_changes"] = [];
  const names: { bar: number; name: string }[] = [];
  let [tick, bar] = [0, 1];
  for (const section of sections) {
    const bars = barsOf(section.bars);
    if (bars.length === 0) continue;
    places[section.id] = { start: tick, length: startsOf(bars).at(-1)!, bar, bars: bars.length };
    names.push({ bar, name: section.name.trim() || "Section" });
    for (const each of bars) {
      const last = signatures.at(-1);
      if (last?.signature === each.signature) last.bars += 1;
      else signatures.push({ signature: each.signature, bars: 1 });
      if (tempos.at(-1)?.bpm !== each.bpm) tempos.push({ tick, bpm: each.bpm });
      tick += ticksOf(each.signature);
      bar += 1;
    }
  }
  if (signatures.length === 0) signatures.push({ signature: "4/4", bars: 1 });
  if (tempos.length === 0) tempos.push({ tick: 0, bpm: 120 });
  return { places, end: tick, tempo_map: { time_signatures: signatures, tempo_changes: tempos }, sections: names };
}

/** An id for a section called `name` that none of `taken` has: `sec-verse`, then `sec-verse-2`. */
export function idFor(name: string, taken: readonly string[]): string {
  const wanted = `sec-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "x"}`;
  let id = wanted;
  for (let number = 2; taken.includes(id); number++) id = `${wanted}-${number}`;
  return id;
}

// Changing the bars of a section: the bars it has after, and where each bar it had went.

/** The bars of a section after a change, and for each bar it had before, its place among them, or `null` when it is gone. */
export type Change = { bars: Bar[]; to: (number | null)[] };

/** The bars `from` to `to` with another time signature or tempo: what `change` gives for each. */
export const changed = (bars: readonly Bar[], from: number, to: number, change: (bar: Bar) => Partial<Bar>): Change => ({
  bars: bars.map((bar, index) => (index >= from && index <= to ? { ...bar, ...change(bar) } : bar)),
  to: bars.map((_, index) => index),
});
/** `added` put in before the bar `at`, or after the last. */
export const inserted = (bars: readonly Bar[], at: number, added: readonly Bar[]): Change => ({
  bars: [...bars.slice(0, at), ...added, ...bars.slice(at)],
  to: bars.map((_, index) => (index < at ? index : index + added.length)),
});
/** The bars `from` to `to` taken out. */
export const removed = (bars: readonly Bar[], from: number, to: number): Change => ({
  bars: bars.filter((_, index) => index < from || index > to),
  to: bars.map((_, index) => (index < from ? index : index > to ? index - (to - from + 1) : null)),
});

const byStart = (a: Part["notes"][number], b: Part["notes"][number]) => a.start - b.start || a.length - b.length || a.pitch - b.pitch || a.velocity - b.velocity;

/** Every line of a part but its notes, each point where `at` puts its tick, and gone where that is `null`. */
function movePoints(part: Music, at: (tick: number) => number | null) {
  const moved = <Point extends { tick: number }>(points: readonly Point[]) =>
    points.flatMap((point) => {
      const tick = at(point.tick);
      return tick === null ? [] : [{ ...point, tick }];
    });
  part.pedal = part.pedal.flatMap((change) => {
    const start = at(change.start);
    return start === null ? [] : [{ ...change, start }];
  });
  part.bend = moved(part.bend);
  part.mod_wheel = moved(part.mod_wheel);
  part.pressure = moved(part.pressure);
  part.controllers = Object.fromEntries(Object.entries(part.controllers).map(([number, points]) => [number, moved(points)]));
}

/**
 * Moves the music of a part along with the bars of its section, which were `was`: each note
 * keeps its place in its bar. A note in a bar that is gone, or past the end of one made shorter,
 * goes; one held across a bar that changed ends where it ended in its last bar, so it gets
 * longer with a longer bar and with bars put in under it.
 */
export function retime(part: Part, was: readonly Bar[], { bars, to }: Change) {
  const [old, next] = [startsOf(was), startsOf(bars)];
  const barAt = (tick: number) => Math.min(Math.max(old.findLastIndex((start) => start <= tick), 0), was.length - 1);
  const lengthAt = (place: number) => next[place + 1]! - next[place]!;
  const startAt = (tick: number): number | null => {
    const bar = barAt(tick);
    const place = to[bar];
    if (place === null || place === undefined) return null;
    const into = tick - old[bar]!;
    return into < lengthAt(place) ? next[place]! + into : null;
  };
  const endAt = (tick: number): number => {
    // An end on a bar line belongs to the bar it closes.
    const bar = barAt(tick - 1);
    const place = to[bar];
    if (place !== null && place !== undefined) return next[place]! + Math.min(tick - old[bar]!, lengthAt(place));
    // In a bar that is gone, it ends with the last bar before it that stayed.
    const before = to.slice(0, bar).findLast((each) => each !== null);
    return before === null || before === undefined ? 0 : next[before + 1]!;
  };
  part.notes = part.notes
    .flatMap((note) => {
      const start = startAt(note.start);
      const length = start === null ? 0 : endAt(note.start + note.length) - start;
      return start === null || length <= 0 ? [] : [{ ...note, start, length }];
    })
    .sort(byStart);
  movePoints(part, startAt);
  part.length = next.at(-1)!;
}

/**
 * The music of a part from `from` up to `to`, counted from `from`: what sounds there, a note cut
 * at `to`, and one held into it from before as a note of its own from the start, as a tied note
 * copies in a score.
 */
export function sliceOf(part: Music, from: number, to: number): Music {
  // Not `structuredClone`: the record a view reads is a proxy, which it refuses.
  const music = JSON.parse(JSON.stringify({ notes: part.notes, pedal: part.pedal, bend: part.bend, mod_wheel: part.mod_wheel, pressure: part.pressure, controllers: part.controllers })) as Music;
  music.notes = music.notes
    .flatMap((note) => {
      const [start, end] = [Math.max(note.start, from), Math.min(note.start + note.length, to)];
      return end > start ? [{ ...note, start: start - from, length: end - start }] : [];
    })
    .sort(byStart);
  movePoints(music, (tick) => (tick >= from && tick < to ? tick - from : null));
  return music;
}

/** Keeps a part inside `length` ticks, as a timeline plays it: what starts at or past it goes, and a note held past it ends there. */
export function fit(part: Music, length: number) {
  const points = [...part.pedal.map((change) => change.start), ...[part.bend, part.mod_wheel, part.pressure, ...Object.values(part.controllers)].flatMap((line) => line.map((point) => point.tick))];
  if (part.notes.some((note) => note.start + note.length > length) || points.some((tick) => tick >= length)) clear(part, length, Infinity);
}

/** Takes the music from `from` up to `to` out of a part: a note held into it ends at `from`. */
export function clear(part: Music, from: number, to: number) {
  part.notes = part.notes.flatMap((note) => {
    if (note.start >= from && note.start < to) return [];
    return note.start < from && note.start + note.length > from ? [{ ...note, length: from - note.start }] : [note];
  });
  movePoints(part, (tick) => (tick >= from && tick < to ? null : tick));
}

/** Puts `music`, `length` ticks of it, into a part from `at` on, in place of what was there, and cut at `end`, where the section ends. */
export function put(part: Music, at: number, music: Music, length: number, end: number) {
  const to = Math.min(at + length, end);
  if (to <= at) return;
  clear(part, at, to);
  const added = sliceOf(music, 0, to - at);
  part.notes = [...part.notes, ...added.notes.map((note) => ({ ...note, start: note.start + at }))].sort(byStart);
  movePoints(added, (tick) => tick + at);
  const joined = <Point extends { tick: number }>(a: readonly Point[], b: readonly Point[]) => [...a, ...b].sort((one, other) => one.tick - other.tick);
  part.pedal = [...part.pedal, ...added.pedal].sort((one, other) => one.start - other.start);
  part.bend = joined(part.bend, added.bend);
  part.mod_wheel = joined(part.mod_wheel, added.mod_wheel);
  part.pressure = joined(part.pressure, added.pressure);
  const numbers = new Set([...Object.keys(part.controllers), ...Object.keys(added.controllers)]);
  part.controllers = Object.fromEntries([...numbers].map((number) => [number, joined(part.controllers[number] ?? [], added.controllers[number] ?? [])]));
}

/** Whether `music` holds anything. */
export const isEmpty = (music: Music) =>
  music.notes.length === 0 && music.pedal.length === 0 && music.bend.length === 0 && music.mod_wheel.length === 0 && music.pressure.length === 0 && Object.values(music.controllers).every((points) => points.length === 0);
