// The piece as bars: where it ends, and bars added or taken out with everything after them
// moving with them, as notation software does. A piece is the clock's tempo map and sections,
// and the song's records: its tracks' clips, audio, lanes.
import type { Frozen, Section, TempoMap } from "../sdk";
import { changeBars } from "../song/marks";
import { lengthAt, lengthOf } from "../song/clips";
import { isRow, type Child, type Draft } from "../song/model";
import { barStarts, tickOfBar, type Runs } from "../song/time";

/** How many bars a piece with no music shows. */
export const FIRST_BARS = 16;
/** The room the score shows after the end line, in ticks: half a bar of 4/4. */
export const AFTER = 1920;

type Piece = { tempo_map: TempoMap; sections: Section[] };
/** The ticks a bar of `signature` lasts. */
export const ticksOfSignature = (signature: string) => {
  const [count, unit] = signature.split("/").map(Number);
  return ((count ?? 4) * 3840) / (unit ?? 4);
};

/** The signature of bar `bar`, from 1. */
export function signatureOf(runs: Runs, bar: number): string {
  const firsts = changeBars(runs);
  return runs[firsts.findLastIndex((first) => first <= bar)]?.signature ?? "4/4";
}

/** Where the piece ends: on the bar line after its last clip or audio; with none, after `FIRST_BARS`. */
export function pieceEnd(children: readonly Child[], tempo_map: Frozen<TempoMap>): number {
  return endLine(placesOf(children, tempo_map, ["timeline", "region"]).map(({ start, length }) => start + length), tempo_map.time_signatures);
}

/** Where each clip or audio part of `children` starts and how long it plays, in ticks. */
export function placesOf(children: readonly Child[], tempo_map: Frozen<TempoMap>, tools: readonly ("timeline" | "region")[]) {
  const song = { children, clock: { tempo_map } };
  return children.filter(isRow).flatMap((track) =>
    track.children.flatMap((child) => (child.tool === "timeline" || child.tool === "region") && tools.includes(child.tool) ? [{ start: child.state.start, length: lengthAt(song, `${track.name}/${child.name}`) }] : []),
  );
}

/** The bar line at or after the last of `ends`; with none, after `FIRST_BARS`. */
export function endLine(ends: readonly number[], runs: Runs): number {
  const end = ends.length === 0 ? tickOfBar(runs, FIRST_BARS + 1) : Math.max(...ends, 1);
  return barStarts(runs, end).find((start) => start >= end) ?? end;
}

/**
 * Where a tick goes when the `length` ticks from `from` are added (above 0) or taken out
 * (below 0): before `from` it stays, after it moves; inside what is taken out it is gone.
 */
export function retimed(tick: number, from: number, by: number): number | null {
  if (tick < from) return tick;
  if (by >= 0) return tick + by;
  return tick >= from - by ? tick + by : null;
}

/**
 * The tempo map and sections with `count` bars of the time signature of bar `bar` added before
 * it, or after it when `after`, its run growing; or taken out from it (below 0).
 */
export function withBars({ tempo_map, sections }: Frozen<Piece>, bar: number, count: number, after = false): Piece {
  const runs = tempo_map.time_signatures;
  const at = after ? bar + 1 : bar;
  const from = tickOfBar(runs, at);
  const by = count * ticksOfSignature(signatureOf(runs, bar));
  const index = changeBars(runs).findLastIndex((first) => first <= bar);
  // The last run goes on, so only a run before it counts its bars; runs of one time signature side by side are one.
  const time_signatures = runs
    .map((run, each) => (each === index && each < runs.length - 1 ? { ...run, bars: run.bars + count } : { ...run }))
    .filter((run) => run.bars > 0)
    .reduce<{ signature: string; bars: number }[]>((all, run) => {
      const last = all.at(-1);
      if (last?.signature === run.signature) last.bars += run.bars;
      else all.push(run);
      return all;
    }, []);
  const taken = (tick: number) => count < 0 && tick >= from && tick < from - by;
  // What a taken-out bar changed the tempo to plays after it, unless the bar after it changes it again.
  const next = tempo_map.tempo_changes.some((change) => change.tick === from - by);
  const kept = tempo_map.tempo_changes.filter((change) => !taken(change.tick) || (!next && change === tempo_map.tempo_changes.findLast((each) => taken(each.tick))));
  const tempo_changes = kept.map((change) => ({ ...change, tick: change.tick === 0 ? 0 : (retimed(change.tick, from, by) ?? from) }));
  const fermatas = (tempo_map.fermatas ?? []).flatMap((fermata) => {
    const tick = retimed(fermata.tick, from, by);
    return tick === null ? [] : [{ ...fermata, tick }];
  });
  const moved = sections.flatMap((section) => {
    if (section.bar < at) return [{ ...section }];
    if (count > 0) return [{ ...section, bar: section.bar + count }];
    // A section that started in a taken-out bar starts at the bar after it, unless one does.
    return section.bar < at - count && sections.some((each) => each.bar === at - count) ? [] : [{ ...section, bar: Math.max(section.bar + count, at) }];
  });
  return { tempo_map: { time_signatures, tempo_changes, ...(fermatas.length > 0 ? { fermatas } : {}) }, sections: moved };
}

