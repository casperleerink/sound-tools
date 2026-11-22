// The piece as bars: where it ends, and bars added or taken out with everything after them
// moving with them, as notation software does. A piece is the clock's tempo map and sections,
// and the song's records: its tracks' clips, audio, lanes.
import type { Section, TempoMap } from "../sdk";
import { changeBars } from "../song/marks";
import { lengthAt, lengthOf } from "../song/clips";
import { isRow, type Child } from "../song/model";
import { barStarts, tickOfBar, type Runs } from "../song/time";

/** How many bars a piece with no music shows. */
export const FIRST_BARS = 16;
/** The room the score shows after the end line, in ticks: half a bar of 4/4. */
export const AFTER = 1920;

type Piece = { tempo_map: TempoMap; sections: Section[] };
/** A piece as the clock gives it, to read. */
type Read = { readonly tempo_map: { readonly time_signatures: Runs; readonly tempo_changes: readonly TempoMap["tempo_changes"][number][]; readonly fermatas?: readonly { tick: number; hold: number }[] }; readonly sections: readonly Section[] };
type Note = { start: number; length: number };
type Point = { tick: number };
/** A clip as an update gets it: one made in the same update has only what it was made with. */
type Clip = { start: number; length?: number | null; notes: Note[]; pedal?: { start: number }[]; bend?: Point[]; mod_wheel?: Point[]; pressure?: Point[]; controllers?: Record<string, Point[]> };
/** A track or what is in it, as an update gets them, as far as time goes. */
export type Item = { name: string; tool: unknown; state: unknown; children?: Item[] };

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
export function pieceEnd(children: readonly Child[], tempo_map: Read["tempo_map"]): number {
  const runs = tempo_map.time_signatures;
  const song = { children, clock: { tempo_map } };
  const ends = children.filter(isRow).flatMap((track) =>
    track.children.flatMap((child) => (child.tool === "timeline" || child.tool === "region" ? [child.state.start + lengthAt(song, `${track.name}/${child.name}`)] : [])),
  );
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

/** The tempo map and sections with `count` bars of the signature at `bar` added before it (above 0), or taken out from it (below 0). */
export function withBars({ tempo_map, sections }: Read, bar: number, count: number): Piece {
  const runs = tempo_map.time_signatures;
  const from = tickOfBar(runs, bar);
  const by = count * ticksOfSignature(signatureOf(runs, bar));
  const firsts = changeBars(runs);
  const index = firsts.findLastIndex((first) => first <= bar);
  // The last run goes on, so only a run before it counts its bars.
  const time_signatures = runs
    .map((run, at) => (at === index && at < runs.length - 1 ? { ...run, bars: run.bars + count } : { ...run }))
    .filter((run) => run.bars > 0);
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
    if (section.bar < bar) return [section];
    if (count > 0) return [{ ...section, bar: section.bar + count }];
    // A section that started in a taken-out bar starts at the bar after it, unless one does.
    const at = Math.max(section.bar + count, bar);
    return section.bar < bar - count && sections.some((each) => each.bar === bar - count) ? [] : [{ ...section, bar: at }];
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
 * The clips, audio and lanes of `tracks` with `by` ticks added at `from` (above 0) or taken out
 * from it (below 0). A clip over `from` grows or shrinks, and one that ends there grows too, so
 * bars added at the end of the piece are in it; a note over what is taken out loses that part,
 * and one over added bars stays as it was.
 */
export function moveMusic(tracks: Item[], from: number, by: number) {
  const end = from - by;
  for (const track of tracks) {
    track.children = (track.children ?? []).flatMap((child) => {
      if (child.tool === "region") {
        const region = child.state as { start: number };
        const start = retimed(region.start, from, by);
        if (start === null) return [];
        region.start = start;
        return [child];
      }
      if (child.tool === "lane") {
        const lane = child.state as { points: Point[] };
        lane.points = points(lane.points, (point) => point.tick, (point, tick) => ({ ...point, tick }), from, by);
        return [child];
      }
      if (child.tool !== "timeline") return [child];
      const clip = child.state as Clip;
      const [first, last] = [clip.start, clip.start + lengthOf(clip)];
      if (by < 0 && first >= from && last <= end) return [];
      const start = retimed(first, from, by) ?? from;
      const length = Math.max((retimed(last, from, by) ?? from) - start, 1);
      const notes = clip.notes.flatMap((note) => {
        const [noteStart, noteEnd] = [first + note.start, first + note.start + note.length];
        const moved = retimed(noteStart, from, by);
        if (moved === null) return [];
        const cut = by < 0 && noteStart < from && noteEnd > from ? Math.min(noteEnd, end) - from : 0;
        return [{ ...note, start: moved - start, length: note.length - cut }];
      });
      const relative = <P>(list: readonly P[], at: (point: P) => number, set: (point: P, tick: number) => P) =>
        points(list, (point) => first + at(point), (point, tick) => set(point, tick - start), from, by);
      const point = (each: Point, tick: number) => ({ ...each, tick });
      child.state = {
        ...clip,
        start,
        ...(clip.length === null || clip.length === undefined ? {} : { length }),
        notes,
        pedal: relative(clip.pedal ?? [], (each) => each.start, (each, tick) => ({ ...each, start: tick })),
        bend: relative(clip.bend ?? [], (each) => each.tick, point),
        mod_wheel: relative(clip.mod_wheel ?? [], (each) => each.tick, point),
        pressure: relative(clip.pressure ?? [], (each) => each.tick, point),
        controllers: Object.fromEntries(Object.entries(clip.controllers ?? {}).map(([number, list]) => [number, relative(list, (each) => each.tick, point)])),
      };
      return [child];
    });
  }
}
