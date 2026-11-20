// The automation of a track: lanes under it, each a line of points, an LFO or an envelope that
// moves a number of the track's fader or of one of its devices over the song.
// A lane's plane: x in ticks of the song, y down its travel, 0 its top (`max`) and 1 its bottom.
import { builtInNumbers, type FilledStateOfTool, type Pointer } from "../sdk";
import { envelope, lfo } from "../lfo";
import { freeName, splitTarget, type Mover } from "../track";
import { isRow, nameFrom, type Child, type Draft, type Song } from "./model";
import { nearestOf } from "./time";

export type AutomationPoint = FilledStateOfTool<"lane">["points"][number];
type Scale = FilledStateOfTool<"lane">["scale"];

/** How a number spreads over the height of its lane, as its knob or fader travels it. */
export type Travel = { min: number; max: number; scale: Scale };

/** The fader's scale: 0 dB at 0.8 of the travel and +6 dB at the top, a power law on the amplitude. */
const FADER = { unity: 0.8, decade: 61.94, top: 6 } as const;

/** Where `value` sits along `travel`: 0 at `min` and 1 at `max`. A fader's is a volume in dB whatever its ends. */
export function positionOf({ min, max, scale }: Travel, value: number): number {
  if (scale === "log") return Math.log(value / min) / Math.log(max / min);
  if (scale === "fader") return Math.min(FADER.unity * 10 ** (value / FADER.decade), 1);
  return (value - min) / (max - min);
}
/** The value at `position` along `travel`: a fader gives -Infinity at 0. */
export function valueAt({ min, max, scale }: Travel, position: number): number {
  if (scale === "log") return min * (max / min) ** position;
  if (scale === "fader") return position <= 0 ? -Infinity : Math.min(FADER.decade * Math.log10(position / FADER.unity), FADER.top);
  return min + position * (max - min);
}
const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
/** The value a lane holds `up` its travel, 0 to 1: a lane holds no -inf, so a volume's bottom is -100 dB. */
export const laneValue = (travel: Travel, up: number) => {
  const value = valueAt(travel, clamp(up, 0, 1));
  return travel.scale === "fader" ? Math.max(value, -100) : value;
};

/** How far along a line bent by `curve` the place `done` of the way is, as the lane plays it. */
export const bent = (done: number, curve: number) => (curve === 0 ? done : done ** (4 ** curve));

/** The value of a lane at `tick`, as it plays: along its travel between its points, bent by the curve of the one before. */
export function laneAt(points: readonly { tick: number; value: number; curve?: number }[], tick: number, scale: Scale): number {
  const after = points.findIndex((point) => point.tick >= tick);
  if (after <= 0) return (after === 0 ? points[0] : points[points.length - 1])?.value ?? 0;
  const [a, b] = [points[after - 1]!, points[after]!];
  const travel = { min: 1, max: 2, scale };
  const part = b.tick === a.tick ? 1 : bent((tick - a.tick) / (b.tick - a.tick), a.curve ?? 0);
  const [from, to] = [positionOf(travel, a.value), positionOf(travel, b.value)];
  return valueAt(travel, from + (to - from) * part);
}

/** Sorted, one a tick: the last given wins. */
export const tidy = (points: readonly AutomationPoint[]) => [...new Map(points.map((point) => [point.tick, point])).values()].sort((a, b) => a.tick - b.tick);

/** A point at `tick` on the line of `sorted`, with the curve of where it lands: a cut line keeps about its shape. */
function pointAt(sorted: readonly AutomationPoint[], scale: Scale, tick: number): AutomationPoint {
  const curve = sorted.findLast((point) => point.tick <= tick)?.curve;
  return { tick, value: laneAt(sorted, tick, scale), ...(curve ? { curve } : {}) };
}

/**
 * The line of a lane from `start` to `end`, from `start`, with a point on each edge so it is whole
 * wherever it lands. `null` without a point inside: the lane is not the item's there.
 */
export function lift(points: readonly AutomationPoint[], scale: Scale, start: number, end: number): AutomationPoint[] | null {
  const sorted = tidy(points);
  if (!sorted.some(({ tick }) => tick >= start && tick < end)) return null;
  const at = (tick: number) => pointAt(sorted, scale, tick);
  return [at(start), ...sorted.filter(({ tick }) => tick > start && tick < end - 1), at(end - 1)].map((point) => ({ ...point, tick: point.tick - start }));
}

