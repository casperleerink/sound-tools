import type { FilledStateOfTool, Key } from "./sdk";
import { editorKey } from "./clip-editor";
import type { Song } from "./song";
import { itemCommand, moveItems, placing, putTaken, take, type ClipCommand } from "./song-clips";
import { BEAT, clamp, itemTool, ITEMS, rowsOf } from "./song-page";
import { addTempoAtPlayhead, removeMark } from "./song-ruler";
import { askToDelete, toggleRecordingOf } from "./song-tracks";

// What the keys do on the song's page: to the notes of the open clip first, then to the chosen
// mark of the ruler, point of a lane, regions, track or clips.

export function songKey(tool: Song, key: Key) {
  const { state, memory, update, clock, children } = tool;
  if (key.key === "space") {
    if (key.down) (clock.playing ? clock.pause() : clock.play());
    return;
  }
  if (!key.down) return;
  if (key.key === "r") {
    toggleRecordingOf(tool, state.order);
    return;
  }
  // The editor's notes first, while it has some chosen; escape with none closes it.
  const opened = memory.below?.kind === "editor" ? memory.below.clip : null;
  if (opened && key.key === "escape" && memory.notes.size === 0) {
    memory.below = { kind: "track", name: opened.split("/")[0] ?? "" };
    return;
  }
  if (opened) {
    const [trackName, clipName] = opened.split("/");
    const row = children.find((each) => each.name === trackName);
    const clip = row?.tool === "track" ? row.children.find((child) => child.name === clipName) : undefined;
    const key2 = opened;
    const edit = (label: string, change: (clip: FilledStateOfTool<"timeline">) => void) =>
      update(label, ({ children }) => {
        const owner = children.find((each) => each.name === key2.split("/")[0]);
        const draft = owner?.tool === "track" ? owner.children.find((child) => child.name === key2.split("/")[1]) : undefined;
        if (draft?.tool === "timeline") change(draft.state);
      });
    const forNotes = memory.notes.size > 0 || (key.cmd && (key.key === "a" || (key.key === "v" && memory.copied !== null)));
    const state = clip?.tool === "timeline" ? clip.state : undefined;
    if (forNotes && editorKey(key, state, clock.tick, memory, edit)) return;
  }
  if (key.key === "t" && !key.cmd) {
    addTempoAtPlayhead(clock, memory);
    return;
  }
  if (memory.mark && ["delete", "backspace", "escape", "enter"].includes(key.key) && !key.cmd) {
    const mark = memory.mark;
    memory.mark = null;
    if (key.key === "enter") memory.editing = mark.kind === "section" ? mark : null;
    else if (key.key !== "escape") removeMark(clock, mark);
    return;
  }
  if (memory.point && (key.key === "delete" || key.key === "backspace" || key.key === "escape")) {
    const { track: owner, lane, tick } = memory.point;
    memory.point = null;
    if (key.key === "escape") return;
    // The last point takes its lane along.
    update("Remove a point", ({ children }) => {
      const row = children.find((child) => child.name === owner);
      const points = row?.tool === "track" || row?.tool === "tape" ? row.children.find((child) => child.name === lane) : undefined;
      if ((row?.tool !== "track" && row?.tool !== "tape") || points?.tool !== "lane") return;
      const rest = points.state.points.filter((point) => point.tick !== tick);
      if (rest.length > 0) points.state.points = rest;
      else {
        row.children.splice(row.children.findIndex((child) => child.name === lane), 1);
        row.state.automation = row.state.automation.filter((each) => each.lane !== lane);
      }
    });
    return;
  }
  // Alt-up and alt-down step the chosen regions' gain by a decibel.
  if (key.alt && (key.key === "up" || key.key === "down") && [...memory.selected].some((each) => itemTool(children, each) === "region")) {
    const by = key.key === "up" ? 1 : -1;
    update("Change gain", ({ children }) => {
      for (const key of memory.selected) {
        const [owner, name] = key.split("/");
        const tape = children.find((child) => child.name === owner);
        const region = tape?.tool === "tape" ? tape.children.find((child) => child.name === name) : undefined;
        if (region?.tool === "region") region.state.gain_db = clamp(Math.round((Math.max(region.state.gain_db, -48) + by) * 10) / 10, -48, 24);
      }
    });
    return;
  }
  if (memory.selected.size === 0 && trackKey(key, state.order, tool, (names) => update("Move a track", (song) => (song.state.order = names)))) return;
  itemKey(key, state.order, tool);
}

