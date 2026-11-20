// The automation of a track: lanes under it, each a line of points, an LFO or an envelope that
// moves a number of the track's fader or of one of its devices over the song.
// A lane's plane: x in ticks of the song, y down its travel, 0 its top (`max`) and 1 its bottom.
import { type Facts, type FieldInfo, type FilledStateOfTool, type Pointer, type Unit } from "../sdk";
import { ENVELOPE, envelope, lfo } from "../lfo";
import { freeName, splitTarget, type Mover } from "../track";
import { isRow, mixOf, nameFrom, words, type Child, type Draft, type Song } from "./model";
import { positionOf, SILENT_DB, valueAt, valueOf, VOLUME } from "../ui/value";
import { nearestOf } from "./time";

export type AutomationPoint = FilledStateOfTool<"lane">["points"][number];
type Scale = FilledStateOfTool<"lane">["scale"];

/** How a number spreads over the height of its lane, as its knob or fader travels it. */
export type Travel = { min: number; max: number; scale: Scale };

const clamp = (value: number, low: number, high: number) => Math.min(Math.max(value, low), high);
/** The value a lane holds `up` its travel, 0 to 1, as its fader or knob sets it there. */
export const laneValue = (travel: Travel, up: number) => valueAt(up, travel);

/** How far along a line bent by `curve` the place `done` of the way is, as the lane plays it. */
export const bent = (done: number, curve: number) => (curve === 0 ? done : done ** (4 ** curve));

/** The value of a lane at `tick`, as it plays: along its travel between its points, bent by the curve of the one before. */
export function laneAt(points: readonly { tick: number; value: number; curve?: number }[], tick: number, scale: Scale): number {
  const after = points.findIndex((point) => point.tick >= tick);
  if (after <= 0) return (after === 0 ? points[0] : points[points.length - 1])?.value ?? 0;
  const [a, b] = [points[after - 1]!, points[after]!];
  const travel = { min: 1, max: 2, scale };
  const part = b.tick === a.tick ? 1 : bent((tick - a.tick) / (b.tick - a.tick), a.curve ?? 0);
  const [from, to] = [positionOf(a.value, travel), positionOf(b.value, travel)];
  return valueOf(from + (to - from) * part, travel);
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
  if (path === "mix" || path.startsWith("send-")) return number === "gain_db" ? { min: SILENT_DB, max: VOLUME.max, scale: "fader" } : { min: -1, max: 1, scale: "linear" };
  let device: Device | undefined = isRow(row) ? (row as Device) : undefined;
  for (const step of path.split("/")) device = device?.children.find((child) => child.name === step);
  const known = device && numbersOf(device).find((each) => each.number === number);
  if (known?.min !== undefined && known.max !== undefined) return { min: known.min, max: known.max, scale: known.scale };
  const lane = isRow(row) ? row.children.find((child) => child.tool === "lane" && row.state.automation.some((each) => each.lane === child.name && each.target === target)) : undefined;
  const values = lane?.tool === "lane" ? lane.state.points.map((point) => point.value) : [];
  return { min: Math.min(0, ...values), max: Math.max(1, ...values), scale: "linear" };
}

/** The unit a lane's number shows in, by its name as the window's cards tell it: a 0 to 1 number is a share. */
export function unitOf(target: string, { min, max, scale }: Travel): Unit | undefined {
  const [, number] = splitTarget(target);
  if (scale === "fader") return "db";
  const named = /_(hz|db|ms|seconds|percent)$/.exec(number)?.[1] as Unit | undefined;
  if (named) return named;
  if (number.split(".").at(-1) === "pan") return "pan";
  return min === 0 && max === 1 ? "percent" : undefined;
}

/** A number a new lane of a track can move: what it is, what the menu calls it, the device it is of, and where it is now. */
export type Movable = { target: string; text: string; group: string; value: number };

/** A device as a track holds it, and its facts as the window told them. */
type Device = { name: string; tool: unknown; state: unknown; children: readonly Device[]; facts?: Facts & Partial<Pick<Facts<"plugin">, "parameters">> };
/** A number of a device a lane can move, by its path in the record, and how far it goes. */
type Known = { number: string; min?: number; max?: number; scale: Scale; default?: number };

