// Time of the song: ticks and seconds through the tempo changes, bars through the time
// signatures, the snap's grid, and a place as the composer reads it, bar.beat.sixteenth.

export const BEAT = 960;
export const BAR = 4 * BEAT;

type TempoChanges = readonly { tick: number; bpm: number }[];
/** The time signatures, as runs of bars: the last run goes on. */
export type Runs = readonly { readonly signature: string; readonly bars: number }[];

/** Ticks as seconds at `bpm`. */
const secondsAt = (ticks: number, bpm: number) => (ticks / BEAT) * (60 / bpm);

/** The tempo at `tick`, and the tick of the next change after it. */
function tempoAt(changes: TempoChanges, tick: number): { bpm: number; next: number } {
  const bpm = [...changes].reverse().find((change) => change.tick <= tick)?.bpm ?? 120;
  return { bpm, next: changes.find((change) => change.tick > tick)?.tick ?? Infinity };
}

/** The seconds from `from` to `to`, through each tempo between: below 0 when `to` is before. */
export function secondsBetween(changes: TempoChanges, from: number, to: number): number {
  if (to < from) return -secondsBetween(changes, to, from);
  let seconds = 0;
  for (let tick = from; tick < to; ) {
    const { bpm, next } = tempoAt(changes, tick);
    const end = Math.min(next, to);
    seconds += secondsAt(end - tick, bpm);
    tick = end;
  }
  return seconds;
}

/** The ticks `seconds` take from `from`, through each tempo after it. */
export function ticksAcross(changes: TempoChanges, from: number, seconds: number): number {
  let [tick, left] = [from, seconds];
  while (left > 0) {
    const { bpm, next } = tempoAt(changes, tick);
    const fits = secondsAt(next - tick, bpm);
    if (left <= fits) return tick + (left * bpm * BEAT) / 60 - from;
    [tick, left] = [next, left - fits];
  }
  return tick - from;
}

/** The ticks `seconds` take up to `to`, through each tempo before it; at most `to`. */
export function ticksBefore(changes: TempoChanges, to: number, seconds: number): number {
  let [low, high] = [0, to];
  for (let i = 0; i < 48; i++) {
    const middle = (low + high) / 2;
    if (secondsBetween(changes, middle, to) > seconds) low = middle;
    else high = middle;
  }
  return to - high;
}

const ticksOfBar = (signature: string) => {
  const [count, unit] = signature.split("/").map(Number);
  return ((count ?? 4) * 4 * BEAT) / (unit ?? 4);
};
const runsOrFirst = (runs: Runs): Runs => (runs.length > 0 ? runs : [{ signature: "4/4", bars: 1 }]);

/** The tick each bar starts at, up to the first at or past `end`. */
export function barStarts(runs: Runs, end: number): number[] {
  const starts = [0];
  let at = 0;
  const all = runsOrFirst(runs);
  all.forEach((run, index) => {
    const goesOn = index === all.length - 1;
    for (let bar = 0; goesOn ? at < end : bar < run.bars; bar++) {
      at += ticksOfBar(run.signature);
      starts.push(at);
    }
  });
  return starts;
}

/** The tick the bar `bar` starts at, from 1. */
export function tickOfBar(runs: Runs, bar: number): number {
  let [tick, at] = [0, 1];
  const all = runsOrFirst(runs);
  all.forEach((run, index) => {
    const bars = index === all.length - 1 ? bar - at : Math.min(run.bars, bar - at);
    tick += bars * ticksOfBar(run.signature);
    at += bars;
  });
  return tick;
}

/** From which tick on a beat is how many ticks long: one for each run of bars of one time signature. */
export function beatRuns(runs: Runs): { from: number; ticks: number }[] {
  let from = 0;
  return runsOrFirst(runs).map(({ signature, bars }) => {
    const run = { from, ticks: (4 * BEAT) / (Number(signature.split("/")[1]) || 4) };
    from += bars * ticksOfBar(signature);
    return run;
  });
}

/** The bar `tick` is in, from 1, and where it starts and ends. */
export function barAt(runs: Runs, tick: number): { bar: number; start: number; end: number } {
  const starts = barStarts(runs, tick + 1);
  const next = starts.findIndex((start) => start > tick);
  return { bar: next, start: starts[next - 1] ?? 0, end: starts[next] ?? 0 };
}

/** The bar whose line is nearest `tick`, from 1, and the tick it starts at. */
export function barNear(runs: Runs, tick: number): { bar: number; tick: number } {
  const lines = barStarts(runs, tick + 1);
  const index = lines.reduce((best, start, at) => (Math.abs(start - tick) < Math.abs((lines[best] ?? 0) - tick) ? at : best), 0);
  return { bar: index + 1, tick: lines[index] ?? 0 };
}

/** The grids of the snap in the corner, as the arrangement's menu: `ticks` 0 is off; "Bar" and "Beat" follow the time signature (`stepAt`). */
export const SNAPS = [
  { label: "Off", ticks: 0 },
  { label: "Bar", ticks: BAR },
  { label: "Beat", ticks: BEAT },
  { label: "1/8", ticks: BEAT / 2 },
  { label: "1/16", ticks: BEAT / 4 },
  { label: "1/32", ticks: BEAT / 8 },
] as const;
/** What the snap is when the song opens: sixteenths. */
export const SNAP = BEAT / 4;

