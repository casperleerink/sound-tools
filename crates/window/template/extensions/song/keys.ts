// What the song does on a key. The actions of one key are tried from the top: the notes of the
// open clip first, then the chosen mark of the ruler, point of a lane, regions, track, effect and
// clips. Each says `false` while what it works on is not chosen, and the key goes on to the next.
import type { Action, ActionKey } from "../sdk";
import { editsOf } from "./edits";
import { chosenEffect } from "./effects";
import { itemAt, rowOfKey, type Song } from "./model";
import { noteCommand, openClip, type NoteCommand } from "./notes";

/** An action of the song. `run` says `false` when there was nothing for it to do. */
const on = (keys: readonly ActionKey[], title: string, run: (song: Song) => boolean | void): Action<Song> => ({ title, keys, run });
/** As the transport's keys: also while the keys are outside the page, such as in the history. */
const everywhere = (action: Action<Song>): Action<Song> => ({ ...action, everywhere: true });
const notes = (keys: readonly ActionKey[], title: string, command: NoteCommand) => on(keys, title, (song) => noteCommand(song, command));

/** Escape: closes the editor with no note chosen, else lets go of the chosen notes, effect, mark, point or clips, else closes what shows below. */
function back(song: Song) {
  const { memory } = song;
  const opened = openClip(song);
  if (opened && memory.notes.length === 0) memory.below = { kind: "track", name: rowOfKey(opened) };
  else if (noteCommand(song, "none")) return;
  else if (chosenEffect(song)) memory.effect = null;
  else if (memory.mark) memory.mark = null;
  else if (memory.point) memory.point = null;
  else if (memory.selected.length > 0) memory.selected = [];
  else if (memory.below) memory.below = null;
}

export const actions: Record<string, Action<Song>> = {
  record: everywhere(on(["r"], "Record", (song) => editsOf(song).tracks.record())),
  add_tempo: on(["t"], "Add a tempo change at the playhead", (song) => editsOf(song).marks.addTempoAtPlayhead()),
  add_signature: on(["shift-t"], "Add a time signature change at the playhead", (song) => editsOf(song).marks.addSignature(song.clock.tick)),
  add_section: on(["shift-s"], "Add a section at the playhead", (song) => editsOf(song).marks.addSection(song.clock.tick)),
  next_mark: everywhere(on(["alt-shift-right"], "Go to the next section, tempo or time signature change", (song) => editsOf(song).marks.step(1))),
  previous_mark: everywhere(on(["alt-shift-left"], "Go to the section, tempo or time signature change before", (song) => editsOf(song).marks.step(-1))),
  back: on(["escape"], "Let go of what is chosen, or close what is open", back),

  select_all_notes: notes(["cmd-a"], "Choose all notes", "all"),
  add_note: notes(["n"], "Add a note after the chosen ones, or at the playhead", "add"),
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
    memory.renamingSection = mark.kind === "section" ? { bar: mark.bar, tick: mark.tick } : null;
  }),
  delete_mark: on(["backspace", "delete"], "Remove the chosen section, tempo change or time signature", (song) => {
    if (!song.memory.mark) return false;
    editsOf(song).marks.remove(song.memory.mark);
  }),
  delete_point: on(["backspace", "delete"], "Remove the chosen automation point", (song) => {
    const point = song.memory.point;
    if (!point) return false;
    editsOf(song).lanes.removePoint(point.track, point.lane, point.tick);
  }),
  audio_louder: on(["alt-up"], "Turn the chosen audio clips up", (song) => editsOf(song).regions.step(1)),
  audio_softer: on(["alt-down"], "Turn the chosen audio clips down", (song) => editsOf(song).regions.step(-1)),

  rename_track: on(["enter"], "Rename the track", (song) => {
    const name = editsOf(song).tracks.shown();
    if (name === null) return false;
    song.memory.renaming = name;
  }),
  delete_track: on(["backspace", "delete"], "Delete the track", (song) => {
    const tracks = editsOf(song).tracks;
    const name = tracks.shown();
    if (name === null) return false;
    void tracks.askToRemove(name);
  }),
  track_above: on(["up"], "Choose the track above", (song) => editsOf(song).tracks.step(-1, false)),
  track_below: on(["down"], "Choose the track below", (song) => editsOf(song).tracks.step(1, false)),
  move_track_up: on(["alt-up"], "Move the track up", (song) => editsOf(song).tracks.step(-1, true)),
  move_track_down: on(["alt-down"], "Move the track down", (song) => editsOf(song).tracks.step(1, true)),

  effect_earlier: on(["alt-left"], "Move the chosen effect earlier", (song) => {
    const chosen = chosenEffect(song);
    if (!chosen) return false;
    editsOf(song).effects(chosen.track).step(chosen.name, -1);
  }),
  effect_later: on(["alt-right"], "Move the chosen effect later", (song) => {
    const chosen = chosenEffect(song);
    if (!chosen) return false;
    editsOf(song).effects(chosen.track).step(chosen.name, 1);
  }),

  next_clip: on(["alt-right"], "Choose the next clip", (song) => editsOf(song).clips.step(1)),
  previous_clip: on(["alt-left"], "Choose the clip before", (song) => editsOf(song).clips.step(-1)),
  clip_menu: on(["alt-enter"], "Open the menu of the chosen clip", ({ memory }) => {
    const [chosen] = memory.selected;
    if (chosen === undefined) return false;
    memory.menu = chosen;
  }),
  add_clip: on(["shift-c"], "Add a clip at the playhead", (song) => editsOf(song).clips.addAtPlayhead()),
  delete_clips: on(["backspace", "delete"], "Delete the chosen clips", (song) => editsOf(song).clips.command("delete")),
  cut_clips: on(["cmd-x"], "Cut the chosen clips", (song) => editsOf(song).clips.command("cut")),
  copy_clips: on(["cmd-c"], "Copy the chosen clips", (song) => editsOf(song).clips.command("copy")),
  duplicate_clips: on(["cmd-d"], "Duplicate the chosen clips", (song) => editsOf(song).clips.command("duplicate")),
  open_clip: on(["enter", "cmd-down"], "Edit the notes of the chosen clip", (song) => {
    const clip = song.memory.selected.find((key) => itemAt(song.children, key)?.tool === "timeline");
    if (!clip) return false;
    editsOf(song).clips.open(clip);
  }),
  editor_size: on(["shift-enter"], "Make the note editor large, or put it back", ({ memory }) => {
    if (memory.below?.kind !== "editor") return false;
    memory.editorLarge = !memory.editorLarge;
  }),
  select_all_clips: on(["cmd-a"], "Choose all clips", (song) => editsOf(song).clips.all()),
  paste_clips: on(["cmd-v"], "Paste clips at the playhead", (song) => editsOf(song).clips.paste()),
  clips_left: on(["left"], "Move the chosen clips earlier", (song) => editsOf(song).clips.nudge(-1)),
  clips_right: on(["right"], "Move the chosen clips later", (song) => editsOf(song).clips.nudge(1)),
  clips_up: on(["up"], "Move the chosen clips to the track above", (song) => editsOf(song).clips.toRow(-1)),
  clips_down: on(["down"], "Move the chosen clips to the track below", (song) => editsOf(song).clips.toRow(1)),
};
