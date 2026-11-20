// What the song does to its clips and regions: choose them, add, move, copy, cut, paste,
// duplicate, delete and resize them, each with the automation under it.
// The timeline's plane: x in ticks of the song, y in rows of the timeline, its tracks in `order`.
import type { Dropped, FilledStateOfTool, Frozen, Pointer, StateOfTool } from "../sdk";
import { freeName, splitTarget } from "../track";
import { emptied, laid, laneAt, lift, type AutomationPoint } from "./lanes";
import { itemAt, itemsOf, nameFrom, rowOfKey, rowsOf, type Child, type Draft, type RowKind, type Song } from "./model";
import { BAR, barAt, BEAT, cellOf, stepOf, ticksAcross } from "./time";

type Clip = FilledStateOfTool<"timeline">;
type Region = FilledStateOfTool<"region">;
type Scale = FilledStateOfTool<"lane">["scale"];

/** How long a timeline plays: its `length`, else to the end of its last note. */
export const lengthOf = (clip: Frozen<Clip>) => clip.length ?? Math.max(1, ...clip.notes.map((note) => note.start + note.length));

/**
 * How long the clip or the region `key` names is on the timeline, in ticks: a region to its last
 * warp point, else as long as it plays through the tempo changes: what the window says of it, or
 * to the end its record names.
 */
export function lengthAt(song: { children: readonly Child[]; clock: { tempo_map: { tempo_changes: readonly { tick: number; bpm: number }[] } } }, key: string): number {
  const item = itemAt(song.children, key);
  if (item?.tool === "timeline") return lengthOf(item.state);
  if (item?.tool !== "region") return 0;
  const { warp, start, file_start_seconds, file_end_seconds } = item.state;
  const told = (item as { facts?: { seconds?: number } }).facts?.seconds;
  const seconds = told ?? (file_end_seconds === null ? 0 : file_end_seconds - file_start_seconds);
  return warp?.at(-1)?.ticks ?? ticksAcross(song.clock.tempo_map.tempo_changes, start, seconds);
}

/** The track of what shows below: its panel, or the notes of one of its clips. */
export const trackBelow = ({ below }: Song["memory"]) => (below?.kind === "editor" ? rowOfKey(below.clip) : below?.kind === "track" ? below.name : null);

/**
 * Chooses the clips and regions `keys` names: the keyboard plays the track of the first, or
 * `track`, and Export selection exports their bars. A mark, a point or an effect is let go.
 */
export function choose(song: Song, keys: readonly string[], track: string | null = trackBelow(song.memory)) {
  const { memory } = song;
  memory.selected = [...new Set(keys)];
  memory.point = null;
  memory.mark = null;
  memory.effect = null;
  const first = keys[0];
  memory.track = first === undefined ? track : rowOfKey(first);
  const spans = keys.flatMap((key) => {
    const start = itemAt(song.children, key)?.state.start;
    return start === undefined ? [] : [{ from: start, to: start + lengthAt(song, key) }];
  });
  const span = spans.length === 0 ? undefined : { from: Math.min(...spans.map((each) => each.from)), to: Math.max(...spans.map((each) => each.to)) };
  song.select(first ?? track, { span });
  // What shows below follows: the panel to the track of the first, an open editor to it.
  const below = memory.below;
  if (first === undefined) return;
  if (below?.kind === "track" && below.name !== rowOfKey(first)) memory.below = { kind: "track", name: rowOfKey(first) };
  else if (below?.kind === "editor" && !memory.selected.includes(below.clip)) {
    memory.below = itemAt(song.children, first)?.tool === "timeline" ? { kind: "editor", clip: first } : { kind: "track", name: rowOfKey(first) };
    memory.notes = [];
  }
}

/**
 * A clip whose start moved by `dx`: what it holds stays where it is in the song. The start stops
 * at tick 0, at the first note and `unit` before the end; a lane cut short keeps the value it had
 * at the new start, and the pedal what it was there.
 */
