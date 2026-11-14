import { type Action, type ActionKey } from "./sdk";
import { noteCommand, type NoteCommand } from "./clip-editor";
import type { Piece } from "./piece";
import { clamp, editedPart, editPart, openSectionOf } from "./piece-page";
import { chosenEffect, stepEffect } from "./piece-panel";
import { askToDelete } from "./piece-tracks";

// What the piece does on a key. The actions of one key are tried from the top: the notes of the
// open part first, then the instrument and the effect. Each says `false` while what it works on
// is not chosen, and the key goes on to the next.

type PieceAction = Action<Piece>;

/** An action of the piece. `run` says `false` when there was nothing for it to do. */
const on = (keys: readonly ActionKey[], title: string, run: (tool: Piece) => boolean | void): PieceAction => ({ title, keys, run });

/** Does `command` to the notes of the part the editor shows; whether there was something to do. */
function onNotes(tool: Piece, command: NoteCommand): boolean {
  const part = editedPart(tool);
  const section = openSectionOf(tool);
  const below = tool.memory.below;
  if (!part || !section || below?.kind !== "editor") return false;
  return noteCommand(command, part, tool.clock.tick, tool.memory, (label, change) => editPart(tool, below.track, section.id, label, change));
}

const notes = (keys: readonly ActionKey[], title: string, command: NoteCommand) => on(keys, title, (tool) => onNotes(tool, command));

/** Escape: closes the editor with no note chosen, else lets go of the chosen notes, effect or bars, else closes what shows below. */
function back(tool: Piece) {
  const { memory } = tool;
  const below = memory.below;
  if (below?.kind === "editor" && memory.notes.size === 0) memory.below = { kind: "track", name: below.track };
  else if (onNotes(tool, "none")) return;
  else if (chosenEffect(tool)) memory.effect = null;
  else if (memory.chosen) memory.chosen = null;
  else if (below) memory.below = null;
}

/** The instrument or return whose panel shows while no bars are chosen. */
const shownTrack = ({ memory }: Piece) => (memory.chosen === null && memory.below?.kind === "track" ? memory.below.name : null);

/** Chooses the instrument above or below the one whose panel shows, or with `move` puts it there. */
const trackStep = (keys: readonly ActionKey[], title: string, by: -1 | 1, move: boolean) =>
  on(keys, title, (tool) => {
    const { children, memory, select, state, update } = tool;
    const name = shownTrack(tool);
    const rank = (each: string) => (state.order.includes(each) ? state.order.indexOf(each) : state.order.length);
    const names = children
      .filter((child) => child.tool === "track")
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
      update("Move an instrument", (piece) => (piece.state.order = names));
    }
  });

/** Moves the chosen effect a step earlier or later in the chain of its instrument. */
const effectStep = (keys: readonly ActionKey[], title: string, by: -1 | 1) =>
  on(keys, title, (tool) => {
    const chosen = chosenEffect(tool);
    if (!chosen) return false;
    stepEffect(tool, chosen.track, chosen.name, by);
  });

export const pieceActions: Record<string, PieceAction> = {
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
  editor_size: on(["shift-enter"], "Make the note editor large, or put it back", ({ memory }) => {
    if (memory.below?.kind !== "editor") return false;
    memory.editorLarge = !memory.editorLarge;
  }),

  rename_instrument: on(["enter"], "Rename the instrument", (tool) => {
    const name = shownTrack(tool);
    if (name === null) return false;
    tool.memory.renaming = name;
  }),
  delete_instrument: on(["backspace", "delete"], "Delete the instrument", (tool) => {
    const name = shownTrack(tool);
    if (name === null) return false;
    void askToDelete(tool, name);
  }),
  instrument_above: trackStep(["up"], "Choose the instrument above", -1, false),
  instrument_below: trackStep(["down"], "Choose the instrument below", 1, false),
  move_instrument_up: trackStep(["alt-up"], "Move the instrument up", -1, true),
  move_instrument_down: trackStep(["alt-down"], "Move the instrument down", 1, true),

  effect_earlier: effectStep(["alt-left"], "Move the chosen effect earlier", -1),
  effect_later: effectStep(["alt-right"], "Move the chosen effect later", 1),
};
