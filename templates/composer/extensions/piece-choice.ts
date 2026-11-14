import type { MenuItem } from "./sdk";
import type { Piece } from "./piece";
import { addBars, clearMusic, copyOf, duplicateBars, paste, removeBars, splitSection } from "./piece-edits";
import { chooseBars, clamp, instrumentsOf, openAt, openOf, showEditor, showInstrument } from "./piece-page";

// What the chosen bars of the open section do, whole or of some instruments: as in a score, a
// passage is chosen, then cleared, copied, pasted or repeated.

/** The choice in the open section, kept inside its bars, its instruments in the order of the rows: none while nothing is chosen. */
export function choiceOf(piece: Pick<Piece, "state" | "memory" | "children">) {
  const [open, chosen] = [openOf(piece), piece.memory.chosen];
  if (!open || !chosen) return undefined;
  const last = open.bars.length - 1;
  const names = instrumentsOf(piece).map((each) => each.name);
  const rows = chosen.rows === null ? null : names.filter((name) => chosen.rows!.includes(name));
  if (rows?.length === 0) return undefined;
  return { open, from: Math.min(chosen.from, last), to: Math.min(chosen.to, last), rows, names };
}
type Choice = NonNullable<ReturnType<typeof choiceOf>>;
const isWhole = (choice: Choice) => choice.rows === null;
const isAll = (choice: Choice) => choice.from === 0 && choice.to === choice.open.bars.length - 1;

/** What the choice holds, its instruments in the order of the rows, so a paste puts them back in that order. */
function copied(piece: Piece, choice: Choice) {
  const copy = copyOf(piece, choice.open.id, choice);
  copy.rows.sort((a, b) => choice.names.indexOf(a.name) - choice.names.indexOf(b.name));
  return copy;
}

export function copyChosen(piece: Piece): boolean {
  const choice = choiceOf(piece);
  if (!choice) return false;
  piece.memory.copied = copied(piece, choice);
  return true;
}
/** Takes the chosen music out: whole bars go, unless they are all the section has; of some instruments, it is cleared. */
export function clearChosen(piece: Piece, whole: boolean): boolean {
  const choice = choiceOf(piece);
  if (!choice || isWhole(choice) !== whole) return false;
  if (whole && isAll(choice)) return true;
  if (whole) {
    removeBars(piece, choice.open.id, choice.from, choice.to);
    chooseBars(piece, choice.from, choice.from);
  } else clearMusic(piece, choice.open.id, choice);
  return true;
}
export function cutChosen(piece: Piece): boolean {
  const choice = choiceOf(piece);
  if (!choice || !copyChosen(piece)) return false;
  if (isWhole(choice) && !isAll(choice)) clearChosen(piece, true);
  else clearMusic(piece, choice.open.id, choice);
  return true;
}
/** Pastes what is copied: before the chosen bars, or over the chosen instruments from the first chosen bar on. What it pasted is chosen. */
export function pasteChosen(piece: Piece): boolean {
  const [choice, copy] = [choiceOf(piece), piece.memory.copied];
  if (!choice || !copy) return false;
  paste(piece, choice.open.id, choice, copy, choice.names);
  const to = choice.from + copy.bars.length - 1;
  const first = choice.rows === null ? -1 : choice.names.indexOf(choice.rows[0]!);
  const rows = choice.names.slice(first, first + copy.rows.length);
  chooseBars(piece, choice.from, to, first < 0 || rows.length === 0 ? null : rows);
  return true;
}
/** The chosen bars again right after them, with their music; of some instruments, their music over the bars after it, which the section gets where it has too few. The copy is chosen. */
export function repeatChosen(piece: Piece): boolean {
  const choice = choiceOf(piece);
  if (!choice) return false;
  const { open, from, to, rows } = choice;
  const after = to + 1;
  if (rows === null) duplicateBars(piece, open.id, from, to);
  else paste(piece, open.id, { from: after, to: after, rows }, copied(piece, choice), choice.names);
  chooseBars(piece, after, after + to - from, rows);
  return true;
}
/** Opens the notes of the first chosen instrument, or of the chosen one, else of the first. */
export function editChosen(piece: Piece): boolean {
  const choice = choiceOf(piece);
  const name = choice && (choice.rows?.[0] ?? choice.names.find((each) => each === piece.memory.track) ?? choice.names[0]);
  if (!name) return false;
  showEditor(piece, name);
  return true;
}