export function startMoved(clip: Clip, dx: number, unit: number) {
  const length = lengthOf(clip);
  const by = Math.min(Math.max(dx, -clip.start), Math.min(...clip.notes.map((note) => note.start), length - unit));
  clip.start += by;
  clip.length = length - by;
  for (const note of clip.notes) note.start -= by;
  const pedal = clip.pedal.filter((change) => change.start >= by).map((change) => ({ ...change, start: change.start - by }));
  const held = pedal[0]?.start === 0 ? undefined : clip.pedal.filter((change) => change.start < by).at(-1);
  clip.pedal = held ? [{ ...held, start: 0 }, ...pedal] : pedal;
  cutLanes(clip, Math.max(by, 0), length);
  if (by < 0) shiftLanes(clip, -by);
}

/** A clip that ends at `length`: what starts at or past it is cut, as a timeline plays nothing there, and a lane keeps the value it had at the new end. */
export function endMoved(clip: Clip, length: number) {
  clip.length = length;
  clip.notes = clip.notes.filter((note) => note.start < length);
  clip.pedal = clip.pedal.filter((change) => change.start < length);
  cutLanes(clip, 0, length);
}

type ClipPoint = Clip["bend"][number];
/**
 * The lanes of a clip from `from` to `to`, from `from`: where points are cut away, a point on the
 * edge keeps the value the lane had there. A controller lane plays from its first point, so one
 * that starts at or after `to` is gone.
 */
function cutLanes(clip: Clip, from: number, to: number) {
  const cut = (points: readonly ClipPoint[]): ClipPoint[] => {
    const kept = points.filter(({ tick }) => tick >= from && tick < to).map(({ tick, value }) => ({ tick: tick - from, value }));
    const edge = (tick: number) => ({ tick: tick - from, value: Math.round(laneAt(points, tick, "linear")) });
    const before = points.some(({ tick }) => tick < from) && kept[0]?.tick !== 0 ? [edge(from)] : [];
    const after = points.some(({ tick }) => tick >= to) && kept.at(-1)?.tick !== to - 1 - from ? [edge(to - 1)] : [];
    return [...before, ...kept, ...after];
  };
  for (const lane of ["bend", "mod_wheel", "pressure"] as const) clip[lane] = cut(clip[lane]);
  clip.controllers = Object.fromEntries(Object.entries(clip.controllers).flatMap(([number, points]) => ((points[0]?.tick ?? to) < to ? [[number, cut(points)]] : [])));
}

/** Every lane point of a clip `by` ticks later. */
function shiftLanes(clip: Clip, by: number) {
  const shift = (points: readonly ClipPoint[]) => points.map(({ tick, value }) => ({ tick: tick + by, value }));
  for (const lane of ["bend", "mod_wheel", "pressure"] as const) clip[lane] = shift(clip[lane]);
  clip.controllers = Object.fromEntries(Object.entries(clip.controllers).map(([number, points]) => [number, shift(points)]));
}

/** A copy of a record to change. */
export const copyOf = <T>(state: Frozen<T>) => structuredClone(state) as T;

/** A clip or a region as it was taken: its row and track, its name, its record, its length, and the line of each lane of its track under it. */
type Taken = {
  row: number;
  track: string;
  name: string;
  item: { tool: "timeline"; state: Clip } | { tool: "region"; state: Region };
  length: number;
  lines: { target: string; scale: Scale; points: AutomationPoint[] }[];
};
/** What cut and copy keep: the clips and the regions, their starts from the earliest and their rows from the top. */
export type Clipboard = Taken[];

/** The clips and the regions `keys` names, `row/name`, with the automation under each. */
export function take(song: Pick<Song, "children" | "clock">, rows: readonly RowKind[], keys: Iterable<string>): Taken[] {
  return [...keys].flatMap((key) => {
    const track = rowOfKey(key);
    const owner = song.children.find((child) => child.name === track);
    const child = itemAt(song.children, key);
    if (!child || (owner?.tool !== "track" && owner?.tool !== "tape")) return [];
    const item = child.tool === "timeline" ? { tool: "timeline" as const, state: copyOf<Clip>(child.state) } : { tool: "region" as const, state: copyOf<Region>(child.state) };
    const [start, end] = [item.state.start, item.state.start + lengthAt(song, key)];
    const lines = owner.state.automation.flatMap(({ lane, target }) => {
      const points = owner.children.find((each) => each.name === lane);
      const line = points?.tool === "lane" ? lift(points.state.points, points.state.scale, start, end) : null;
      return line && points?.tool === "lane" ? [{ target, scale: points.state.scale, points: line }] : [];
    });
    return [{ row: rows.findIndex((row) => row.name === track), track, name: child.name, item, length: end - start, lines }];
  });
}