/** A lane with nothing from `start` to `end` but a straight line between what plays just outside, by a point on each edge. */
export function emptied(points: readonly AutomationPoint[], scale: Scale, start: number, end: number): AutomationPoint[] {
  const sorted = tidy(points);
  const before = start > 0 ? [{ tick: start - 1, value: laneAt(sorted, start - 1, scale) }] : [];
  return tidy([...sorted.filter(({ tick }) => tick < start || tick >= end), ...before, pointAt(sorted, scale, end)]);
}

/** A lane with `line` put down from `at` over what it had there. */
export function laid(points: readonly AutomationPoint[], scale: Scale, line: readonly AutomationPoint[], at: number): AutomationPoint[] {
  const length = (line.at(-1)?.tick ?? 0) + 1;
  return tidy([...emptied(points, scale, at, at + length), ...line.map((point) => ({ ...point, tick: point.tick + at }))]);
}

/** The travel of the number `target` a lane of `row` moves: a fader's volume, a pan, or a built-in device's number; else what its points span. */
export function travelOf(row: Child, target: string): Travel {
  const [path, number] = splitTarget(target);
  if (path === "mix" || path.startsWith("send-")) return number === "gain_db" ? { min: -100, max: 6, scale: "fader" } : { min: -1, max: 1, scale: "linear" };
  let device: { tool: unknown; children: readonly unknown[] } | undefined = isRow(row) ? row : undefined;
  for (const step of path.split("/")) device = (device?.children as { name: string; tool: unknown; children: readonly unknown[] }[] | undefined)?.find((child) => child.name === step);
  const numbers = builtInNumbers[String(device?.tool)] ?? {};
  const known = numbers[number] ?? numbers[number.replace(/\.\d+\./, ".*.")];
  if (known?.min !== undefined && known.max !== undefined) return { min: known.min, max: known.max, scale: known.scale };
  const lane = isRow(row) ? row.children.find((child) => child.tool === "lane" && row.state.automation.some((each) => each.lane === child.name && each.target === target)) : undefined;
  const values = lane?.tool === "lane" ? lane.state.points.map((point) => point.value) : [];
  return { min: Math.min(0, ...values), max: Math.max(1, ...values), scale: "linear" };
}

/** The track or audio track `name` of a draft. */
const rowIn = (draft: Draft, name: string) => {
  const owner = draft.children.find((child) => child.name === name);
  return owner?.tool === "track" || owner?.tool === "tape" ? owner : undefined;
};

/** Changes the points of the lane `lane` of `track` as one undo step `label`, sorted, one a tick; with none left, the lane goes. */
export const editPoints = ({ update }: Pick<Song, "update">, label: string, track: string, lane: string, change: (points: AutomationPoint[]) => AutomationPoint[]) =>
  update(label, (draft) => {
    const owner = rowIn(draft, track);
    const child = owner?.children.find((each) => each.name === lane);
    if (!owner || child?.tool !== "lane") return;
    const points = tidy(change(child.state.points.map((point) => ({ ...point }))));
    if (points.length > 0) child.state.points = points;
    else {
      owner.children.splice(owner.children.indexOf(child), 1);
      owner.state.automation = owner.state.automation.filter((each) => each.lane !== lane);
    }
  });

/** The words of what moves a lane's number, as its menu names them: an envelope on a track only, which has notes. */
export const MOVED_BY: readonly (readonly [Mover, string])[] = [["lane", "Points"], ["lfo", "LFO"], ["envelope", "Envelope"]];
/** The first letters of a lane child's name, after what moves it. */
const PREFIX: Record<Mover, string> = { lane: "lane", lfo: "lfo", envelope: "env" };

/** What a lane moves, as a person reads it: `Volume`, `Pan`, or `Device · Number`. `device` is the device's title, its name in words when left out. */
export function laneName(target: string, device?: string, number?: string): string {
  const [path, field] = splitTarget(target);
  const said = number ?? field.replace(/_(hz|db|ms|seconds|cents|octaves|semitones)$/, "").replace(/[._-]+/g, " ").replace(/^./, (first) => first.toUpperCase());
  if (path === "mix") return field === "gain_db" ? "Volume" : said;
  const named = device ?? (path.split("/").at(-1) ?? path).replace(/[._-]+/g, " ").replace(/^./, (first) => first.toUpperCase());
  return `${named} · ${said}`;
}

