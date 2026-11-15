import type { MenuItem } from "./sdk";
import type { Piece } from "./piece";
import { addBars, clearMusic, copyOf, duplicateBars, paste, removeBars, setFermatas, setSignature, setSlide, setTempo, splitSection } from "./piece-edits";
import { beatOf, countOf, endOf, formText, longestHold, partsOf, startsOf, tempoOf, type Bar } from "./piece-form";
import { barAtTick, barsOfPiece, barsOfSection, chooseBars, clamp, instrumentsOf, showEditor, showInstrument, spansOf } from "./piece-page";
import { nameSection, sectionMenu } from "./piece-sections";

// What the chosen bars of the piece do, whole or of some instruments: as in a score, a passage is
// chosen, then cleared, copied, pasted or repeated. A choice may cross section lines.

/** The choice, kept inside the piece, its instruments in the order of the rows, with the sections it covers: none while nothing is chosen. */
export function choiceOf(piece: Pick<Piece, "state" | "memory" | "children">) {
  const chosen = piece.memory.chosen;
  const all = barsOfPiece(piece.state);
  const last = all.bars.length - 1;
  if (!chosen || last < 0) return undefined;
  const names = instrumentsOf(piece).map((each) => each.name);
  const rows = chosen.rows === null ? null : names.filter((name) => chosen.rows!.includes(name));
  if (rows?.length === 0) return undefined;
  const [from, to] = [Math.min(chosen.from, last), Math.min(chosen.to, last)];
  return { all, from, to, rows, names, spans: spansOf(all, from, to) };
}
type Choice = NonNullable<ReturnType<typeof choiceOf>>;
const isWhole = (choice: Choice) => choice.rows === null;
const isAll = (choice: Choice) => choice.from === 0 && choice.to === choice.all.bars.length - 1;
/** The section and its bar a paste at the bar `bar` of the piece goes to: past the last, the end of the last section. */
const placeOf = ({ all }: Choice, bar: number) => {
  const at = all.bars[bar];
  if (at) return { id: at.id, bar: at.at };
  const last = all.bars.at(-1)!;
  return { id: last.id, bar: last.at + 1 };
};

/** What the choice holds, its instruments in the order of the rows, so a paste puts them back in that order. */
function copied(piece: Piece, choice: Choice) {
  const copy = copyOf(piece, choice.spans, choice.rows);
  copy.rows.sort((a, b) => choice.names.indexOf(a.name) - choice.names.indexOf(b.name));
  return copy;
}