/**
 * How far apart the lines of the grid `snap` are in the bar `tick` is in, a sixteenth while it is
 * off. Each bar counts its lines from its own line: "Bar" is the bar, "Beat" the beat of its time signature.
 */
export function stepAt(runs: Runs, tick: number, snap: number): number {
  const { start, end } = barAt(runs, Math.max(tick, 0));
  if (snap === BAR) return end - start;
  if (snap === BEAT) return beatRuns(runs).findLast((run) => run.from <= start)?.ticks ?? BEAT;
  return snap > 0 ? snap : BEAT / 4;
}
/** The line of the grid `snap` at or before `tick`, and the next one: a bar's last cell ends at the next bar's line. */
export function linesAround(runs: Runs, tick: number, snap: number): [number, number] {
  const { start, end } = barAt(runs, Math.max(tick, 0));
  const step = stepAt(runs, start, snap);
  const before = start + Math.floor((Math.max(tick, 0) - start) / step) * step;
  return [before, Math.min(before + step, end)];
}
/** The line of the grid `snap` at or after `tick`. */
export function lineFrom(runs: Runs, tick: number, snap: number): number {
  const [before, after] = linesAround(runs, tick, snap);
  return before === tick ? tick : after;
}
/** The start of the cell of the grid `snap` that `tick` is in, or the tick while it is off: where a double click adds. */
export const cellOf = (runs: Runs, tick: number, snap: number) => (snap > 0 ? linesAround(runs, Math.round(tick), snap)[0] : Math.max(Math.round(tick), 0));
/** The line of the grid `snap` nearest `tick`, or the tick while it is off: where a click plays from. */
export function nearestOf(runs: Runs, tick: number, snap: number): number {
  if (!(snap > 0)) return Math.max(Math.round(tick), 0);
  const [before, after] = linesAround(runs, tick, snap);
  return after - tick <= tick - before ? after : before;
}
/** The lines of the grid `snap` from `from` to before `to`, but the bar lines, each bar's from its own line. */
export function linesIn(runs: Runs, from: number, to: number, snap: number): number[] {
  const starts = barStarts(runs, to);
  return starts.slice(0, -1).flatMap((start, index) => {
    const [end, step] = [starts[index + 1]!, stepAt(runs, start, snap)];
    if (end <= from || start >= to) return [];
    return Array.from({ length: Math.ceil((end - start) / step) - 1 }, (_, at) => start + (at + 1) * step).filter((tick) => tick >= from && tick < to);
  });
}
/** `tick` moved `by` lines of the grid `snap`, as far off the grid as it was; a sixteenth a step while it is off: what a nudge does. */
export function stepped(runs: Runs, tick: number, snap: number, by: number): number {
  if (!(snap > 0)) return tick + by * stepAt(runs, tick, snap);
  let line = nearestOf(runs, tick, snap);
  const off = tick - line;
  // Before tick 0 the grid goes on as the first bar's, so a caller can stop a group there.
  for (let left = Math.abs(by); left > 0; left--) line = by > 0 ? linesAround(runs, line, snap)[1] : line > 0 ? linesAround(runs, line - 1, snap)[0] : line - stepAt(runs, 0, snap);
  return line + off;
}
/**
 * The `snap` and `from` of a drag of what is at `tick`, and at `y` on steps of `down`, so it lands
 * on the lines of the grid `snap` of its bar's time signature. A drag's grid is even, so across a
 * time signature change it keeps these steps.
 */
export function dragGrid(runs: Runs, tick: number, snap: number, [down, y] = [0, 0]): { snap: [number, number]; from: [number, number] } {
  return { snap: [snap > 0 ? stepAt(runs, tick, snap) : 0, down], from: [tick - barAt(runs, Math.max(tick, 0)).start, y] };
}

/** The bar, the beat and the sixteenth, from 1, of `tick` of the song: `[6, 2, 1]`. */
export function placeOf(runs: Runs, tick: number): [bar: number, beat: number, sixteenth: number] {
  const { bar, start } = barAt(runs, tick);
  const beat = beatRuns(runs).findLast((run) => run.from <= tick)?.ticks ?? BEAT;
  const into = tick - start;
  return [bar, Math.floor(into / beat) + 1, Math.floor((into % beat) / (BEAT / 4)) + 1];
}

/** The tick `text` names as `placeOf` says one, `6.2.1`: its beat and sixteenth are 1 when left out. `null` when it names none. */
export function tickOfPlace(runs: Runs, text: string): number | null {
  if (!/^\d+(\.\d+){0,2}$/.test(text.trim())) return null;
  const [bar = 1, beat = 1, sixteenth = 1] = text.trim().split(".").map(Number);
  if (bar < 1 || beat < 1 || sixteenth < 1) return null;
  const line = tickOfBar(runs, bar);
  const ticks = beatRuns(runs).findLast((run) => run.from <= line)?.ticks ?? BEAT;
  return line + (beat - 1) * ticks + ((sixteenth - 1) * BEAT) / 4;
}