/**
 * What a key does to the track whose panel shows while no clip is chosen, as the arrangement's
 * keys did: enter names it, up and down choose the track above or below, alt-up and alt-down
 * move it there. Whether it was one of them.
 */
function trackKey(key: Key, order: readonly string[], tool: Song, reorder: (names: string[]) => void): boolean {
  const { children, memory, select } = tool;
  const name = memory.below?.kind === "track" ? memory.below.name : null;
  if (!name || key.cmd || !["enter", "up", "down", "delete", "backspace"].includes(key.key)) return false;
  if (key.key === "enter") {
    memory.renaming = name;
    return true;
  }
  if (key.key === "delete" || key.key === "backspace") {
    void askToDelete(tool, name);
    return true;
  }
  const rank = (each: string) => (order.includes(each) ? order.indexOf(each) : order.length);
  const names = children
    .filter((child) => child.tool === "track" || child.tool === "tape")
    .map((child) => child.name)
    .sort((a, b) => rank(a) - rank(b) || (a < b ? -1 : 1));
  const at = names.indexOf(name);
  if (at < 0) return false;
  const next = clamp(at + (key.key === "up" ? -1 : 1), 0, names.length - 1);
  if (key.alt) {
    if (next === at) return true;
    names.splice(at, 1);
    names.splice(next, 0, name);
    reorder(names);
    return true;
  }
  const other = names[next]!;
  memory.below = { kind: "track", name: other };
  memory.track = other;
  select(other);
  return true;
}

/** What a key does to the chosen clips and regions, as the arrangement's: copy, cut, paste, duplicate, delete, nudge; enter opens a clip. */
function itemKey(key: Key, order: readonly string[], tool: Song) {
  const { memory, update, clock, children } = tool;
  const rows = rowsOf(children, order);
  const command: ClipCommand | null = !key.cmd && (key.key === "delete" || key.key === "backspace") ? "delete"
    : !key.cmd ? null : key.key === "x" ? "cut" : key.key === "c" ? "copy" : key.key === "d" ? "duplicate" : null;
  const clip = [...memory.selected].find((each) => itemTool(children, each) === "timeline");
  if (command) {
    itemCommand(command, order, tool);
  } else if ((key.key === "enter" || (key.cmd && key.key === "down")) && clip) {
    // Its notes below, as a double click does.
    memory.below = { kind: "editor", clip };
    memory.notes = new Set();
  } else if (key.key === "escape") {
    memory.selected = new Set();
  } else if (key.cmd && key.key === "a") {
    memory.selected = new Set(rows.flatMap(({ name }) => {
      const owner = children.find((child) => child.name === name);
      const items = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.filter((child) => child.tool === "timeline" || child.tool === "region") : [];
      return items.map((child) => `${name}/${child.name}`);
    }));
  } else if (key.cmd && key.key === "v" && memory.clipboard) {
    // At the playhead, the top one on the track of the first chosen, else on the chosen track,
    // else on the first; a chosen return takes none.
    const first = [...memory.selected][0]?.split("/")[0] ?? memory.track;
    const top = first === null ? 0 : rows.findIndex((row) => row.name === first);
    const clipboard = memory.clipboard;
    const at = Math.round(clock.tick);
    const placed = top < 0 ? null : placing(rows, clipboard, (each) => ({ row: top + each.row, start: at + each.item.state.start }));
    if (!placed) return;
    let pasted = new Set<string>();
    update("Paste clips", (part) => (pasted = putTaken(part, placed)));
    memory.selected = pasted;
  } else if (memory.selected.size > 0 && (key.key === "left" || key.key === "right") && !key.cmd) {
    // By the snap, the earliest stopping at tick 0.
    const starts = take(children, rows, memory.selected, () => 0).map((each) => each.item.state.start);
    const dx = Math.max((key.key === "left" ? -1 : 1) * (memory.snap || BEAT / 4), -Math.min(...starts));
    if (dx !== 0) memory.selected = moveItems(tool, order, memory.selected, dx, (row) => row, false);
  } else if (memory.selected.size > 0 && (key.key === "up" || key.key === "down") && !key.cmd) {
    // To the nearest rows above or below where every one lands on a row of its own kind.
    const step = key.key === "up" ? -1 : 1;
    const taken = take(children, rows, memory.selected, () => 0);
    const fits = (by: number) => taken.every((each) => ITEMS[rows[each.row + by]?.tool ?? ""] === each.item.tool);
    const by = rows.map((_, times) => step * (times + 1)).find(fits);
    if (by !== undefined) memory.selected = moveItems(tool, order, memory.selected, 0, (row) => row + by, false);
  }
}