/** The numbers of `device` a lane can move, as its facts say: its fields' and a plugin's parameters. */
function numbersOf(device: Device): Known[] {
  const facts = device.facts;
  const parameters = (facts?.parameters ?? []).filter((parameter) => parameter.lane)
    .map((parameter): Known => ({ number: `parameters.${parameter.id}.value`, min: parameter.min, max: parameter.max, scale: "linear", default: parameter.default }));
  return [...fieldNumbers(facts?.fields ?? [], device.state, ""), ...parameters];
}

/** The numbers among `fields` of `state` a lane can move, by their paths after `prefix`. */
function fieldNumbers(fields: readonly FieldInfo[], state: unknown, prefix: string): Known[] {
  return fields.flatMap((field) => itemNumbers(field, (state as Record<string, unknown> | undefined)?.[field.name], `${prefix}${field.name}`));
}
function itemNumbers(field: FieldInfo, value: unknown, path: string): Known[] {
  switch (field.kind) {
    case "number":
      return field.lane ? [{ number: path, min: field.min, max: field.max, scale: field.scale, default: field.default }] : [];
    case "group":
      return fieldNumbers(field.fields, value, `${path}.`);
    case "list":
    case "map":
      return Object.entries((value as object | undefined) ?? {}).flatMap(([key, item]) => itemNumbers(field.item, item, `${path}.${key}`));
    default:
      return [];
  }
}

const numberIn = (state: unknown, path: string): unknown => path.split(".").reduce<unknown>((at, step) => (at as Record<string, unknown> | undefined)?.[step], state);

/**
 * What a new lane of `row` can move, a group a device by its title: the track's volume and pan, its
 * instrument's numbers, then each effect's in the chain's order; those a lane moves already left
 * out.
 */
export function movablesOf(row: Child): Movable[] {
  if (!isRow(row)) return [];
  const gain = mixOf(row)?.state.gain_db;
  const own: Movable[] = [
    { target: "mix.gain_db", text: "Volume", group: "Track", value: gain === "-inf" ? SILENT_DB : (gain ?? 0) },
    { target: "mix.pan", text: "Pan", group: "Track", value: mixOf(row)?.state.pan ?? 0 },
  ];
  const chain = row.children.find((child) => child.name === "chain");
  const effects = chain?.tool === "chain" ? chain.state.order.flatMap((name) => chain.children.filter((each) => each.name === name).map((each) => ({ path: `chain/${name}`, device: each }))) : [];
  const synth = row.children.find((child) => child.name === "synth");
  const devices = [...(synth ? [{ path: "synth", device: synth }] : []), ...effects];
  const parameters = (device: Device) => new Map((device.facts?.parameters ?? []).map((parameter) => [`parameters.${parameter.id}.value`, parameter.name]));
  const numbers = devices.flatMap(({ path, device }) =>
    numbersOf(device as Device).map((known) => {
      const number = known.number;
      const target = `${path}.${number}`;
      const held = numberIn(device.state, number);
      const value = typeof held === "number" ? held : (known.default ?? laneValue(travelOf(row, target), 0.5));
      // An item of a list counts from 1, a key of a map is as it is: `bands.0.gain_db` is `Bands 1 gain`.
      const parts = number.split(".").map((part, at, all) => (/^\d+$/.test(part) && Array.isArray(numberIn(device.state, all.slice(0, at).join("."))) ? String(Number(part) + 1) : part));
      return { target, text: parameters(device as Device).get(number) ?? words(parts.join(" ")), group: (device as Device).facts?.title ?? words(device.name), value };
    }),
  );
  const taken = new Set(row.state.automation.map((each) => each.target));
  return [...own, ...numbers].filter((each) => !taken.has(each.target));
}

/** How far the points at `ticks` of `points` may move in time: one alone stays between its neighbours, many stay after 0. */
export function roomOf(points: readonly { tick: number }[], ticks: readonly number[]): [earliest: number, latest: number] {
  if (ticks.length !== 1) return [-Math.min(...ticks), FAR];
  const index = points.findIndex((point) => point.tick === ticks[0]);
  const tick = points[index]?.tick;
  if (tick === undefined) return [0, 0];
  return [(points[index - 1]?.tick ?? -1) + 1 - tick, (points[index + 1]?.tick ?? FAR) - 1 - tick];
}
/** Further than any song goes, in ticks: a bound a drag can be given. */
const FAR = 1e12;

/** The track or audio track `name` of a draft. */
const rowIn = (draft: Draft, name: string) => {
  const owner = draft.children.find((child) => child.name === name);
  return owner?.tool === "track" || owner?.tool === "tape" ? owner : undefined;
};

