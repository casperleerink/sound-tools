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

/** The grids of the snap in the corner, as the arrangement's menu: `ticks` 0 is off. */
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

/** The start of the cell of the grid `snap` that `tick` is in, or the tick while it is off: where a double click adds. */
export const cellOf = (tick: number, snap: number) => Math.max(snap > 0 ? Math.floor(Math.round(tick) / snap) * snap : Math.round(tick), 0);
/** The line of the grid `snap` nearest `tick`, or the tick while it is off: where a click plays from. */
export const nearestOf = (tick: number, snap: number) => Math.max(snap > 0 ? Math.round(tick / snap) * snap : Math.round(tick), 0);
/** What a nudge moves by: the snap, a sixteenth while it is off. */
export const stepOf = (snap: number) => snap || BEAT / 4;

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