/** `taken` as the clipboard keeps it: from the earliest start and the top row. */
function clipboardOf(taken: readonly Taken[]): Clipboard | null {
  if (taken.length === 0) return null;
  const [from, top] = [Math.min(...taken.map((each) => each.item.state.start)), Math.min(...taken.map((each) => each.row))];
  return taken.map((each) => ({ ...each, row: each.row - top, item: { ...each.item, state: { ...each.item.state, start: each.item.state.start - from } } }) as Taken);
}

/** Takes `taken` out of the song, and the lines each took that `goes` says go along: each lane straight between the edges of where it was. */
function removeTaken({ children }: Draft, taken: readonly Taken[], goes: (each: Taken, target: string) => boolean) {
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
    return { each, row: rows[Math.min(Math.max(row, 0), rows.length - 1)], start: Math.max(Math.round(start), 0) };
  });
  return placed.every(({ each, row }) => row && itemsOf(row) === each.item.tool) ? (placed as { each: Taken; row: RowKind; start: number }[]) : null;
}
type Placed = NonNullable<ReturnType<typeof placing>>;

/**
 * Puts copies of what `placed` places in the song, with the lines they took over what each lane
 * had there: a volume or a pan on any row, a lane of a device on its own track only, as the
 * device is the track's. A row with no lane for a volume or a pan gets one, at its fader around
 * what lands. The keys of the copies.
 */