/** The automation of `row` that moves the numbers of its child `path`, such as `chain/eq` or `send-ret-a`, or of what that child holds. */
export const lanesInto = <A extends { target: string }>(row: { state: { automation: readonly A[] } }, path: string) =>
  row.state.automation.filter(({ target }) => `${splitTarget(target)[0]}/`.startsWith(`${path}/`));
/** Takes `lanesInto(row, path)` out of `row`, with the lanes, LFOs and envelopes that moved nothing else. */
export function dropLanesOf(row: NonNullable<ReturnType<typeof rowIn>>, path: string) {
  const gone = lanesInto(row, path);
  row.state.automation = row.state.automation.filter((each) => !gone.includes(each));
  for (const { lane } of gone) {
    const at = row.children.findIndex((child) => child.name === lane);
    if (at >= 0 && !row.state.automation.some((each) => each.lane === lane)) row.children.splice(at, 1);
  }
}

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
  const choose = (track: string, lane: string, ticks: number[]) => {
    memory.selected = [];
    memory.mark = null;
    memory.point = { track, lane, ticks };
  };
  const chosenIn = (track: string, lane: string) => (memory.point?.track === track && memory.point.lane === lane ? memory.point.ticks : []);
  /** The points at `ticks` moved by `dx` ticks and `dy` down the travel, as one step, and chosen. */
  const shift = (track: string, lane: string, ticks: readonly number[], dx: number, dy: number) => {
    const points = pointsOf(track, lane);
    const [earliest, latest] = roomOf(points, ticks);
    const by = clamp(Math.round(dx), earliest, latest);
    const range = travel(track, lane);
    const moved = points.filter((point) => ticks.includes(point.tick)).map((point) => ({ ...point, tick: point.tick + by, value: laneValue(range, positionOf(point.value, range) - dy) }));
    if (moved.length === 0) return false;
    // The moved points last: one that lands on another takes its place.
    editPoints(song, moved.length > 1 ? "Move points" : "Move a point", track, lane, (all) => [...all.filter((point) => !ticks.includes(point.tick)), ...moved]);
    choose(track, lane, moved.map((point) => point.tick));
    return true;
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
        row.children.push({ name: lane, tool: "lane", state: { points: [{ tick: 0, value: Math.max(value, SILENT_DB) }], scale } });
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
        else if (tool === "lfo") row.children.push({ name: name as `lfo-${string}`, tool: lfo, state: { shape: "sine", rate: "1/4", low, high, scale } });
        else {
          const owner = draft.children.find((child) => child.name === track);
          const times = Object.fromEntries(Object.entries(ENVELOPE).map(([field, spec]) => [field, spec.default])) as Record<keyof typeof ENVELOPE, number>;
          if (owner?.tool === "track") owner.children.push({ name: name as `env-${string}`, tool: envelope, state: { ...times, low, high, scale } });
        }
        row.state.automation = row.state.automation.map((each) => (each.lane === lane ? { ...each, lane: name } : each));
      });
    },
    /** A double click on a lane: a point there, on the snap unless cmd is held, chosen. */
    addPoint(track: string, lane: string, p: Pick<Pointer, "x" | "y" | "cmd">) {
      const tick = p.cmd ? Math.max(Math.round(p.x), 0) : nearestOf(p.x, memory.snap);
      editPoints(song, "Add a point", track, lane, (all) => [...all, { tick, value: laneValue(travel(track, lane), 1 - p.y) }]);
      choose(track, lane, [tick]);
      return tick;
    },
    /** A press on the point at `tick`: alt removes it, shift adds it to the chosen ones or lets it go, else it is chosen alone. */
    pressPoint(track: string, lane: string, tick: number, p: Pick<Pointer, "shift" | "alt">) {
      if (p.alt) return this.removePoint(track, lane, tick);
      const chosen = chosenIn(track, lane);
      if (!p.shift) return choose(track, lane, [tick]);
      choose(track, lane, chosen.includes(tick) ? chosen.filter((each) => each !== tick) : [...chosen, tick]);
    },
    /** The point at `tick` dragged by `p`: with the others chosen in its lane when it is one of them, else alone between its neighbours. */
    movePoint(track: string, lane: string, tick: number, p: Pick<Pointer, "dx" | "dy">) {
      const chosen = chosenIn(track, lane);
      shift(track, lane, chosen.includes(tick) ? chosen : [tick], p.dx, p.dy);
    },
    /** The chosen points moved by `dx` ticks and `dy` of the travel, by the keys: false while none is chosen. */
    nudge(dx: number, dy: number) {
      const point = memory.point;
      return point !== null && shift(point.track, point.lane, point.ticks, dx, dy);
    },
    /** A drag over a lane: the points it covers are chosen, with those chosen already when shift is held. */
    choosePoints(track: string, lane: string, p: Pick<Pointer, "x" | "dx" | "shift">) {
      const [from, to] = [Math.min(p.x, p.x + p.dx), Math.max(p.x, p.x + p.dx)];
      const covered = pointsOf(track, lane).filter((point) => point.tick >= from && point.tick <= to).map((point) => point.tick);
      choose(track, lane, [...new Set([...(p.shift ? chosenIn(track, lane) : []), ...covered])]);
    },
    /** An alt-drag on the lane: the points it covers go. */
    erase(track: string, lane: string, p: Pick<Pointer, "x" | "dx">) {
      const [from, to] = [Math.min(p.x, p.x + p.dx), Math.max(p.x, p.x + p.dx)];
      editPoints(song, "Erase points", track, lane, (all) => all.filter((point) => point.tick < from || point.tick > to));
    },
    /** An alt-click on a point: it goes, and the last point takes the lane along. */
    removePoint(track: string, lane: string, tick: number) {
      const chosen = chosenIn(track, lane);
      if (chosen.includes(tick)) choose(track, lane, chosen.filter((each) => each !== tick));
      editPoints(song, "Remove a point", track, lane, (all) => all.filter((point) => point.tick !== tick));
    },
    /** Delete: the chosen points go. False while none is chosen. */
    removeChosen() {
      const point = memory.point;
      if (!point || point.ticks.length === 0) return false;
      memory.point = { ...point, ticks: [] };
      editPoints(song, point.ticks.length > 1 ? "Remove points" : "Remove a point", point.track, point.lane, (all) => all.filter((each) => !point.ticks.includes(each.tick)));
      return true;
    },
    /** The handle in the middle of the line from the point at `tick` dragged by `dy`: the curve whose middle is where it was let go. */
    bend(track: string, lane: string, tick: number, p: Pick<Pointer, "dy">) {
      const points = pointsOf(track, lane);
      const index = points.findIndex((point) => point.tick === tick);
      const [point, after] = [points[index], points[index + 1]];
      if (!point || !after) return;
      const range = travel(track, lane);
      const [from, to] = [positionOf(point.value, range), positionOf(after.value, range)];
      const middle = from + (to - from) * bent(0.5, point.curve ?? 0) - p.dy;
      const done = clamp((middle - from) / (to - from), 0.01, 0.99);
      const curve = clamp(Math.log(Math.log(done) / Math.log(0.5)) / Math.log(4), -1, 1);
      editPoints(song, "Bend a line", track, lane, (all) => all.map((each) => (each.tick === tick ? { ...each, curve } : each)));
    },
    /** A double click on the handle: the line is straight again. */
    straighten(track: string, lane: string, tick: number) {
      editPoints(song, "Straighten a line", track, lane, (all) => all.map((each) => (each.tick === tick ? { tick: each.tick, value: each.value } : each)));
    },
    /** A field of the LFO or the envelope that moves the number of `lane`: its shape or rate, or a time or the sustain. */
    tune(track: string, lane: string, field: "shape" | "rate" | keyof typeof ENVELOPE, value: string | number) {
      update(`Change the ${words(field).toLowerCase()}`, (draft) => {
        const child = rowIn(draft, track)?.children.find((each) => each.name === lane);
        if (child?.tool === "lfo" || child?.tool === "envelope") (child.state as Record<string, unknown>)[field] = value;
      });
    },
    /** The low or the high line of an LFO's or an envelope's band dragged by `dy`. */
    band(track: string, lane: string, end: "low" | "high", p: Pick<Pointer, "dy">) {
      const range = travel(track, lane);
      update(end === "low" ? "Change the low" : "Change the high", (draft) => {
        const child = rowIn(draft, track)?.children.find((each) => each.name === lane);
        if (child?.tool === "lfo" || child?.tool === "envelope") child.state[end] = laneValue(range, positionOf(child.state[end], range) - p.dy);
      });
    },
  };
};
