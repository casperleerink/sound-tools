import { positionOf, valueAt, type FilledStateOfTool, type Frozen, type Part } from "./sdk";
import { lengthOf } from "./clip-editor";
import type { parts, Song } from "./song";
import { clamp, ITEMS, lengths, nameFrom, rowsOf, type RowKind } from "./song-page";
import { freeName, splitTarget } from "./track";

// What the song does to its clips and regions: move, copy, cut, paste, duplicate and delete them,
// each with the automation under it, and resize a clip from its start.

/**
 * A clip whose start moved by `dx`: what it holds stays where it is in the song. The start stops
 * at tick 0, at the first note and `unit` before the end; a lane cut short keeps the value it had
 * at the new start, and the pedal what it was there.
 */
export function startMoved(clip: FilledStateOfTool<"timeline">, dx: number, unit: number) {
  const length = lengthOf(clip);
  const by = clamp(dx, -clip.start, Math.min(...clip.notes.map((note) => note.start), length - unit));
  clip.start += by;
  clip.length = length - by;
  for (const note of clip.notes) note.start -= by;
  const pedal = clip.pedal.filter((change) => change.start >= by).map((change) => ({ ...change, start: change.start - by }));
  const held = pedal[0]?.start === 0 ? undefined : clip.pedal.filter((change) => change.start < by).at(-1);
  clip.pedal = held ? [{ ...held, start: 0 }, ...pedal] : pedal;
  for (const lane of ["bend", "mod_wheel", "pressure"] as const) {
    const points = clip[lane];
    const kept = points.filter((point) => point.tick >= by).map((point) => ({ ...point, tick: point.tick - by }));
    const cut = points.some((point) => point.tick < by) && kept[0]?.tick !== 0;
    clip[lane] = cut ? [{ tick: 0, value: Math.round(laneAt(points, by, "linear")) }, ...kept] : kept;
  }
}

export type LanePoint = { tick: number; value: number };
type Scale = FilledStateOfTool<"lane">["scale"];

/** The value of a lane at `tick`, as it plays: straight between its points along its travel, whose ends change nothing of that. */
function laneAt(points: readonly LanePoint[], tick: number, scale: Scale): number {
  const after = points.findIndex((point) => point.tick >= tick);
  if (after <= 0) return (after === 0 ? points[0] : points[points.length - 1])?.value ?? 0;
  const [a, b] = [points[after - 1]!, points[after]!];
  const travel = { min: 1, max: 2, scale };
  const part = b.tick === a.tick ? 1 : (tick - a.tick) / (b.tick - a.tick);
  const [from, to] = [positionOf(travel, a.value), positionOf(travel, b.value)];
  return valueAt(travel, from + (to - from) * part);
}

/** Sorted, one a tick: the last given wins. */
const tidy = (points: readonly LanePoint[]) => [...new Map(points.map((point) => [point.tick, point])).values()].sort((a, b) => a.tick - b.tick);

/**
 * The line of a lane from `start` to `end`, from `start`, with a point on each edge so it is whole
 * wherever it lands. `null` without a point inside: the lane is not the item's there, and a line
 * held elsewhere would move a value nobody put under it.
 */
function lift(points: readonly LanePoint[], scale: Scale, start: number, end: number): LanePoint[] | null {
  const sorted = tidy(points);
  if (!sorted.some(({ tick }) => tick >= start && tick < end)) return null;
  const at = (tick: number) => ({ tick, value: laneAt(sorted, tick, scale) });
  return [at(start), ...sorted.filter(({ tick }) => tick > start && tick < end - 1), at(end - 1)].map(({ tick, value }) => ({ tick: tick - start, value }));
}

/** A lane with nothing from `start` to `end` but a straight line between what plays just outside, by a point on each edge. */
function emptied(points: readonly LanePoint[], scale: Scale, start: number, end: number): LanePoint[] {
  const sorted = tidy(points);
  const at = (tick: number) => ({ tick, value: laneAt(sorted, tick, scale) });
  return tidy([...sorted.filter(({ tick }) => tick < start || tick >= end), ...(start > 0 ? [at(start - 1)] : []), at(end)]);
}

/** A lane with `line` put down from `at` over what it had there. */
function laid(points: readonly LanePoint[], scale: Scale, line: readonly LanePoint[], at: number): LanePoint[] {
  const length = (line.at(-1)?.tick ?? 0) + 1;
  return tidy([...emptied(points, scale, at, at + length), ...line.map(({ tick, value }) => ({ tick: tick + at, value }))]);
}

