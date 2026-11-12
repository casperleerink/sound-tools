import { playPause, type Action, type ActionKey, type FilledStateOfTool } from "./sdk";
import { noteCommand, type NoteCommand } from "./clip-editor";
import { editPoints } from "./song-automation";
import type { Song } from "./song";
import { itemCommand, moveItems, placing, putTaken, take, type ClipCommand } from "./song-clips";
import { BEAT, clamp, itemTool, ITEMS, rowsOf } from "./song-page";
import { addSection, addSignatureNear, addTempoAtPlayhead, marksOf, removeMark } from "./song-ruler";
import { askToDelete, toggleRecordingOf } from "./song-tracks";

// What the song does on a key. The actions of one key are tried from the top: the notes of the
// open clip first, then the chosen mark of the ruler, point of a lane, regions, track and clips.
// Each says `false` while what it works on is not chosen, and the key goes on to the next.

type SongAction = Action<Song>;

/** An action of the song. `run` says `false` when there was nothing for it to do. */
const on = (keys: readonly ActionKey[], title: string, run: (tool: Song) => boolean | void): SongAction => ({ title, keys, run });

/** Does `command` to the notes of the clip the editor shows; whether there was something to do. */
function onNotes({ memory, children, update, clock }: Song, command: NoteCommand): boolean {
  const opened = memory.below?.kind === "editor" ? memory.below.clip : null;
  if (!opened) return false;
  const [trackName, clipName] = opened.split("/");
  const row = children.find((each) => each.name === trackName);
  const clip = row?.tool === "track" ? row.children.find((child) => child.name === clipName) : undefined;
  if (clip?.tool !== "timeline") return false;
  const edit = (label: string, change: (clip: FilledStateOfTool<"timeline">) => void) =>
    update(label, ({ children }) => {
      const owner = children.find((each) => each.name === trackName);
      const draft = owner?.tool === "track" ? owner.children.find((child) => child.name === clipName) : undefined;
      if (draft?.tool === "timeline") change(draft.state);
    });
  return noteCommand(command, clip.state, clock.tick, memory, edit);
}

const notes = (keys: readonly ActionKey[], title: string, command: NoteCommand) => on(keys, title, (tool) => onNotes(tool, command));

/** Escape: closes the editor with no note chosen, else lets go of the chosen notes, mark, point or clips, else closes what shows below. */
function back(tool: Song) {
  const { memory } = tool;
  const opened = memory.below?.kind === "editor" ? memory.below.clip : null;
  if (opened && memory.notes.size === 0) memory.below = { kind: "track", name: opened.split("/")[0] ?? "" };
  else if (onNotes(tool, "none")) return;
  else if (memory.mark) memory.mark = null;
  else if (memory.point) memory.point = null;
  else if (memory.selected.size > 0) memory.selected = new Set();
  else if (memory.below) memory.below = null;
}

/** The track whose panel shows while no clip is chosen. */
const shownTrack = ({ memory }: Song) => (memory.selected.size === 0 && memory.below?.kind === "track" ? memory.below.name : null);

/** Chooses the track above or below the one whose panel shows, or with `move` puts it there. */
const trackStep = (keys: readonly ActionKey[], title: string, by: -1 | 1, move: boolean) =>
  on(keys, title, (tool) => {
    const { children, memory, select, state, update } = tool;
    const name = shownTrack(tool);
    const rank = (each: string) => (state.order.includes(each) ? state.order.indexOf(each) : state.order.length);
    const names = children
      .filter((child) => child.tool === "track" || child.tool === "tape")
      .map((child) => child.name)
      .sort((a, b) => rank(a) - rank(b) || (a < b ? -1 : 1));
    const at = name === null ? -1 : names.indexOf(name);
    if (name === null || at < 0) return false;
    const next = clamp(at + by, 0, names.length - 1);
    const other = names[next]!;
    if (!move) {
      memory.below = { kind: "track", name: other };
      memory.track = other;
      select(other);
    } else if (next !== at) {
      names.splice(at, 1);
      names.splice(next, 0, name);
      update("Move a track", (song) => (song.state.order = names));
    }
  });

/** Steps the gain of the chosen regions by a decibel. */
const regionGain = (keys: readonly ActionKey[], title: string, by: -1 | 1) =>
  on(keys, title, ({ memory, children, update }) => {
    if (![...memory.selected].some((each) => itemTool(children, each) === "region")) return false;
    update("Change gain", ({ children }) => {
      for (const key of memory.selected) {
        const [owner, name] = key.split("/");
        const tape = children.find((child) => child.name === owner);
        const region = tape?.tool === "tape" ? tape.children.find((child) => child.name === name) : undefined;
        if (region?.tool === "region") region.state.gain_db = clamp(Math.round((Math.max(region.state.gain_db, -48) + by) * 10) / 10, -48, 24);
      }
    });
  });

/** Cut, copy, duplicate or delete the chosen clips and regions. */
const clips = (keys: readonly ActionKey[], title: string, command: ClipCommand) => on(keys, title, (tool) => itemCommand(command, tool.state.order, tool));

