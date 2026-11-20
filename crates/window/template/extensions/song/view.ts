// What every block of the song's page lays out by: its sizes, and the rows of the plane "song",
// in order. Across, the plane is in ticks of the song; down, in rows of a track: a track and
// each of its lanes are one, the row that adds a lane is less.
import type { Drawn, ValuesOf } from "../sdk";
import { editsOf } from "./edits";
import type { bus } from "../bus";
import type { tape } from "../tape";
import type { track } from "../track";
import { control, type Bound } from "../ui/field";
import { VOLUME } from "../ui/value";
import { trackBelow } from "./clips";
import { isRow, isStrip, lanesOf, ordered, rowOfKey, type fields, type Memory, type parts, type Song } from "./model";
import { BAR } from "./time";

/** The plane the ruler, the timeline and the lanes share across, and the heads down. */
export const PLANE = "song";
/** Points per tick at zoom 1: a bar is 96 points. Zoom goes from a tenth to eight times. */
export const ACROSS = 0.025;
export const ZOOM: [min: number, max: number] = [0.1, 8];
/** The heads' column, a track's row, the row that adds a lane, a return's and the master's row. */
export const [HEAD, ROW, ADD_LANE, LOW_ROW] = [176, 64, 40, 40];
/** The ruler, the tempo lane under it while it shows, and the room left of bar 1. */
export const [RULER, TEMPO, GUTTER] = [44, 32, 8];
/** What shows below: a track's panel, the mixer, and the note editor at its shortest and its line over it. */
export const [RACK, MIXER, SHORTEST, DIVIDER] = [216, 298, 184, 5];
/** The returns shown open before they scroll: half a row says there are more. */
const RETURNS = 4.5;
/** The song shows at least 64 bars, and 32 after what it holds. */
const [SHOWN, AFTER] = [64 * BAR, 32 * BAR];

/** A child of the song as its view gets it. */
export type Seen = Drawn<ValuesOf<typeof fields>, typeof parts>["children"][number];
/** A track or an audio track: a row of the timeline. */
export type Track = Extract<Seen, { tool: typeof track | typeof tape }>;
/** A track, an audio track or a return: what has a panel and a strip. */
export type Strip = Extract<Seen, { tool: typeof track | typeof tape | typeof bus }>;

/** A row of the plane, from `top` and `height` tall, in rows of a track. */
export type Row = { top: number; height: number; track: Track } & (
  | { kind: "track" }
  | { kind: "lane"; lane: string; target: string }
  | { kind: "add-lane" }
);

/** The rows of the plane in order: each track, and under one whose lanes show, a row for each lane and one to add a lane. */
export function rowsOf(tracks: readonly Track[], memory: Pick<Memory, "lanes">): Row[] {
  const rows: Row[] = [];
  let top = 0;
  const add = (row: Row) => (rows.push(row), (top += row.height));
  for (const track of tracks) {
    add({ kind: "track", track, top, height: 1 });
    if (!memory.lanes.includes(track.name)) continue;
    for (const { lane, target } of lanesOf(track)) add({ kind: "lane", track, lane, target, top, height: 1 });
    add({ kind: "add-lane", track, top, height: ADD_LANE / ROW });
  }
  return rows;
}

/** The lane rows of each track whose lanes show, a list a track. */
export function laneGroups(rows: readonly Row[]) {
  const groups = new Map<string, Extract<Row, { kind: "lane" | "add-lane" }>[]>();
  for (const row of rows) if (row.kind !== "track") groups.set(row.track.name, [...(groups.get(row.track.name) ?? []), row]);
  return [...groups.values()];
}

/** How tall the rows under the heads are, in points: the returns' row and those open, then the master's. */
export const lowOf = (returns: number, open: boolean) => (returns === 0 ? 0 : LOW_ROW + (open ? Math.min(returns, RETURNS) * LOW_ROW : 0)) + LOW_ROW;

/** The tick the song shows to: past its clips, regions, lane points and repeat. */
export function endOf(tracks: readonly Track[], repeat: { to: number } | null): number {
  const ends = tracks.flatMap((track) =>
    track.children.flatMap((child) => {
      if (child.tool === "timeline") return [child.state.start + (child.state.length ?? Math.max(0, ...child.state.notes.map((note) => note.start + note.length)))];
      if (child.tool === "region") return [child.state.start];
      if (child.tool === "lane") return child.state.points.map((point) => point.tick);
      return [];
    }),
  );
  return Math.max(SHOWN, ...ends, repeat?.to ?? 0) + AFTER;
}