/** A copy of a record to change, from a view's or a key's. */
const copyOf = <T,>(state: Frozen<T>) => JSON.parse(JSON.stringify(state)) as T;

/**
 * A clip or a region as it was taken: its row of the timeline and its track, its name, its
 * record, how long it is on the timeline, and the line of each lane of its track under it.
 */
type Taken = {
  row: number;
  track: string;
  name: string;
  item: { tool: "timeline"; state: FilledStateOfTool<"timeline"> } | { tool: "region"; state: FilledStateOfTool<"region"> };
  length: number;
  lines: { target: string; scale: Scale; points: LanePoint[] }[];
};
/** What cut and copy keep: the clips and the regions, their starts from the earliest and their rows from the top. */
export type Clipboard = Taken[];

/** The clips and the regions `keys` names, `row/name`, with the automation under each. */
export function take(children: Song["children"], rows: readonly RowKind[], keys: Iterable<string>, length: (key: string) => number): Taken[] {
  return [...keys].flatMap((key) => {
    const [track = "", name = ""] = key.split("/");
    const owner = children.find((child) => child.name === track);
    if (owner?.tool !== "track" && owner?.tool !== "tape") return [];
    const child = owner.children.find((each) => each.name === name);
    const item = child?.tool === "timeline" ? { tool: "timeline" as const, state: copyOf<FilledStateOfTool<"timeline">>(child.state) }
      : child?.tool === "region" ? { tool: "region" as const, state: copyOf<FilledStateOfTool<"region">>(child.state) } : null;
    if (!item) return [];
    const [start, end] = [item.state.start, item.state.start + length(key)];
    const lines = owner.state.automation.flatMap(({ lane, target }) => {
      const points = owner.children.find((each) => each.name === lane);
      const line = points?.tool === "lane" ? lift(points.state.points, points.state.scale, start, end) : null;
      return line && points?.tool === "lane" ? [{ target, scale: points.state.scale, points: line }] : [];
    });
    return [{ row: rows.findIndex((row) => row.name === track), track, name, item, length: end - start, lines }];
  });
}

/** `taken` as the clipboard keeps it: from the earliest start and the top row. */
function clipboardOf(taken: readonly Taken[]): Clipboard | null {
  if (taken.length === 0) return null;
  const [from, top] = [Math.min(...taken.map((each) => each.item.state.start)), Math.min(...taken.map((each) => each.row))];
  return taken.map((each) => ({ ...each, row: each.row - top, item: { ...each.item, state: { ...each.item.state, start: each.item.state.start - from } } } as Taken));
}

/** Takes `taken` out of the song, and the lines each took that `goes` says go along: each lane straight between the edges of where it was. */
function removeTaken({ children }: Part<unknown, typeof parts>, taken: readonly Taken[], goes: (each: Taken, target: string) => boolean) {
  for (const each of taken) {
    const { track, name, item, length, lines } = each;
    const owner = children.find((child) => child.name === track);
    if (owner?.tool === "track") owner.children = owner.children.filter((child) => child.name !== name);
    else if (owner?.tool === "tape") owner.children = owner.children.filter((child) => child.name !== name);
    else continue;
    for (const { lane, target } of owner.state.automation) {
      const points = owner.children.find((child) => child.name === lane);
      if (points?.tool === "lane" && goes(each, target) && lines.some((line) => line.target === target)) {
        points.state.points = emptied(points.state.points, points.state.scale, item.state.start, item.state.start + length);
      }
    }
  }
}

/**
 * Where each of `taken` lands: on the row and at the start `where` gives, a row past the last on
 * the last. `null` when one would land on a row that holds the other kind: a clip goes on a
 * track, a region on an audio track.
 */
export function placing(rows: readonly RowKind[], taken: readonly Taken[], where: (each: Taken) => { row: number; start: number }) {
  const placed = taken.map((each) => {
    const { row, start } = where(each);
    return { each, row: rows[clamp(row, 0, rows.length - 1)]!, start: Math.max(Math.round(start), 0) };
  });
  return placed.every(({ each, row }) => row && ITEMS[row.tool] === each.item.tool) ? placed : null;
}
type Placed = NonNullable<ReturnType<typeof placing>>;

/**
 * Puts copies of what `placed` places in the song, with the lines they took over what each lane
 * had there: a volume or a pan on any row, a lane of a device
 * on its own track only, as the device is the track's. A row with no lane for a volume or a pan
 * gets one, at its fader around what lands. The keys of the copies.
 */