/** What a view does to the lanes of the song: each call one undo step. `p` is in the lane's plane. */
export const laneEdits = (song: Song) => {
  const { update, memory } = song;
  const rowNamed = (name: string) => song.children.find((child) => child.name === name);
  const pointsOf = (track: string, lane: string) => {
    const owner = rowNamed(track);
    const child = owner && isRow(owner) ? owner.children.find((each) => each.name === lane) : undefined;
    return child?.tool === "lane" ? child.state.points : [];
  };
  const targetOf = (track: string, lane: string) => {
    const owner = rowNamed(track);
    return owner && isRow(owner) ? owner.state.automation.find((each) => each.lane === lane)?.target : undefined;
  };
  const travel = (track: string, lane: string) => {
    const owner = rowNamed(track);
    const target = targetOf(track, lane);
    return owner && target ? travelOf(owner, target) : { min: 0, max: 1, scale: "linear" as const };
  };
  const choose = (track: string, lane: string, tick: number) => {
    memory.selected = [];
    memory.mark = null;
    memory.point = { track, lane, tick };
  };
  /** The ticks the point at `tick` may move to: it does not pass its neighbours. */
  const between = (track: string, lane: string, tick: number) => {
    const points = pointsOf(track, lane);
    const index = points.findIndex((point) => point.tick === tick);
    return [(points[index - 1]?.tick ?? -1) + 1, (points[index + 1]?.tick ?? 1e12) - 1] as const;
  };
  return {
    /** Shows or hides the lanes of `track`. */
    show(track: string, shown: boolean) {
      memory.lanes = shown ? [...new Set([...memory.lanes, track])] : memory.lanes.filter((each) => each !== track);
    },
    /** A lane for `target` of `track`, such as `mix.gain_db` or `chain/filter.cutoff_hz`, starting where the number is now, `value`: the lanes then show. */
    add(track: string, target: string, value: number) {
      const owner = rowNamed(track);
      if (!owner || !isRow(owner) || owner.state.automation.some((each) => each.target === target)) return;
      const { scale } = travelOf(owner, target);
      update("Add a lane", (draft) => {
        const row = rowIn(draft, track);
        if (!row) return;
        const lane = freeName(row.children.map((child) => child.name), `lane-${nameFrom(target)}`) as `lane-${string}`;
        row.children.push({ name: lane, tool: "lane", state: { points: [{ tick: 0, value: Math.max(value, -100) }], scale } });
        row.state.automation = [...row.state.automation, { lane, target }];
      });
      this.show(track, true);
    },
    remove(track: string, lane: string) {
      update("Remove a lane", (draft) => {
        const row = rowIn(draft, track);
        if (!row) return;
        row.state.automation = row.state.automation.filter((each) => each.lane !== lane);
        const at = row.children.findIndex((child) => child.name === lane);
        if (at >= 0) row.children.splice(at, 1);
      });
    },
    /**
     * Moves the number of `lane` with `tool` instead: points that hold what it played at the
     * start, or an LFO or an envelope between a quarter and three quarters of the number's travel.
     */
    moveWith(track: string, lane: string, tool: Mover) {
      const range = travel(track, lane);
      const scale = range.scale === "log" ? "log" : "linear";
      const [low, high] = [laneValue(range, 0.25), laneValue(range, 0.75)];
      update(tool === "lane" ? "Move with points" : tool === "lfo" ? "Move with an LFO" : "Move with an envelope", (draft) => {
        const row = rowIn(draft, track);
        const target = row?.state.automation.find((each) => each.lane === lane)?.target;
        const old = row?.children.find((child) => child.name === lane);
        if (!row || !target || !old || old.tool === tool || (tool === "envelope" && row.tool !== "track")) return;
        const held = old.tool === "lane" ? (old.state.points[0]?.value ?? low) : old.tool === "lfo" || old.tool === "envelope" ? old.state.low : low;
        row.children.splice(row.children.findIndex((child) => child.name === lane), 1);
        const name = freeName(row.children.map((child) => child.name), `${PREFIX[tool]}-${nameFrom(target)}`);
        if (tool === "lane") row.children.push({ name: name as `lane-${string}`, tool: "lane", state: { points: [{ tick: 0, value: held }], scale: range.scale } });
        else if (tool === "lfo") row.children.push({ name: name as `lfo-${string}`, tool: lfo, state: { low, high, scale } });
        else {
          const owner = draft.children.find((child) => child.name === track);
          if (owner?.tool === "track") owner.children.push({ name: name as `env-${string}`, tool: envelope, state: { low, high, scale } });
        }
        row.state.automation = row.state.automation.map((each) => (each.lane === lane ? { ...each, lane: name } : each));
      });
    },
    /** A press on an empty lane: a point there, on the snap unless cmd is held, chosen. Its drag then moves it with `move(…, true)`. */
    addPoint(track: string, lane: string, p: Pick<Pointer, "x" | "y" | "cmd">) {
      const tick = p.cmd ? Math.max(Math.round(p.x), 0) : nearestOf(p.x, memory.snap);
      editPoints(song, "Add a point", track, lane, (all) => [...all, { tick, value: laneValue(travel(track, lane), 1 - p.y) }]);
      choose(track, lane, tick);
      return tick;
    },
    /** The point at `tick` dragged by `p`, between its neighbours, chosen; `added` when the same press added it, so both are one undo step. */
    movePoint(track: string, lane: string, tick: number, p: Pick<Pointer, "dx" | "dy">, added = false) {
      const point = pointsOf(track, lane).find((each) => each.tick === tick);
      if (!point) return;
      const range = travel(track, lane);
      const [earliest, latest] = between(track, lane, tick);
      const to = clamp(tick + Math.round(p.dx), earliest, latest);
      const value = laneValue(range, positionOf(range, point.value) - p.dy);
      editPoints(song, added ? "Add a point" : "Move a point", track, lane, (all) => all.map((each) => (each.tick === tick ? { ...each, tick: to, value } : each)));
      choose(track, lane, to);
    },
    /** An alt-drag on the lane: the points it covers go. */
    erase(track: string, lane: string, p: Pick<Pointer, "x" | "dx">) {
      const [from, to] = [Math.min(p.x, p.x + p.dx), Math.max(p.x, p.x + p.dx)];
      editPoints(song, "Erase points", track, lane, (all) => all.filter((point) => point.tick < from || point.tick > to));
    },
    /** An alt-click on a point, or delete on the chosen one: it goes, and the last point takes the lane along. */
    removePoint(track: string, lane: string, tick: number) {
      if (memory.point?.track === track && memory.point.lane === lane && memory.point.tick === tick) memory.point = null;
      editPoints(song, "Remove a point", track, lane, (all) => all.filter((point) => point.tick !== tick));
    },
    /** The handle in the middle of the line from the point at `tick` dragged by `dy`: the curve whose middle is where it was let go. */
    bend(track: string, lane: string, tick: number, p: Pick<Pointer, "dy">) {
      const points = pointsOf(track, lane);
      const index = points.findIndex((point) => point.tick === tick);
      const [point, after] = [points[index], points[index + 1]];
      if (!point || !after) return;
      const range = travel(track, lane);
      const [from, to] = [positionOf(range, point.value), positionOf(range, after.value)];
      const middle = from + (to - from) * bent(0.5, point.curve ?? 0) - p.dy;
      const done = clamp((middle - from) / (to - from), 0.01, 0.99);
      const curve = clamp(Math.log(Math.log(done) / Math.log(0.5)) / Math.log(4), -1, 1);
      editPoints(song, "Bend a line", track, lane, (all) => all.map((each) => (each.tick === tick ? { ...each, curve } : each)));
    },
    /** A double click on the handle: the line is straight again. */
    straighten(track: string, lane: string, tick: number) {
      editPoints(song, "Straighten a line", track, lane, (all) => all.map((each) => (each.tick === tick ? { tick: each.tick, value: each.value } : each)));
    },
    /** The low or the high line of an LFO's or an envelope's band dragged by `dy`. */
    band(track: string, lane: string, end: "low" | "high", p: Pick<Pointer, "dy">) {
      const range = travel(track, lane);
      update(end === "low" ? "Change the low" : "Change the high", (draft) => {
        const child = rowIn(draft, track)?.children.find((each) => each.name === lane);
        if (child?.tool === "lfo" || child?.tool === "envelope") child.state[end] = laneValue(range, positionOf(range, child.state[end]) - p.dy);
      });
    },
  };
};