/** Points of a clip or a lane, each at `at` ticks from 0, retimed; those taken out gone. */
function points<P>(list: readonly P[], at: (point: P) => number, set: (point: P, tick: number) => P, from: number, by: number): P[] {
  return list.flatMap((point) => {
    const tick = retimed(at(point), from, by);
    return tick === null ? [] : [set(point, tick)];
  });
}

/**
 * The clips, audio and lanes of the song's `children` with `by` ticks added at `from` (above 0)
 * or taken out from it (below 0). A clip over `from` grows or shrinks. A note that starts in what is taken
 * out goes, and one before it loses what lay in it; audio that starts in it starts where it was.
 */
export function moveMusic(children: Draft["children"], from: number, by: number) {
  const end = from - by;
  const point = <P extends { tick: number }>(each: P, tick: number) => ({ ...each, tick });
  for (const track of children) {
    if (!isRow(track)) continue;
    for (const [index, child] of [...track.children.entries()].reverse()) {
      if (child.tool === "region") child.state.start = retimed(child.state.start, from, by) ?? from;
      if (child.tool === "lane") child.state.points = points(child.state.points, (each) => each.tick, point, from, by);
      if (child.tool !== "timeline") continue;
      const clip = child.state;
      const [first, last] = [clip.start, clip.start + lengthOf(clip)];
      if (by < 0 && first >= from && last <= end) {
        track.children.splice(index, 1);
        continue;
      }
      const start = retimed(first, from, by) ?? from;
      // A clip that ends where bars go in keeps its end, so what it holds there does not ring on.
      const length = Math.max((last === from && by > 0 ? last : (retimed(last, from, by) ?? from)) - start, 1);
      const relative = <P>(list: readonly P[] | undefined, at: (each: P) => number, set: (each: P, tick: number) => P) =>
        points(list ?? [], (each) => first + at(each), (each, tick) => set(each, tick - start), from, by);
      const notes = clip.notes.flatMap((note) => {
        const noteStart = first + note.start;
        const moved = retimed(noteStart, from, by);
        if (moved === null) return [];
        // A note sounds to its clip's end at most; the clip bars go into must not make it longer.
        const length = noteStart < from && from < last ? Math.min(note.length, last - noteStart) : note.length;
        const cut = by < 0 && noteStart < from ? Math.max(Math.min(noteStart + length, end) - from, 0) : 0;
        return [{ ...note, start: moved - start, length: length - cut }];
      });
      child.state = {
        ...clip,
        start,
        ...(clip.length === null || clip.length === undefined ? {} : { length }),
        notes,
        pedal: relative(clip.pedal, (each) => each.start, (each, tick) => ({ ...each, start: tick })),
        bend: relative(clip.bend, (each) => each.tick, point),
        mod_wheel: relative(clip.mod_wheel, (each) => each.tick, point),
        pressure: relative(clip.pressure, (each) => each.tick, point),
        controllers: Object.fromEntries(Object.entries(clip.controllers ?? {}).map(([number, list]) => [number, relative(list, (each) => each.tick, point)])),
      };
    }
  }
}

/** The notes that start from `from` to before `to` taken out of every clip, and one ringing into them cut where they start; the bars stay. */
export function clearNotes(children: Draft["children"], from: number, to: number) {
  for (const track of children) {
    if (!isRow(track)) continue;
    for (const child of track.children) {
      if (child.tool !== "timeline") continue;
      const first = child.state.start;
      child.state.notes = child.state.notes.flatMap((note) => {
        const start = first + note.start;
        if (start >= from && start < to) return [];
        return start < from && start + note.length > from ? [{ ...note, length: from - start }] : [note];
      });
    }
  }
}