export function putTaken({ children }: Part<unknown, typeof parts>, placed: Placed): Set<string> {
  const keys = new Set<string>();
  for (const { each, row, start } of placed) {
    const owner = children.find((child) => child.name === row.name);
    if (owner?.tool !== "track" && owner?.tool !== "tape") continue;
    const name = freeName(owner.children.map((child) => child.name), each.name);
    if (owner.tool === "track" && each.item.tool === "timeline") owner.children.push({ name, tool: "timeline", state: { ...each.item.state, start } });
    else if (owner.tool === "tape" && each.item.tool === "region") owner.children.push({ name, tool: "region", state: { ...each.item.state, start } });
    keys.add(`${owner.name}/${name}`);
    for (const line of each.lines) {
      const fader = line.target.startsWith("mix.");
      if (owner.name !== each.track && !fader) continue;
      const lane = owner.state.automation.find(({ target }) => target === line.target);
      const points = owner.children.find((child) => child.name === lane?.lane);
      if (points?.tool === "lane") points.state.points = laid(points.state.points, points.state.scale, line.points, start);
      else if (!lane && fader) {
        const mix = owner.children.find((child) => child.name === "mix");
        const record = mix?.tool === "fader" ? mix.state[splitTarget(line.target)[1] as "gain_db" | "pan"] : undefined;
        const value = typeof record === "number" ? record : record === "-inf" ? -100 : 0;
        const added = freeName(owner.children.map((child) => child.name), `lane-${nameFrom(line.target)}`) as `lane-${string}`;
        owner.children.push({ name: added, tool: "lane", state: { points: laid([{ tick: 0, value }], line.scale, line.points, start), scale: line.scale } });
        owner.state.automation = [...owner.state.automation, { lane: added, target: line.target }];
      }
    }
  }
  return keys;
}

/**
 * Moves the clips and the regions `keys` names by `dx` ticks, each onto the row `to` gives for
 * its own, or onto its own where that row holds the other kind, or copies them there, with the
 * automation under them: as one undo step. The keys of where they are after.
 */
export function moveItems(tool: Pick<Song, "children" | "update" | "outputs" | "clock">, order: readonly string[], keys: ReadonlySet<string>, dx: number, to: (row: number) => number, copy: boolean): Set<string> {
  const rows = rowsOf(tool.children, order);
  const taken = take(tool.children, rows, keys, lengths(tool));
  const rowOf = new Map(taken.map((each) => {
    const row = to(each.row);
    return [each, ITEMS[rows[row]?.tool ?? ""] === each.item.tool ? row : each.row];
  }));
  // A volume and a pan go along to any track, a lane of a device only on its own.
  const goes = (each: Taken, target: string) => target.startsWith("mix.") || rows[rowOf.get(each) ?? -1]?.name === each.track;
  const placed = placing(rows, taken, (each) => ({ row: rowOf.get(each) ?? each.row, start: each.item.state.start + dx }));
  if (!placed) return new Set(keys);
  let after = new Set(keys);
  tool.update(copy ? "Copy clips" : "Move clips", (part) => {
    if (!copy) removeTaken(part, taken, goes);
    after = putTaken(part, placed);
  });
  return after;
}
/** What the keys and a clip's menu do to the chosen clips. */
export type ClipCommand = "cut" | "copy" | "duplicate" | "delete";

/** Cut, copy, duplicate or delete the chosen clips and regions, each as one undo step: the copies of a duplicate are chosen after. */
export function itemCommand(command: ClipCommand, order: readonly string[], tool: Pick<Song, "memory" | "update" | "select" | "children" | "outputs" | "clock">) {
  const { memory, update, select, children } = tool;
  const rows = rowsOf(children, order);
  const taken = take(children, rows, memory.selected, lengths(tool));
  if (taken.length === 0) return;
  if (command === "cut" || command === "copy") memory.clipboard = clipboardOf(taken);
  if (command === "duplicate") {
    // Right after them, on their rows; the clipboard keeps what it had.
    const span = Math.max(...taken.map((each) => each.item.state.start + each.length)) - Math.min(...taken.map((each) => each.item.state.start));
    const placed = placing(rows, taken, (each) => ({ row: each.row, start: each.item.state.start + span }));
    if (!placed) return;
    let copies = new Set<string>();
    update("Duplicate clips", (part) => (copies = putTaken(part, placed)));
    memory.selected = copies;
  } else if (command !== "copy") {
    // A cut takes the automation under them along, as a move does; a delete leaves it.
    update(command === "cut" ? "Cut clips" : "Remove clips", (part) => removeTaken(part, taken, () => command === "cut"));
    memory.selected = new Set();
    select(null);
  }
}