/** Chooses the bar before or after the chosen ones, of the same instruments, or with `extend` takes it in too. With none chosen, the bar the playhead is in, or the first. */
export function stepBars(piece: Piece, by: -1 | 1, extend: boolean): boolean {
  const choice = choiceOf(piece);
  if (!choice) {
    const open = openOf(piece);
    if (!open) return false;
    const into = piece.clock.tick - open.place.start;
    const bar = into >= 0 && into < open.place.length ? open.starts.findLastIndex((start) => start <= into) : 0;
    chooseBars(piece, bar, bar);
    return true;
  }
  const next = by < 0 ? choice.from - 1 : choice.to + 1;
  if (extend) chooseBars(piece, Math.min(choice.from, next), Math.max(choice.to, next), choice.rows);
  else chooseBars(piece, next, next, choice.rows);
  return true;
}
/**
 * Moves the chosen music to the instruments above or below, or with `extend` takes in the one
 * there too. Up from the first instrument chooses the whole bars, down from them the first.
 */
export function stepRows(piece: Piece, by: -1 | 1, extend: boolean): boolean {
  const choice = choiceOf(piece);
  if (!choice) return false;
  const { from, to, rows, names } = choice;
  if (rows === null) {
    if (by > 0 && !extend && names[0]) chooseRows(piece, from, to, [names[0]]);
    return true;
  }
  const [top, bottom] = [names.indexOf(rows[0]!), names.indexOf(rows.at(-1)!)];
  if (extend) {
    const [first, last] = by < 0 ? [top - 1, bottom] : [top, bottom + 1];
    if (first >= 0 && last < names.length) chooseRows(piece, from, to, names.slice(first, last + 1));
  } else if (by < 0 && top === 0) chooseBars(piece, from, to);
  else {
    const first = clamp(top + by, 0, names.length - rows.length);
    chooseRows(piece, from, to, names.slice(first, first + rows.length));
  }
  return true;
}
/** Chooses the bars `from` to `to` of the instruments `rows`, and the first of them. */
export function chooseRows(piece: Piece, from: number, to: number, rows: readonly string[]) {
  chooseBars(piece, from, to, rows);
  if (rows[0]) showInstrument(piece, rows[0]);
}

/** Adds a bar `after` the chosen ones, or `before` them, or at the `end` of the section, as with none chosen: it is chosen. */
export function addBar(piece: Piece, where: "before" | "after" | "end") {
  const open = openOf(piece);
  if (!open) return;
  const choice = choiceOf(piece);
  const at = !choice || where === "end" ? open.bars.length : where === "before" ? choice.from : choice.to + 1;
  addBars(piece, open.id, at);
  chooseBars(piece, at, at);
}
/** A new section from the first chosen bar on, opened with its name to type; none at the first bar. */
export function startSectionAt(piece: Piece): boolean {
  const choice = choiceOf(piece);
  const id = choice && choice.from > 0 ? splitSection(piece, choice.open.id, choice.from) : undefined;
  if (!id) return false;
  openAt(piece, id);
  piece.memory.naming = { id, in: "head" };
  return true;
}

/** Loops the chosen bars as the piece plays, or the open section while none are; again, it plays on. */
export function toggleLoop(piece: Piece) {
  const { clock } = piece;
  const open = openOf(piece);
  if (clock.repeat !== null || !open) return clock.setRepeat(null);
  const choice = choiceOf(piece);
  const [from, to] = choice ? [choice.from, choice.to] : [0, open.bars.length - 1];
  clock.setRepeat({ from: open.place.start + open.starts[from]!, to: open.place.start + open.starts[to + 1]! });
}

/** The menu of the choice, each item with its key: whole bars also add a bar or start a section, music opens its notes. */
export async function choiceMenu(piece: Piece) {
  const choice = choiceOf(piece);
  if (!choice) return;
  const whole = isWhole(choice);
  type Item = MenuItem & { run: () => unknown };
  const items: Item[] = [
    { label: "Cut", shortcut: "piece.cut", run: () => cutChosen(piece) },
    { label: "Copy", shortcut: "piece.copy", run: () => copyChosen(piece) },
    { label: whole ? "Paste as new bars" : "Paste", shortcut: "piece.paste", disabled: piece.memory.copied === null, run: () => pasteChosen(piece) },
    { label: whole ? "Repeat as new bars" : "Repeat", shortcut: "piece.repeat_music", run: () => repeatChosen(piece) },
    whole
      ? { label: "Remove", shortcut: "piece.remove_bars", disabled: isAll(choice), run: () => clearChosen(piece, true) }
      : { label: "Clear", shortcut: "piece.clear_music", run: () => clearChosen(piece, false) },
    ...(whole
      ? ([
          { label: "Add a bar before", shortcut: "piece.add_bar_before", separator: true, run: () => addBar(piece, "before") },
          { label: "Add a bar after", shortcut: "piece.add_bar", run: () => addBar(piece, "after") },
          { label: "Start a section here", shortcut: "piece.add_section", disabled: choice.from === 0, separator: true, run: () => startSectionAt(piece) },
        ] satisfies Item[])
      : ([{ label: "Edit notes", shortcut: "piece.edit_notes", separator: true, run: () => editChosen(piece) }] satisfies Item[])),
  ];
  const picked = await piece.openMenu(items.map(({ run: _, ...item }) => item));
  if (picked !== null) items[picked]?.run();
}