/** Moves the chosen clips and regions by the snap in time, the earliest stopping at tick 0. */
const nudge = (keys: readonly ActionKey[], title: string, by: -1 | 1) =>
  on(keys, title, (tool) => {
    const { memory, children, state } = tool;
    if (memory.selected.size === 0) return false;
    const starts = take(children, rowsOf(children, state.order), memory.selected, () => 0).map((each) => each.item.state.start);
    const dx = Math.max(by * (memory.snap || BEAT / 4), -Math.min(...starts));
    if (dx !== 0) memory.selected = moveItems(tool, state.order, memory.selected, dx, (row) => row, false);
  });

/** Moves the chosen clips and regions to the nearest rows above or below where every one lands on a row of its own kind. */
const toRow = (keys: readonly ActionKey[], title: string, step: -1 | 1) =>
  on(keys, title, (tool) => {
    const { memory, children, state } = tool;
    if (memory.selected.size === 0) return false;
    const rows = rowsOf(children, state.order);
    const taken = take(children, rows, memory.selected, () => 0);
    const fits = (by: number) => taken.every((each) => ITEMS[rows[each.row + by]?.tool ?? ""] === each.item.tool);
    const by = rows.map((_, times) => step * (times + 1)).find(fits);
    if (by !== undefined) memory.selected = moveItems(tool, state.order, memory.selected, 0, (row) => row + by, false);
  });

/** Chooses the clip or region after or before the chosen one on its track; with none chosen, the first or the last of the chosen track, or of the first. */
const clipStep = (keys: readonly ActionKey[], title: string, by: -1 | 1) =>
  on(keys, title, ({ memory, children, state }) => {
    const [chosen] = memory.selected;
    const row = chosen?.split("/")[0] ?? memory.track ?? rowsOf(children, state.order)[0]?.name;
    const owner = children.find((child) => child.name === row);
    const items = (owner?.tool === "track" || owner?.tool === "tape" ? owner.children.flatMap((child) => (child.tool === "timeline" || child.tool === "region" ? [child] : [])) : [])
      .sort((a, b) => a.state.start - b.state.start);
    const at = items.findIndex((item) => `${row}/${item.name}` === chosen);
    const next = items[at < 0 ? (by > 0 ? 0 : items.length - 1) : clamp(at + by, 0, items.length - 1)];
    if (!next || !row) return false;
    memory.selected = new Set([`${row}/${next.name}`]);
    memory.track = row;
    memory.point = null;
    memory.mark = null;
  });

/** Goes to the mark of the ruler after or before the chosen one, or the playhead, and chooses it: delete then removes it, enter names a section. */
const markStep = (keys: readonly ActionKey[], title: string, by: -1 | 1) =>
  on(keys, title, ({ memory, clock }) => {
    const marks = marksOf(clock);
    const chosen = memory.mark;
    const at = chosen ? marks.findIndex((mark) => mark.kind === chosen.kind && mark.tick === chosen.tick) : -1;
    const next = at >= 0 ? marks[at + by] : by > 0 ? marks.find((mark) => mark.tick >= clock.tick) : marks.findLast((mark) => mark.tick <= clock.tick);
    if (!next) return false;
    memory.selected = new Set();
    memory.point = null;
    memory.mark = next;
    clock.jump(next.tick);
  });