export function copyChosen(piece: Piece): boolean {
  const choice = choiceOf(piece);
  if (!choice) return false;
  piece.memory.copied = copied(piece, choice);
  return true;
}
/** Takes the chosen music out: whole bars go, unless they are all the piece has; of some instruments, it is cleared. */
export function clearChosen(piece: Piece, whole: boolean): boolean {
  const choice = choiceOf(piece);
  if (!choice || isWhole(choice) !== whole) return false;
  if (whole && isAll(choice)) return true;
  if (whole) {
    removeBars(piece, choice.spans);
    chooseBars(piece, choice.from, choice.from);
  } else clearMusic(piece, choice.spans, choice.rows);
  return true;
}
export function cutChosen(piece: Piece): boolean {
  const choice = choiceOf(piece);
  if (!choice || !copyChosen(piece)) return false;
  if (isWhole(choice) && !isAll(choice)) clearChosen(piece, true);
  else clearMusic(piece, choice.spans, choice.rows);
  return true;
}
/** The last bar of the piece that music pasted from the bar `from` lies under: it is as long as its own bars were, which may be other bars than these. */
function barsUnder(piece: Piece, from: number, copy: { bars: readonly Bar[] }): number {
  const { starts } = barsOfPiece(piece.state);
  const end = (starts[from] ?? 0) + startsOf(copy.bars).at(-1)!;
  return Math.max(starts.findLastIndex((start) => start < end), from);
}
/** Pastes what is copied: before the chosen bars, or over the chosen instruments from the first chosen bar on. What it pasted is chosen. */
export function pasteChosen(piece: Piece): boolean {
  const [choice, copy] = [choiceOf(piece), piece.memory.copied];
  if (!choice || !copy) return false;
  paste(piece, placeOf(choice, choice.from), choice.rows, copy, choice.names);
  const first = choice.rows === null ? -1 : choice.names.indexOf(choice.rows[0]!);
  const rows = choice.names.slice(first, first + copy.rows.length);
  if (first < 0 || rows.length === 0) chooseBars(piece, choice.from, choice.from + copy.bars.length - 1);
  else chooseBars(piece, choice.from, barsUnder(piece, choice.from, copy), rows);
  return true;
}
/** The chosen bars again right after them, with their music; of some instruments, their music over the bars after it, which the piece gets where it has too few. The copy is chosen. */
export function repeatChosen(piece: Piece): boolean {
  const choice = choiceOf(piece);
  if (!choice) return false;
  const { from, to, rows } = choice;
  const after = to + 1;
  const copy = copied(piece, choice);
  if (rows === null) duplicateBars(piece, choice.spans);
  else paste(piece, placeOf(choice, after), rows, copy, choice.names);
  chooseBars(piece, after, rows === null ? after + to - from : barsUnder(piece, after, copy), rows);
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

/** Chooses the bar before or after the chosen ones, of the same instruments, or with `extend` takes it in too. With none chosen, the bar the playhead is in. */
export function stepBars(piece: Piece, by: -1 | 1, extend: boolean): boolean {
  const choice = choiceOf(piece);
  if (!choice) {
    const bar = barAtTick(barsOfPiece(piece.state), piece.clock.tick);
    chooseBars(piece, bar, bar);
    return true;
  }
  const next = by < 0 ? choice.from - 1 : choice.to + 1;
  if (next < 0 || next >= choice.all.bars.length) return true;
  if (extend) chooseBars(piece, Math.min(choice.from, next), Math.max(choice.to, next), choice.rows);
  else chooseBars(piece, next, next, choice.rows);
  return true;
}
/** Takes in the bars after the chosen ones up to the end of the section of the last, or of the next section when it ends there: a tempo or a time signature then goes on from here. */
export function chooseToEnd(piece: Piece): boolean {
  const choice = choiceOf(piece);
  if (!choice) return false;
  const { all, from, to, rows } = choice;
  const section = barsOfSection(all, all.bars[to]!.id)!;
  const next = all.bars[to + 1];
  const end = to < section.to || !next ? section.to : barsOfSection(all, next.id)!.to;
  chooseBars(piece, from, end, rows);
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

/** Adds a bar `after` the chosen ones, or `before` them, or at the `end` of the piece, as with none chosen: it is chosen. */
export function addBar(piece: Piece, where: "before" | "after" | "end") {
  const choice = choiceOf(piece);
  const { bars } = barsOfPiece(piece.state);
  const last = bars.at(-1);
  if (!last) return;
  const [bar, at] = !choice || where === "end" ? [last, bars.length] : where === "before" ? [bars[choice.from]!, choice.from] : [bars[choice.to]!, choice.to + 1];
  addBars(piece, bar.id, where === "before" ? bar.at : bar.at + 1);
  chooseBars(piece, at, at);
}
/** A new section from the first chosen bar on, with its name to type; none where a section starts already. */
export function startSectionAt(piece: Piece): boolean {
  const choice = choiceOf(piece);
  const first = choice?.all.bars[choice.from];
  const id = first && first.at > 0 ? splitSection(piece, first.id, first.at) : undefined;
  if (!choice || !id) return false;
  // The chosen bars are the new section's now: the note editor shows its part.
  piece.memory.part = id;
  void nameSection(piece, id);
  return true;
}

/** The chosen bars, or with none the bar the playhead is in, which it chooses: what a key then changes shows. */
function chosenOrPlaying(piece: Piece) {
  const choice = choiceOf(piece);
  if (choice) return { from: choice.from, to: choice.to };
  const bar = barAtTick(barsOfPiece(piece.state), piece.clock.tick);
  chooseBars(piece, bar, bar);
  return { from: bar, to: bar };
}

/** Loops the chosen bars as the piece plays, or the section the playhead is in while none are; again, it plays on. */
export function toggleLoop(piece: Piece) {
  const { clock } = piece;
  const all = barsOfPiece(piece.state);
  if (clock.repeat !== null || all.bars.length === 0) return clock.setRepeat(null);
  const choice = choiceOf(piece);
  const [from, to] = choice ? [choice.from, choice.to] : (({ from, to }) => [from, to])(barsOfSection(all, all.bars[barAtTick(all, clock.tick)]!.id)!);
  clock.setRepeat({ from: all.starts[from]!, to: all.starts[to + 1]! });
}

/** A few to pick: a longer list would cover the bars the field is for. */
const SIGNATURES = ["2/4", "3/4", "4/4", "6/8"];
/** The note values a time signature takes. */
const NOTES = [1, 2, 4, 8, 16, 32];

/** Asks for the time signature of the bars `from` to `to` of the piece, or with none of the chosen bars or the one that plays, and gives it to them. */
export async function askSignature(piece: Piece, bars: { from: number; to: number } = chosenOrPlaying(piece)) {
  const bar = barsOfPiece(piece.state).bars[bars.from];
  if (!bar) return;
  const typed = await piece.openField({ placeholder: bar.signature, suggestions: SIGNATURES, width: 84 });
  const [, beats, note] = /^\s*(\d+)\s*\/\s*(\d+)\s*$/.exec(typed ?? "") ?? [];
  if (!beats || Number(beats) < 1 || Number(beats) > 32 || !NOTES.includes(Number(note))) {
    if (typed) piece.memory.notice = { text: "A time signature is written like 3/4 or 6/8.", form: formText(piece.state.sections as Parameters<typeof formText>[0]) };
    return;
  }
  setSignature(piece, spansOf(barsOfPiece(piece.state), bars.from, bars.to), `${Number(beats)}/${Number(note)}`);
}
/** The first number of what was typed, as a tempo: none when it has no number. */
function tempoIn(typed: string | null): number | undefined {
  const [, number] = /(\d+(?:[.,]\d+)?)/.exec(typed ?? "") ?? [];
  return number ? clamp(Math.round(Number(number.replace(",", ".")) * 10) / 10, 10, 600) : undefined;
}
/** Asks for the tempo of the bars `from` to `to` of the piece, in the beat of each, or with none of the chosen bars or the one that plays, and gives it to them, steady. */
export async function askTempo(piece: Piece, bars: { from: number; to: number } = chosenOrPlaying(piece)) {
  const bar = barsOfPiece(piece.state).bars[bars.from];
  if (!bar) return;
  const tempo = tempoIn(await piece.openField({ placeholder: `${beatOf(bar.signature).sign} = ${tempoOf(bar)}`, width: 100 }));
  if (tempo !== undefined) setTempo(piece, spansOf(barsOfPiece(piece.state), bars.from, bars.to), tempo);
}
const STEADY = "Steady tempo";
/**
 * Asks for the tempo the bars `from` to `to` of the piece slow down or speed up to, in the beat of
 * the last, or with none of the chosen bars or the one that plays: they get there in a straight
 * line from the tempo the first starts at. Bars that do so already can be made steady again.
 */
export async function askSlide(piece: Piece, bars: { from: number; to: number } = chosenOrPlaying(piece)) {
  const all = barsOfPiece(piece.state);
  const last = all.bars[bars.to];
  if (!last) return;
  const slides = all.bars.slice(bars.from, bars.to + 1).some((bar) => bar.to !== undefined);
  const sign = beatOf(last.signature).sign;
  const typed = await piece.openField({ placeholder: slides ? `to ${sign} = ${tempoOf({ ...last, bpm: endOf(last) })}` : `Slow down or speed up to ${sign} =`, suggestions: slides ? [STEADY] : [], width: slides ? 140 : 220 });
  const spans = spansOf(barsOfPiece(piece.state), bars.from, bars.to);
  if (typed === STEADY) return setSlide(piece, spans, null);
  const tempo = tempoIn(typed);
  if (tempo !== undefined) setSlide(piece, spans, tempo);
}

/** The bar of the piece the tick `tick` is in, and the count of that bar, from 1. */
function countAt(all: ReturnType<typeof barsOfPiece>, tick: number) {
  const bar = barAtTick(all, tick);
  const signature = all.bars[bar]?.signature ?? "4/4";
  return { bar, beat: clamp(Math.floor((tick - all.starts[bar]!) / countOf(signature)) + 1, 1, partsOf(signature)[0]) };
}
/** The bar a fermata goes on: the one at `tick`, or with none the last chosen bar, or the one that plays. */
const fermataBar = (piece: Piece, tick?: number) => (tick === undefined ? chosenOrPlaying(piece).to : barAtTick(barsOfPiece(piece.state), tick));
/** Puts a fermata on the count at `tick`, or with none on the last count of the last chosen bar. On a bar that has one its menu opens, so a key reaches how long it holds. */
export function toggleFermata(piece: Piece, tick?: number): boolean {
  const all = barsOfPiece(piece.state);
  const bar = all.bars[fermataBar(piece, tick)];
  if (!bar) return false;
  const beat = tick === undefined ? partsOf(bar.signature)[0] : countAt(all, tick).beat;
  if (bar.fermata) void fermataMenu(piece, fermataBar(piece, tick));
  else setFermatas(piece, "Add a fermata", [{ id: bar.id, bar: bar.at, fermata: { beat, extra: 1 } }]);
  return true;
}
/** Moves the fermata of the bar `from` of the piece to the count at `tick`, in place of one its bar has. */
export function moveFermata(piece: Piece, from: number, tick: number) {
  const all = barsOfPiece(piece.state);
  const [was, to] = [all.bars[from], countAt(all, tick)];
  const bar = all.bars[to.bar];
  if (!was?.fermata || !bar || (to.bar === from && to.beat === was.fermata.beat)) return;
  setFermatas(piece, "Move a fermata", [...(to.bar === from ? [] : [{ id: was.id, bar: was.at, fermata: null }]), { id: bar.id, bar: bar.at, fermata: { ...was.fermata, beat: to.beat } }]);
}
/** How much longer a fermata holds, to pick from. */
const HOLDS = [0.5, 1, 2, 4];
const holdSaid = (beats: number) => (beats === 0.5 ? "Half a beat longer" : beats === 1 ? "1 beat longer" : `${beats} beats longer`);
/** The menu of the fermata of the bar `index` of the piece: how much longer it holds, a length to type, the count before or after, or off. */
export async function fermataMenu(piece: Piece, index: number) {
  const bar = barsOfPiece(piece.state).bars[index];
  const fermata = bar?.fermata;
  if (!bar || !fermata) return;
  const set = (to: typeof fermata | null, label: string) => setFermatas(piece, label, [{ id: bar.id, bar: bar.at, fermata: to }]);
  const hold = (extra: number) => set({ ...fermata, extra }, "Change a fermata");
  const step = (by: number) => set({ ...fermata, beat: fermata.beat + by }, "Move a fermata");
  const [longest, other] = [longestHold(bar), !HOLDS.includes(fermata.extra)];
  const type = async () => {
    const [, number] = /(\d+(?:[.,]\d+)?)/.exec((await piece.openField({ placeholder: `Beats longer, up to ${longest}`, width: 200 })) ?? "") ?? [];
    if (number && Number(number.replace(",", ".")) > 0) hold(Math.min(Math.round(Number(number.replace(",", ".")) * 100) / 100, longest));
  };
  type Item = MenuItem & { run: () => unknown };
  const items: Item[] = [
    // Not a hold the bar's tempo cannot play: the clock goes no slower than 10.
    ...HOLDS.map((beats) => ({ label: holdSaid(beats), checked: fermata.extra === beats, disabled: beats > longest, run: () => hold(beats) })),
    { label: other ? `${holdSaid(fermata.extra)}…` : "Another length…", checked: other, run: type },
    { label: "Move a beat earlier", disabled: fermata.beat <= 1, separator: true, run: () => step(-1) },
    { label: "Move a beat later", disabled: fermata.beat >= partsOf(bar.signature)[0], run: () => step(1) },
    { label: "Remove the fermata", separator: true, run: () => set(null, "Remove a fermata") },
  ];
  const picked = await piece.openMenu(items.map(({ run: _, ...item }) => item));
  if (picked !== null) void items[picked]?.run();
}

/** Asks how many bars to add after the chosen ones, or at the end with none chosen, and adds them, chosen. */
export async function askBars(piece: Piece) {
  const choice = choiceOf(piece);
  const { bars } = barsOfPiece(piece.state);
  const last = choice ? choice.to : bars.length - 1;
  const after = bars[last];
  if (!after) return;
  const [, number] = /(\d+)/.exec((await piece.openField({ placeholder: `Bars to add after bar ${last + 1}`, width: 210 })) ?? "") ?? [];
  const count = clamp(Number(number ?? 0), 0, 200);
  if (count < 1) return;
  addBars(piece, after.id, after.at + 1, count);
  chooseBars(piece, last + 1, last + count);
}

/** The menu of the choice, each item with its key: whole bars also add a bar or start a section, music opens its notes. Opened at the tick `tick`, its fermata goes on the count there. */
export async function choiceMenu(piece: Piece, tick?: number) {
  const choice = choiceOf(piece);
  if (!choice) return;
  const hasFermata = choice.all.bars[fermataBar(piece, tick)]?.fermata !== undefined;
  const whole = isWhole(choice);
  const startsSection = choice.all.bars[choice.from]!.at === 0;
  const [first, last] = [choice.all.bars[choice.from]!, choice.all.bars[choice.to]!];
  const section = whole && startsSection && first.id === last.id && choice.all.bars[choice.to + 1]?.id !== first.id ? first.id : undefined;
  type Item = MenuItem & { run: () => unknown };
  const items: Item[] = [
    { label: "Cut", shortcut: "piece.cut", run: () => cutChosen(piece) },
    { label: "Copy", shortcut: "piece.copy", run: () => copyChosen(piece) },
    { label: whole ? "Paste as new bars" : "Paste", shortcut: "piece.paste", disabled: piece.memory.copied === null, run: () => pasteChosen(piece) },
    { label: whole ? "Repeat as new bars" : "Repeat", shortcut: "piece.repeat_music", run: () => repeatChosen(piece) },
    whole
      ? { label: "Remove", shortcut: "piece.remove_bars", disabled: isAll(choice), run: () => clearChosen(piece, true) }
      : { label: "Clear", shortcut: "piece.clear_music", run: () => clearChosen(piece, false) },
    { label: "Time signature…", shortcut: "piece.type_time_signature", separator: true, run: () => askSignature(piece) },
    { label: "Tempo…", shortcut: "piece.type_tempo", run: () => askTempo(piece) },
    { label: "Rit. or accel.…", shortcut: "piece.type_slide", run: () => askSlide(piece) },
    { label: hasFermata ? "Fermata…" : "Fermata", shortcut: "piece.fermata", run: () => toggleFermata(piece, tick) },
    ...(whole
      ? ([
          // With all the bars of one section chosen, its own menu: its name, a copy, its place.
          ...(section ? ([{ label: "Section…", shortcut: "piece.section_menu", separator: true, run: () => sectionMenu(piece, section) }] satisfies Item[]) : []),
          { label: "Start a section here", shortcut: "piece.add_section", disabled: startsSection, separator: section === undefined, run: () => startSectionAt(piece) },
          // One bar after them is the button under them, and shift-B.
          { label: "Add a bar before", shortcut: "piece.add_bar_before", separator: true, run: () => addBar(piece, "before") },
          { label: "Add bars after…", shortcut: "piece.add_bars", run: () => askBars(piece) },
        ] satisfies Item[])
      : ([{ label: "Edit notes", shortcut: "piece.edit_notes", separator: true, run: () => editChosen(piece) }] satisfies Item[])),
    { label: "Choose to the end of the section", shortcut: "piece.bars_to_end", disabled: choice.to === choice.all.bars.length - 1, separator: true, run: () => chooseToEnd(piece) },
  ];
  // Taller than a menu is by itself: all of it shows.
  const picked = await piece.openMenu(items.map(({ run: _, ...item }) => item), { maxHeight: 560 });
  if (picked !== null) items[picked]?.run();
}