export function putTaken({ children }: Draft, placed: Placed): string[] {
  const keys: string[] = [];
  for (const { each, row, start } of placed) {
    const owner = children.find((child) => child.name === row.name);
    if (owner?.tool !== "track" && owner?.tool !== "tape") continue;
    const name = freeName(owner.children.map((child) => child.name), each.name);
    if (owner.tool === "track" && each.item.tool === "timeline") owner.children.push({ name, tool: "timeline", state: { ...each.item.state, start } });
    else if (owner.tool === "tape" && each.item.tool === "region") owner.children.push({ name, tool: "region", state: { ...each.item.state, start } });
    keys.push(`${owner.name}/${name}`);
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
 * Moves the clips and regions `keys` names by `dx` ticks and `dy` rows, each onto its own row
 * where that row holds the other kind, or copies them there, with the automation under them,
 * as one undo step. The keys of where they are after.
 */
export function moveItems(song: Song, keys: readonly string[], dx: number, dy: number, copy: boolean): string[] {
  const rows = rowsOf(song.children, song.state.order);
  const taken = take(song, rows, keys);
  const rowOf = new Map(taken.map((each) => [each, itemsOf(rows[each.row + dy] ?? { tool: "" }) === each.item.tool && rows[each.row + dy] ? each.row + dy : each.row]));
  // A volume and a pan go along to any track, a lane of a device only on its own.
  const goes = (each: Taken, target: string) => target.startsWith("mix.") || rows[rowOf.get(each) ?? -1]?.name === each.track;
  const placed = placing(rows, taken, (each) => ({ row: rowOf.get(each) ?? each.row, start: each.item.state.start + dx }));
  if (!placed) return [...keys];
  let after = [...keys];
  song.update(copy ? "Copy clips" : "Move clips", (draft) => {
    if (!copy) removeTaken(draft, taken, goes);
    after = putTaken(draft, placed);
  });
  return after;
}

/** Adds an empty clip to the track `name`, from `start` and `length` long, as one undo step: its key, or none when `name` is no track. */
export function addClip({ update }: Pick<Song, "update">, name: string, start: number, length: number): string | undefined {
  let added: string | undefined;
  update("Add a clip", ({ children }) => {
    const owner = children.find((child) => child.name === name);
    if (owner?.tool !== "track") return;
    const clip = freeName(owner.children.map((child) => child.name), "clip");
    owner.children.push({ name: clip, tool: "timeline", state: { start, length, notes: [] } });
    added = `${owner.name}/${clip}`;
  });
  return added;
}

/** Changes the clips `keys` names, `track/clip`, in one undo step. */
export const editClips = ({ update }: Pick<Song, "update">, label: string, keys: Iterable<string>, change: (clip: Clip) => void) =>
  update(label, ({ children }) => {
    for (const key of keys) {
      const owner = children.find((each) => each.name === rowOfKey(key));
      const clip = owner?.tool === "track" ? owner.children.find((child) => child.name === key.slice(key.indexOf("/") + 1)) : undefined;
      if (clip?.tool === "timeline") change(clip.state);
    }
  });

/** What the keys and a clip's menu do to the chosen clips. */
export type ClipCommand = "cut" | "copy" | "duplicate" | "delete";

/** The parts of a MIDI file, as a drop gives them. */
type Parts = Dropped["parts"];

/** Adds an instrument track called `title` after the others, with `clips` on it: its name. */
export type AddTrack = (draft: Draft, title: string, clips?: { name: string; state: StateOfTool<"timeline"> }[]) => string;

/** What a view and the keys do to the clips and regions of the song: each change one undo step. */
export const clipEdits = (song: Song, addTrack: AddTrack) => {
  const { memory } = song;
  const rows = () => rowsOf(song.children, song.state.order);
  const tick = () => Math.round(song.clock.tick);
  return {
    choose: (keys: readonly string[], track?: string | null) => choose(song, keys, track),
    /** A press on empty space: nothing is chosen. */
    clear: () => choose(song, []),
    /**
     * A press on the clip or the region `key`: a right press chooses it for its menu, a double
     * click opens a clip's notes below, shift or cmd adds it or takes it out, a plain press
     * chooses it alone. A region chosen alone shows its card in its track's panel.
     */
    press(key: string, p: Pick<Pointer, "clicks" | "shift" | "cmd" | "right">) {
      const region = itemAt(song.children, key)?.tool === "region";
      if (p.right) {
        if (!memory.selected.includes(key)) choose(song, [key]);
      } else if (p.clicks === 2 && !region) {
        this.open(key);
      } else if (p.shift || p.cmd) {
        choose(song, memory.selected.includes(key) ? memory.selected.filter((each) => each !== key) : [...memory.selected, key]);
      } else {
        choose(song, [key]);
      }
      if (region && memory.selected.length === 1 && memory.selected[0] === key) memory.below = { kind: "track", name: rowOfKey(key) };
    },
    /** Opens the notes of the clip `key` below. */
    open(key: string) {
      if (itemAt(song.children, key)?.tool !== "timeline") return;
      memory.below = { kind: "editor", clip: key };
      memory.notes = [];
    },
    /** A double click on empty space of row `y`: a clip a bar long in the cell of the snap under `x`, chosen. */
    add(p: Pick<Pointer, "x" | "y">) {
      const row = rows()[Math.floor(p.y)];
      if (row?.tool !== "track") return choose(song, []);
      const start = cellOf(p.x, memory.snap);
      const { start: from, end } = barAt(song.clock.tempo_map.time_signatures, start);
      const added = addClip(song, row.name, start, end - from || BAR);
      if (added) choose(song, [added]);
    },
    /** Adds a clip at the playhead, on the track of the first chosen clip, else on the chosen track: as long as the bar it is in. */
    addAtPlayhead(): boolean {
      const name = memory.selected[0] === undefined ? memory.track : rowOfKey(memory.selected[0]);
      if (name === null || song.children.find((child) => child.name === name)?.tool !== "track") return false;
      const start = cellOf(tick(), memory.snap);
      const { start: from, end } = barAt(song.clock.tempo_map.time_signatures, start);
      const added = addClip(song, name, start, end - from || BAR);
      if (added) choose(song, [added]);
      return true;
    },
    /** A drag of the chosen by `dx` ticks and `dy` rows: alt copies them, and the copies are chosen. */
    move(p: Pick<Pointer, "dx" | "dy" | "alt">) {
      if (memory.selected.length > 0) choose(song, moveItems(song, memory.selected, Math.round(p.dx), Math.round(p.dy), p.alt));
    },
    /** A drag of the end of the chosen clips: shorter cuts what starts past it, at least a step of the snap. */
    resizeEnd(p: Pick<Pointer, "dx">) {
      editClips(song, "Resize clips", memory.selected, (clip) => endMoved(clip, Math.max(lengthOf(clip) + Math.round(p.dx), stepOf(memory.snap))));
    },
    /** A drag of the start of the chosen clips: their notes stay where they are in the song. */
    resizeStart(p: Pick<Pointer, "dx">) {
      editClips(song, "Resize clips", memory.selected, (clip) => startMoved(clip, Math.round(p.dx), stepOf(memory.snap)));
    },
    /** A drag over empty space from `x`, `y` (rows): what it covers is chosen; shift or cmd adds it. */
    marquee(p: Pick<Pointer, "x" | "y" | "dx" | "dy" | "shift" | "cmd">) {
      const [left, right] = [Math.min(p.x, p.x + p.dx), Math.max(p.x, p.x + p.dx)];
      const [top, bottom] = [Math.min(p.y, p.y + p.dy), Math.max(p.y, p.y + p.dy)];
      const inside = rows().flatMap((row, index) => {
        if (index + 1 <= top || index >= bottom) return [];
        const owner = song.children.find((child) => child.name === row.name);
        const items = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.filter((child) => child.tool === "timeline" || child.tool === "region") : [];
        return items.map((item) => `${row.name}/${item.name}`).filter((key) => {
          const start = itemAt(song.children, key)?.state.start ?? 0;
          return start < right && start + lengthAt(song, key) > left;
        });
      });
      choose(song, [...(p.shift || p.cmd ? memory.selected : []), ...inside]);
    },
    /** Cut, copy, duplicate or delete the chosen: a duplicate goes right after them and is chosen; a cut takes the automation under them, a delete leaves it. */
    command(command: ClipCommand): boolean {
      const taken = take(song, rows(), memory.selected);
      if (taken.length === 0) return false;
      if (command === "cut" || command === "copy") memory.clipboard = clipboardOf(taken);
      if (command === "duplicate") {
        const span = Math.max(...taken.map((each) => each.item.state.start + each.length)) - Math.min(...taken.map((each) => each.item.state.start));
        const placed = placing(rows(), taken, (each) => ({ row: each.row, start: each.item.state.start + span }));
        if (!placed) return true;
        let copies: string[] = [];
        song.update("Duplicate clips", (draft) => (copies = putTaken(draft, placed)));
        choose(song, copies);
      } else if (command !== "copy") {
        song.update(command === "cut" ? "Cut clips" : "Remove clips", (draft) => removeTaken(draft, taken, () => command === "cut"));
        memory.selected = [];
        song.select(null);
      }
      return true;
    },
    /** Pastes at the playhead: the top one on the track of the first chosen, else the chosen track, else the first; the rest keep their rows under it. */
    paste(): boolean {
      const clipboard = memory.clipboard;
      if (!clipboard) return false;
      const all = rows();
      const first = memory.selected[0] === undefined ? memory.track : rowOfKey(memory.selected[0]);
      const top = first === null ? 0 : all.findIndex((row) => row.name === first);
      const placed = top < 0 ? null : placing(all, clipboard, (each) => ({ row: top + each.row, start: tick() + each.item.state.start }));
      if (!placed) return true;
      let pasted: string[] = [];
      song.update("Paste clips", (draft) => (pasted = putTaken(draft, placed)));
      choose(song, pasted);
      return true;
    },
    /** Moves the chosen by a step of the snap in time, the earliest stopping at tick 0. */
    nudge(by: -1 | 1): boolean {
      if (memory.selected.length === 0) return false;
      const starts = memory.selected.map((key) => itemAt(song.children, key)?.state.start ?? 0);
      const dx = Math.max(by * stepOf(memory.snap), -Math.min(...starts));
      if (dx !== 0) choose(song, moveItems(song, memory.selected, dx, 0, false));
      return true;
    },
    /** Moves the chosen to the nearest rows above or below where every one lands on a row of its own kind. */
    toRow(step: -1 | 1): boolean {
      if (memory.selected.length === 0) return false;
      const all = rows();
      const taken = take(song, all, memory.selected);
      const fits = (by: number) => taken.every((each) => all[each.row + by] !== undefined && itemsOf(all[each.row + by]!) === each.item.tool);
      const by = all.map((_, times) => step * (times + 1)).find(fits);
      if (by !== undefined) choose(song, moveItems(song, memory.selected, 0, by, false));
      return true;
    },
    /** Chooses the clip or region after or before the chosen one on its track; with none chosen, the first or the last of the chosen track, or of the first. */
    step(by: -1 | 1): boolean {
      const chosen = memory.selected[0];
      const row = chosen === undefined ? (memory.track ?? rows()[0]?.name) : rowOfKey(chosen);
      const owner = song.children.find((child) => child.name === row);
      const items = (owner?.tool === "track" || owner?.tool === "tape" ? owner.children.flatMap((child) => (child.tool === "timeline" || child.tool === "region" ? [child] : [])) : [])
        .sort((a, b) => a.state.start - b.state.start);
      const at = items.findIndex((item) => `${row}/${item.name}` === chosen);
      const next = items[at < 0 ? (by > 0 ? 0 : items.length - 1) : Math.min(Math.max(at + by, 0), items.length - 1)];
      if (!next || row === undefined) return false;
      choose(song, [`${row}/${next.name}`]);
      return true;
    },
    /** Chooses every clip and region. */
    all() {
      choose(song, rows().flatMap(({ name }) => {
        const owner = song.children.find((child) => child.name === name);
        const items = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.filter((child) => child.tool === "timeline" || child.tool === "region") : [];
        return items.map((child) => `${name}/${child.name}`);
      }));
    },
    /** Gives the clip `key` the title `title`. */
    rename(key: string, title: string) {
      memory.renamingClip = null;
      editClips(song, "Rename a clip", [key], (clip) => (clip.title = title.trim()));
    },
    /** Audio files dropped at `x` on row `y`: regions on that audio track, else the first, at the nearest beat. */
    dropAudio(audio: readonly string[], p: Pick<Pointer, "x" | "y">) {
      const all = rows();
      const owner = all[Math.floor(p.y)]?.tool === "tape" ? all[Math.floor(p.y)] : all.find((row) => row.tool === "tape");
      if (!owner || audio.length === 0) return;
      const start = Math.max(Math.round(p.x / BEAT) * BEAT, 0);
      const added: string[] = [];
      song.update("Add audio", ({ children }) => {
        const tape = children.find((child) => child.name === owner.name);
        if (tape?.tool !== "tape") return;
        for (const file of audio) {
          const name = freeName(tape.children.map((child) => child.name), "region");
          tape.children.push({ name, tool: "region", state: { asset: file, start } });
          added.push(`${tape.name}/${name}`);
        }
      });
      choose(song, added);
    },
    /** The parts of a MIDI file dropped at `x` on row `y`: one onto that instrument track as a clip in the cell there, else each onto a new track named after it. */
    dropNotes(parts: Parts, p: Pick<Pointer, "x" | "y">) {
      if (parts.length === 0) return;
      const row = rows()[Math.floor(p.y)];
      const start = cellOf(p.x, memory.snap);
      let added: string[] = [];
      song.update("Import notes", (draft) => {
        const owner = draft.children.find((child) => child.name === row?.name);
        const [only] = parts;
        if (owner?.tool === "track" && only && parts.length === 1) {
          const name = freeName(owner.children.map((child) => child.name), "clip");
          owner.children.push({ name, tool: "timeline", state: { start, notes: only.notes } });
          added = [`${owner.name}/${name}`];
        } else {
          added = parts.map((part) => `${addTrack(draft, part.name || "Notes", [{ name: "clip", state: { start, notes: part.notes } }])}/clip`);
        }
      });
      choose(song, added);
    },
  };
};