/** The track at `y` of the plane, as its index among the tracks: what the edits take as a row. -1 past the last. */
export function trackAt(rows: readonly Row[], tracks: readonly Track[], y: number): number {
  const row = rows.find((each) => y >= each.top && y < each.top + each.height);
  return row ? tracks.indexOf(row.track) : y < 0 ? 0 : -1;
}

/** `y` of the plane where the edits take rows: a track's row is its index on, and its lanes are the edge under it. */
export function trackY(rows: readonly Row[], tracks: readonly Track[], y: number): number {
  const row = rows.find((each) => y < each.top + each.height) ?? rows.at(-1);
  if (!row) return 0;
  const index = tracks.indexOf(row.track);
  return row.kind === "track" ? index + Math.min(Math.max(y - row.top, 0), 1) : index + 1;
}

/** The one chosen track or return, which its head shows: of what shows below, else of the chosen clips, else beside the mixer the one the keyboard plays. */
export const chosenTrack = (memory: Memory) =>
  trackBelow(memory) ?? (memory.selected[0] === undefined ? null : rowOfKey(memory.selected[0])) ?? (memory.below?.kind === "mixer" ? memory.track : null);

/** What every block of the page gets: the song, its edits, and its rows worked out once a run. */
function laidOut(self: Drawn<ValuesOf<typeof fields>, typeof parts>, song: Song) {
  const strips = ordered(self.children.filter(isStrip), self.state.order);
  const tracks = strips.filter(isRow);
  const returns = strips.filter((each) => each.tool === "bus");
  const rows = rowsOf(tracks, song.memory);
  return {
    self,
    song,
    edits: editsOf(song),
    strips,
    tracks,
    returns,
    rows,
    /** How tall the rows are, in rows of a track. */
    height: rows.reduce((sum, row) => sum + row.height, 0),
    /** How tall the returns and the master are under the heads, in points. */
    low: lowOf(returns.length, song.memory.returnsOpen),
    // The range the ruler shows, not the clock's: a view that reads the clock runs again as it plays.
    end: endOf(tracks, song.memory.repeatRange),
  };
}

const pages = new WeakMap<object, Page>();
/** The page of a song: one object, of the newest run, so a handler a memo kept from an earlier run reads the song as it is now. */
export function pageOf(self: Drawn<ValuesOf<typeof fields>, typeof parts>, song: Song): Page {
  const page = laidOut(self, song);
  const kept = pages.get(song.memory);
  if (!kept) pages.set(song.memory, page);
  return kept ? Object.assign(kept, page) : page;
}
/** What every block of the song's page gets. */
export type Page = ReturnType<typeof laidOut>;

/** The fader `fader` of `name`, a track, a return or the master: its mix, or a send `send-<return>`. */
function faderOf({ self }: Page, name: string, fader = "mix") {
  const owner = self.children.find((child) => child.name === name);
  const found = name === "master" ? owner : owner && isStrip(owner) ? owner.children.find((child) => child.name === fader) : undefined;
  return found?.tool === "fader" ? found : undefined;
}

/** A gain in dB on the fader's scale, as a Fader or a Knob takes it: -inf is silence. */
function gainOf(page: Page, path: string, gain: number | "-inf" | undefined, change: (db: number | "-inf") => void): Bound {
  const value = gain === "-inf" ? -Infinity : (gain ?? 0);
  return { ...control(page.song.held, path, { min: VOLUME.min, max: VOLUME.max, unit: "db" }, value, (db) => change(db === -Infinity ? "-inf" : db)), ...VOLUME, initial: 0 };
}

/** The volume of the fader of `name`, a track, a return or the master, as a Fader takes it: one undo step. */
export function volumeOf(page: Page, name: string): Bound {
  const path = name === "master" ? "master.gain_db" : `${name}/mix.gain_db`;
  return gainOf(page, path, faderOf(page, name)?.state.gain_db, (db) => page.edits.tracks.volume(name, db));
}

/** How loud `name` sends into the return `back`, as a Knob takes it: one undo step. */
export function sendOf(page: Page, name: string, back: string): Bound {
  return gainOf(page, `${name}/send-${back}.gain_db`, faderOf(page, name, `send-${back}`)?.state.gain_db ?? "-inf", (db) => page.edits.tracks.send(name, back, db));
}

/** The pan of `name`, a track, a return or the master, as a Knob takes it: one undo step. */
export function panOf(page: Page, name: string): Bound {
  const path = name === "master" ? "master.pan" : `${name}/mix.pan`;
  return control(page.song.held, path, { min: -1, max: 1, unit: "pan", default: 0 }, faderOf(page, name)?.state.pan ?? 0, (pan) => page.edits.tracks.pan(name, pan));
}
