import { type Action, type ActionKey } from "./sdk";
import { noteCommand, type NoteCommand } from "./clip-editor";
import type { Piece } from "./piece";
import { addBar, clearChosen, copyChosen, cutChosen, editChosen, pasteChosen, repeatChosen, startSectionAt, stepBars, stepRows, toggleRepeat } from "./piece-choice";
import { chooseBars, clamp, editedPart, editPart, instrumentsOf, openOf, openSectionOf, showPanel } from "./piece-page";
import { chosenEffect, stepEffect } from "./piece-panel";
import { addSectionAfter, moveSectionBy, stepSection } from "./piece-sections";
import { askToDelete } from "./piece-tracks";

// What the piece does on a key. The actions of one key are tried from the top: the notes of the
// open part first, then the bars and the sections, then the instrument and the effect. Each says
// `false` while what it works on is not chosen, and the key goes on to the next.

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

/** Escape: lets go of the chosen notes, effect or bars, else closes the editor, else what shows below. */
function back(tool: Piece) {
  const { memory } = tool;
  const below = memory.below;
  if (below?.kind === "editor" && memory.notes.size > 0) onNotes(tool, "none");
  else if (chosenEffect(tool)) memory.effect = null;
  else if (memory.chosen) memory.chosen = null;
  else if (below?.kind === "editor") memory.below = { kind: "track", name: below.track };
  else if (below) memory.below = null;
}

/** The instrument or return whose panel shows while no bars are chosen. */
const shownTrack = ({ memory }: Piece) => (memory.chosen === null && memory.below?.kind === "track" ? memory.below.name : null);

/** Chooses the instrument above or below the one whose panel shows, or with `move` puts it there. */
const trackStep = (keys: readonly ActionKey[], title: string, by: -1 | 1, move: boolean) =>
  on(keys, title, (tool) => {
    const name = shownTrack(tool);
    const names = instrumentsOf(tool).map((each) => each.name);
    const at = name === null ? -1 : names.indexOf(name);
    if (name === null || at < 0) return false;
    const next = clamp(at + by, 0, names.length - 1);
    if (!move) showPanel(tool, names[next]!);
    else if (next !== at) {
      names.splice(at, 1);
      names.splice(next, 0, name);
      tool.update("Move an instrument", (piece) => (piece.state.order = names));
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
  add_note: notes(["n"], "Add a note", "add"),
  // With bars chosen and no note, a paste goes to the bars.
  paste_notes: on(["cmd-v"], "Paste notes", (tool) => (tool.memory.chosen === null || tool.memory.notes.size > 0) && onNotes(tool, "paste")),
  quantize_notes: notes(["q"], "Quantize notes", "quantize"),
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

  remove_bars: on(["backspace", "delete"], "Remove the chosen bars", (tool) => clearChosen(tool, true)),
  clear_music: on(["backspace", "delete"], "Clear the chosen music", (tool) => clearChosen(tool, false)),
  cut: on(["cmd-x"], "Cut the chosen bars or music", cutChosen),
  copy: on(["cmd-c"], "Copy the chosen bars or music", copyChosen),
  paste: on(["cmd-v"], "Paste before the chosen bars, or over the chosen music", pasteChosen),
  repeat_music: on(["cmd-d"], "Repeat the chosen bars or music right after them", repeatChosen),
  edit_notes: on(["enter"], "Edit the notes of the chosen music", editChosen),
  select_all_bars: on(["cmd-a"], "Choose all bars of the section", (tool) => {
    const open = openOf(tool);
    if (!open) return false;
    chooseBars(tool, 0, open.bars.length - 1);
  }),
  bar_before: on(["left"], "Choose the bar before", (tool) => stepBars(tool, -1, false)),
  bar_after: on(["right"], "Choose the next bar", (tool) => stepBars(tool, 1, false)),
  bars_before: on(["shift-left"], "Choose the bar before too", (tool) => stepBars(tool, -1, true)),
  bars_after: on(["shift-right"], "Choose the next bar too", (tool) => stepBars(tool, 1, true)),
  music_above: on(["up"], "Choose the music of the instrument above, or the whole bars", (tool) => stepRows(tool, -1, false)),
  music_below: on(["down"], "Choose the music of the instrument below", (tool) => stepRows(tool, 1, false)),
  more_above: on(["shift-up"], "Choose the instrument above too", (tool) => stepRows(tool, -1, true)),
  more_below: on(["shift-down"], "Choose the instrument below too", (tool) => stepRows(tool, 1, true)),
  add_bar: on(["shift-b"], "Add a bar", (tool) => addBar(tool, false)),
  add_bar_before: on(["alt-shift-b"], "Add a bar before", (tool) => addBar(tool, true)),
  repeat: on(["l"], "Repeat the chosen bars, or the section, as it plays", toggleRepeat),

  previous_section: on(["alt-shift-left"], "Section before", (tool) => stepSection(tool, -1)),
  next_section: on(["alt-shift-right"], "Next section", (tool) => stepSection(tool, 1)),
  move_section_earlier: on(["alt-shift-up"], "Move the section earlier", (tool) => moveSectionBy(tool, openSectionOf(tool)?.id ?? "", -1)),
  move_section_later: on(["alt-shift-down"], "Move the section later", (tool) => moveSectionBy(tool, openSectionOf(tool)?.id ?? "", 1)),
  rename_section: on(["shift-r"], "Rename the section", (tool) => {
    const id = openSectionOf(tool)?.id;
    if (id) tool.memory.naming = { id, in: "head" };
  }),
  add_section: on(["shift-s"], "Start a section here, or add one", (tool) => {
    if (!startSectionAt(tool)) addSectionAfter(tool, "head");
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
