// Where the parts of a lane are in its plane: across in ticks of the song, down its travel, 0 at
// its top and 1 at its bottom.
import { BEATS, lfoAt, SHAPES } from "../../../lfo";
import { bent, type AutomationPoint, type Travel } from "../../../song/lanes";
import { BEAT } from "../../../song/time";
import { positionOf } from "../../value";

/** To a ten-thousandth, finer than a lane's height shows: a song of many points sends less to the window. */
export const fine = (value: number) => Math.round(value * 1e4) / 1e4;

/** How far down its lane `value` is. */
export const downOf = (value: number, travel: Travel) => fine(1 - Math.min(Math.max(positionOf(value, travel), 0), 1));

/** How many straight pieces a bent line is drawn in. */
const PIECES = 16;

/**
 * The line of `points` as segments end to end: flat from 0 to the first point and from the last
 * to `end`, and bent between two as the lane plays it.
 */
export function lineOf(points: readonly AutomationPoint[], travel: Travel, end: number): number[] {
  const first = points[0];
  if (!first) return [];
  const out = [0, downOf(first.value, travel), first.tick, downOf(first.value, travel)];
  points.forEach((point, index) => {
    const next = points[index + 1];
    const y = downOf(point.value, travel);
    if (!next) return out.push(point.tick, y, Math.max(end, point.tick), y);
    if (!point.curve) return out.push(point.tick, y, next.tick, downOf(next.value, travel));
    let [x0, y0] = [point.tick, y];
    for (let piece = 1; piece <= PIECES; piece++) {
      const [x1, y1] = [Math.round(point.tick + ((next.tick - point.tick) * piece) / PIECES), middleOf(point, next, travel, piece / PIECES)];
      out.push(x0, y0, x1, y1);
      [x0, y0] = [x1, y1];
    }
  });
  return out;
}

/** How far down the line from `point` to `next` is at `done` of the way, bent by the curve of `point`. */
export function middleOf(point: AutomationPoint, next: AutomationPoint, travel: Travel, done = 0.5): number {
  const [from, to] = [positionOf(point.value, travel), positionOf(next.value, travel)];
  return fine(1 - Math.min(Math.max(from + (to - from) * bent(done, point.curve ?? 0), 0), 1));
}

/** Where along a cycle each shape is drawn through: a square and a saw jump just before their middle and their end. */
const PHASES: Record<(typeof SHAPES)[number], number[]> = {
  sine: Array.from({ length: 9 }, (_, step) => step / 8),
  triangle: [0, 0.5, 1],
  square: [0, 0.5 - 1e-6, 0.5, 1 - 1e-6, 1],
  saw: [0, 1 - 1e-6, 1],
};

/** The wave an LFO plays from 0 to `end`, cycle after cycle, as segments. */
export function waveOf(lfo: { shape: (typeof SHAPES)[number]; rate: keyof typeof BEATS; low: number; high: number; scale: "linear" | "log" }, travel: Travel, end: number): number[] {
  const cycle = BEATS[lfo.rate] * BEAT;
  const out: number[] = [];
  let last: [number, number] | undefined;
  for (let start = 0; start < end; start += cycle) {
    for (const phase of PHASES[lfo.shape]) {
      // The wave starts again at low: a phase of 1 is the next cycle's 0.
      const at: [number, number] = [fine(start + phase * cycle), downOf(lfoAt(lfo, phase === 1 ? 0 : phase), travel)];
      if (last) out.push(...last, ...at);
      last = at;
    }
  }
  return out;
}

/** How long a piece of a long line is drawn alone, in ticks: a canvas as long as a song is more than a screen can hold. */
export const PIECE = 4 * 4 * BEAT;
/** The room each piece has on either side, in ticks, so a dot on its edge is whole. */
export const MARGIN = BEAT;

/** A dot at each point, as a line that goes nowhere: drawn with round ends, it is round. */
export const dotsOf = (points: readonly AutomationPoint[], travel: Travel) =>
  points.flatMap((point) => {
    const y = downOf(point.value, travel);
    return [point.tick, y, point.tick, y, point.tick, y, point.tick, y];
  });

/** `segments` cut into pieces `PIECE` long, each from its `start` and in ticks from `MARGIN` before it: only the pieces that show are drawn. */
export function piecesOf(segments: readonly number[]): { start: number; points: number[] }[] {
  const pieces = new Map<number, number[]>();
  const put = (piece: number, ...segment: number[]) => {
    const start = piece * PIECE;
    const into = pieces.get(start) ?? [];
    into.push(segment[0]! - start + MARGIN, segment[1]!, segment[2]! - start + MARGIN, segment[3]!);
    pieces.set(start, into);
  };
  for (let at = 0; at + 3 < segments.length; at += 4) {
    let [x0, y0] = [segments[at]!, segments[at + 1]!];
    const [x1, y1] = [segments[at + 2]!, segments[at + 3]!];
    // Cut where it crosses the edge of a piece, so each piece holds only what is in it.
    for (let piece = Math.floor(x0 / PIECE); ; piece++) {
      const edge = (piece + 1) * PIECE;
      if (x1 <= edge || x1 === x0) {
        put(piece, x0, y0, x1, y1);
        break;
      }
      const y = fine(y0 + ((y1 - y0) * (edge - x0)) / (x1 - x0));
      put(piece, x0, y0, edge, y);
      [x0, y0] = [edge, y];
    }
  }
  return [...pieces].map(([start, points]) => ({ start, points }));
}
