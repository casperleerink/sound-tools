import type { Piece } from "./piece";
import { addSection, deleteSection, duplicateSection, joinSection, moveSection, renameSection } from "./piece-edits";
import { barAtTick, barsOfPiece, barsOfSection, chooseBars } from "./piece-page";

// The sections of the piece: choosing one, naming it, and its menu, which duplicates, moves,
// joins or deletes it.

const idsOf = (piece: Pick<Piece, "state">) => piece.state.sections.map((section) => section.id);

/** The section of the first chosen bar, or of the bar the playhead is in. */
export function currentSection(piece: Pick<Piece, "state" | "memory" | "clock">) {
  const all = barsOfPiece(piece.state);
  return all.bars[piece.memory.chosen?.from ?? barAtTick(all, piece.clock.tick)]?.id ?? all.bars[0]?.id;
}
/** Chooses all the bars of the section `id`. */
export function chooseSection(piece: Pick<Piece, "state" | "memory" | "clock">, id: string) {
  const bars = barsOfSection(barsOfPiece(piece.state), id);
  if (bars) chooseBars(piece, bars.from, bars.to);
}
/** Chooses the first bar of the section before the current one, or of the next. */
export function stepSection(piece: Piece, by: -1 | 1) {
  const ids = idsOf(piece);
  const next = ids[ids.indexOf(currentSection(piece) ?? "") + by];
  const bars = next && barsOfSection(barsOfPiece(piece.state), next);
  if (bars) chooseBars(piece, bars.from, bars.from);
}
/** Moves the section `id` one place earlier or later, and chooses it. */
export function moveSectionBy(piece: Piece, id: string, by: -1 | 1) {
  const ids = idsOf(piece);
  const at = ids.indexOf(id);
  if (at < 0 || at + by < 0 || at + by >= ids.length) return;
  moveSection(piece, id, by < 0 ? ids[at - 1]! : (ids[at + 2] ?? null));
  chooseSection(piece, id);
}
/** Opens the name of the section `id` to type, where the pointer pressed last; no name keeps the one it has. */
export async function nameSection(piece: Piece, id: string) {
  const name = piece.state.sections.find((section) => section.id === id)?.name ?? "";
  const typed = await piece.openField({ value: name, placeholder: "Name of the section", width: 200 });
  if (typed !== null && typed !== name) renameSection(piece, id, typed);
}
/**
 * Adds a section of eight bars at the end of the piece, chosen, with its name to type. `here` is
 * a press on the button after the last bar: the new name is then under the pointer, where the
 * field opens, so the page stays put until it is typed.
 */
export async function addSectionAtEnd(piece: Piece, here = false) {
  const id = addSection(piece, null);
  if (!here) chooseSection(piece, id);
  await nameSection(piece, id);
  if (here) chooseSection(piece, id);
}

export async function sectionMenu(piece: Piece, id: string) {
  chooseSection(piece, id);
  const ids = idsOf(piece);
  const at = ids.indexOf(id);
  const last = at === ids.length - 1;
  const picked = await piece.openMenu([
    { label: "Rename", shortcut: "piece.rename_section" },
    { label: "Duplicate" },
    { label: "Move earlier", shortcut: "piece.move_section_earlier", disabled: at === 0, separator: true },
    { label: "Move later", shortcut: "piece.move_section_later", disabled: last },
    { label: "Join with the next", disabled: last },
    { label: "Delete", disabled: ids.length <= 1, separator: true },
  ]);
  if (picked === 0) void nameSection(piece, id);
  else if (picked === 1) {
    const copy = duplicateSection(piece, id);
    if (copy) chooseSection(piece, copy);
  } else if (picked === 2 || picked === 3) moveSectionBy(piece, id, picked === 2 ? -1 : 1);
  else if (picked === 4) {
    joinSection(piece, id);
    chooseSection(piece, id);
  } else if (picked === 5) {
    deleteSection(piece, id);
    piece.memory.chosen = null;
  }
}