export const songActions: Record<string, SongAction> = {
  play: playPause,
  record: on(["r"], "Record", (tool) => toggleRecordingOf(tool, tool.state.order)),
  add_tempo: on(["t"], "Add a tempo change at the playhead", ({ clock, memory }) => addTempoAtPlayhead(clock, memory)),
  add_signature: on(["shift-t"], "Add a time signature change at the playhead", ({ clock, memory }) => addSignatureNear(clock, memory, clock.tick)),
  add_section: on(["shift-s"], "Add a section at the playhead", ({ clock, memory }) => addSection(clock, memory, clock.tick)),
  next_mark: markStep(["alt-shift-right"], "Go to the next section, tempo or time signature change", 1),
  previous_mark: markStep(["alt-shift-left"], "Go to the section, tempo or time signature change before", -1),
  back: on(["escape"], "Let go of what is chosen, or close what is open", back),

  select_all_notes: notes(["cmd-a"], "Choose all notes", "all"),
  paste_notes: notes(["cmd-v"], "Paste notes", "paste"),
  quantize_notes: notes(["q"], "Quantize the chosen notes, or all of them", "quantize"),
  delete_notes: notes(["backspace", "delete"], "Delete the chosen notes", "delete"),
  copy_notes: notes(["cmd-c"], "Copy the chosen notes", "copy"),
  cut_notes: notes(["cmd-x"], "Cut the chosen notes", "cut"),
  duplicate_notes: notes(["cmd-d"], "Duplicate the chosen notes", "duplicate"),
  notes_louder: notes(["alt-up"], "Make the chosen notes louder", "louder"),
  notes_softer: notes(["alt-down"], "Make the chosen notes softer", "softer"),
  notes_left: notes(["left"], "Move the chosen notes earlier", "left"),
  notes_right: notes(["right"], "Move the chosen notes later", "right"),
  notes_up: notes(["up"], "Move the chosen notes up", "up"),
  notes_down: notes(["down"], "Move the chosen notes down", "down"),
  notes_up_octave: notes(["shift-up"], "Move the chosen notes up an octave", "octave-up"),
  notes_down_octave: notes(["shift-down"], "Move the chosen notes down an octave", "octave-down"),

  name_section: on(["enter"], "Name the chosen section", ({ memory }) => {
    const mark = memory.mark;
    if (!mark) return false;
    memory.mark = null;
    memory.editing = mark.kind === "section" ? mark : null;
  }),
  delete_mark: on(["backspace", "delete"], "Remove the chosen section, tempo change or time signature", ({ memory, clock }) => {
    const mark = memory.mark;
    if (!mark) return false;
    memory.mark = null;
    removeMark(clock, mark);
  }),
  delete_point: on(["backspace", "delete"], "Remove the chosen automation point", (tool) => {
    if (!tool.memory.point) return false;
    const { track, lane, tick } = tool.memory.point;
    tool.memory.point = null;
    editPoints(tool, "Remove a point", track, lane, (points) => points.filter((point) => point.tick !== tick));
  }),
  audio_louder: regionGain(["alt-up"], "Turn the chosen audio clips up", 1),
  audio_softer: regionGain(["alt-down"], "Turn the chosen audio clips down", -1),

  rename_track: on(["enter"], "Rename the track", (tool) => {
    const name = shownTrack(tool);
    if (name === null) return false;
    tool.memory.renaming = name;
  }),
  delete_track: on(["backspace", "delete"], "Delete the track", (tool) => {
    const name = shownTrack(tool);
    if (name === null) return false;
    void askToDelete(tool, name);
  }),
  track_above: trackStep(["up"], "Choose the track above", -1, false),
  track_below: trackStep(["down"], "Choose the track below", 1, false),
  move_track_up: trackStep(["alt-up"], "Move the track up", -1, true),
  move_track_down: trackStep(["alt-down"], "Move the track down", 1, true),

  next_clip: clipStep(["alt-right"], "Choose the next clip", 1),
  previous_clip: clipStep(["alt-left"], "Choose the clip before", -1),
  clip_menu: on(["alt-enter"], "Open the menu of the chosen clip", ({ memory }) => {
    const [chosen] = memory.selected;
    if (chosen === undefined) return false;
    memory.clipMenu = chosen;
  }),
  delete_clips: clips(["backspace", "delete"], "Delete the chosen clips", "delete"),
  cut_clips: clips(["cmd-x"], "Cut the chosen clips", "cut"),
  copy_clips: clips(["cmd-c"], "Copy the chosen clips", "copy"),
  duplicate_clips: clips(["cmd-d"], "Duplicate the chosen clips", "duplicate"),
  open_clip: on(["enter", "cmd-down"], "Edit the notes of the chosen clip", ({ memory, children }) => {
    const clip = [...memory.selected].find((each) => itemTool(children, each) === "timeline");
    if (!clip) return false;
    memory.below = { kind: "editor", clip };
    memory.notes = new Set();
  }),
  select_all_clips: on(["cmd-a"], "Choose all clips", ({ memory, children, state }) => {
    memory.selected = new Set(rowsOf(children, state.order).flatMap(({ name }) => {
      const owner = children.find((child) => child.name === name);
      const items = owner?.tool === "track" || owner?.tool === "tape" ? owner.children.filter((child) => child.tool === "timeline" || child.tool === "region") : [];
      return items.map((child) => `${name}/${child.name}`);
    }));
  }),
  // At the playhead, the top one on the track of the first chosen, else on the chosen track,
  // else on the first; a chosen return takes none.
  paste_clips: on(["cmd-v"], "Paste clips at the playhead", ({ memory, children, state, clock, update }) => {
    const clipboard = memory.clipboard;
    if (!clipboard) return false;
    const rows = rowsOf(children, state.order);
    const first = [...memory.selected][0]?.split("/")[0] ?? memory.track;
    const top = first === null ? 0 : rows.findIndex((row) => row.name === first);
    const at = Math.round(clock.tick);
    const placed = top < 0 ? null : placing(rows, clipboard, (each) => ({ row: top + each.row, start: at + each.item.state.start }));
    if (!placed) return;
    let pasted = new Set<string>();
    update("Paste clips", (part) => (pasted = putTaken(part, placed)));
    memory.selected = pasted;
  }),
  clips_left: nudge(["left"], "Move the chosen clips earlier", -1),
  clips_right: nudge(["right"], "Move the chosen clips later", 1),
  clips_up: toRow(["up"], "Move the chosen clips to the track above", -1),
  clips_down: toRow(["down"], "Move the chosen clips to the track below", 1),
};
