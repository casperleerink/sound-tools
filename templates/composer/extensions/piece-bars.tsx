import { Button, Circle, For, Fragment, h, Label, Line, memo, now, Rect, Show, Space, Text, type Drag, type MenuItem, type Press } from "./sdk";
import type { Piece } from "./piece";
import { addBars, addPart, duplicateBars, removeBars, splitSection } from "./piece-edits";
import type { Bar } from "./piece-form";
import { AFTER, chooseBars, clamp, colourOf, DIM, LOW_ROW, muted, openAt, openOf, ROW, STRIP, type Page } from "./piece-page";
import { addTrack } from "./piece-tracks";

// The bars of the open section: the strip over the rows, which chooses them, and a row of notes
// per instrument under it. Across, a unit is a point: a bar is as wide as it is long.

/** Where the two lines of the strip start, and its playhead's dot, under them. */
const [NUMBER, MARKS, HEAD] = [5, 21, STRIP - 4];
/** About how many points a label of 12 points takes. */
const widthOf = (text: string) => text.length * 5.6;
const bpmText = (bpm: number) => `${Math.round(bpm * 10) / 10} bpm`;
/** What the strip says under the number of bar `index`: its time signature and its tempo, where they change, and both at the first bar. */
const marksAt = (bars: readonly Bar[], index: number) => {
  const [bar, before] = [bars[index]!, bars[index - 1]];
  return { signature: bar.signature !== before?.signature ? bar.signature : "", tempo: bar.bpm !== before?.bpm ? bpmText(bar.bpm) : "" };
};

/** The chosen bars of the open section, as whole bars: none while the music of some instruments is chosen. */
function chosenBars(piece: Piece) {
  const [open, chosen] = [openOf(piece), piece.memory.chosen];
  if (!open || !chosen || chosen.rows !== null) return undefined;
  const last = open.bars.length - 1;
  return { open, from: Math.min(chosen.from, last), to: Math.min(chosen.to, last) };
}

/** Adds a bar after the chosen ones, or at the end; `before` them, or at the start: it is chosen. */
export function addBar(piece: Piece, before: boolean) {
  const open = openOf(piece);
  if (!open) return;
  const chosen = chosenBars(piece);
  const at = before ? (chosen?.from ?? 0) : chosen ? chosen.to + 1 : open.bars.length;
  addBars(piece, open.id, at);
  chooseBars(piece, at, at);
}
/** Removes the chosen bars with their music, unless they are all the section has: the bar that takes their place is chosen. */
export function removeChosen(piece: Piece): boolean {
  const chosen = chosenBars(piece);
  if (!chosen) return false;
  if (chosen.to - chosen.from + 1 < chosen.open.bars.length) {
    removeBars(piece, chosen.open.id, chosen.from, chosen.to);
    chooseBars(piece, chosen.from, chosen.from);
  }
  return true;
}
/** The chosen bars again right after them, with their music: the copy is chosen. */
export function duplicateChosen(piece: Piece): boolean {
  const chosen = chosenBars(piece);
  if (!chosen) return false;
  duplicateBars(piece, chosen.open.id, chosen.from, chosen.to);
  chooseBars(piece, chosen.to + 1, 2 * chosen.to - chosen.from + 1);
  return true;
}
/** A new section from the first chosen bar on, opened with its name to type; none at the first bar. */
export function startSectionAt(piece: Piece): boolean {
  const chosen = chosenBars(piece);
  const id = chosen && chosen.from > 0 ? splitSection(piece, chosen.open.id, chosen.from) : undefined;
  if (!id) return false;
  openAt(piece, id);
  piece.memory.naming = { id, in: "head" };
  return true;
}
/** Chooses the bar before or after the chosen ones, or with `extend` takes it in too. */
export function stepBars(piece: Piece, by: -1 | 1, extend: boolean): boolean {
  const chosen = chosenBars(piece);
  if (!chosen) return false;
  const next = by < 0 ? chosen.from - 1 : chosen.to + 1;
  if (extend) chooseBars(piece, Math.min(chosen.from, next), Math.max(chosen.to, next));
  else chooseBars(piece, next, next);
  return true;
}

/** The menu of the chosen bars, each item with its key. */
export async function barMenu(piece: Piece) {
  const chosen = chosenBars(piece);
  if (!chosen) return;
  const items: MenuItem[] = [
    { label: "Add a bar before", shortcut: "piece.add_bar_before" },
    { label: "Add a bar after", shortcut: "piece.add_bar" },
    { label: "Duplicate", shortcut: "piece.duplicate_bars" },
    { label: "Remove", shortcut: "piece.delete_bars", disabled: chosen.to - chosen.from + 1 >= chosen.open.bars.length },
    { label: "Start a section here", shortcut: "piece.add_section", disabled: chosen.from === 0, separator: true },
  ];
  const picked = await piece.openMenu(items);
  if (picked === 0 || picked === 1) addBar(piece, picked === 0);
  else if (picked === 2) duplicateChosen(piece);
  else if (picked === 3) removeChosen(piece);
  else if (picked === 4) startSectionAt(piece);
}

/** The x of the tick `tick` of the open section, which follows the room and the bars. */
const xOf = ({ scale }: Page, tick: number) => () => tick * scale();
/** The playhead, as far into the open section as it plays: 1 while it is inside. */
const playheadOf = ({ place, scale }: Page) => ({
  x: () => now.tick.minus(place()?.start ?? 0).times(scale()).plus(0.5),
  inside: () => {
    const at = place();
    return at ? now.tick.ge(at.start).times(now.tick.lt(at.start + at.length)) : 0;
  },
});
/** The chosen bars, from where the first starts to where the last ends, in points: none while no whole bars are chosen. */
const chosenSpan = ({ memory, starts, scale }: Page) => {
  const chosen = memory.chosen;
  const last = starts().length - 2;
  if (!chosen || chosen.rows !== null || last < 0) return { x: 0, width: 0, shown: 0 };
  const [from, to] = [starts()[Math.min(chosen.from, last)]!, starts()[Math.min(chosen.to, last) + 1]!];
  return { x: from * scale(), width: (to - from) * scale(), shown: 1 };
};
const extentOf = ({ starts, scale }: Page) => (starts().at(-1) ?? 0) * scale() + AFTER;

/**
 * The strip over the rows: a bar's number, and under it its time signature and tempo where they
 * change. A press chooses a bar, shift-press or a drag a range, and puts the playhead there; a
 * right press opens the menu of the chosen bars. The plus after the last adds one.
 */
export const BarStrip = ({ page }: { page: Page }) => {
  const { memory, bars, starts, place, scale, barAt } = page;
  const cells = memo(() => bars().map((_, index) => ({ index, from: starts()[index]!, to: starts()[index + 1]!, number: (place()?.bar ?? 1) + index, ...marksAt(bars(), index) })));
  const chosen = () => chosenSpan(page);
  const inChosen = (index: number) => {
    const at = memory.chosen;
    return at !== null && at.rows === null && index >= at.from && index <= at.to;
  };
  const playhead = playheadOf(page);
  const sweep = now.drag("bars");
  const sweepDrag: Drag = {
    id: "bars",
    axis: "x",
    onEnd: ({ startX, x }) => {
      const [from, to] = [barAt(startX), barAt(clamp(x, 0, (starts().at(-1) ?? 0) * scale() - 1))];
      if (from !== null && to !== null) chooseBars(page, from, to);
    },
  };
  const press = ({ x, shift, button }: Press) => {
    const bar = barAt(x);
    if (bar === null) return;
    const at = memory.chosen;
    const among = at !== null && at.rows === null && bar >= at.from && bar <= at.to;
    if (button === "right") {
      if (!among) chooseBars(page, bar, bar);
      void barMenu(page);
    } else if (shift && at !== null && at.rows === null) chooseBars(page, Math.min(at.from, bar), Math.max(at.to, bar));
    else chooseBars(page, bar, bar);
  };
  return (
    <Space key="strip" height={STRIP} extent={() => ({ x: [0, extentOf(page)] })} follow={{ x: "rows" }} onPress={press} drag={sweepDrag} cursor="pointer">
      <Rect x={() => chosen().x + 0.5} y={1.5} width={() => Math.max(chosen().width - 1, 0)} height={STRIP - 3} radius={4} color="selected" border="text" borderWidth={1} opacity={() => chosen().shown} />
      <Rect x={sweep.startX.min(sweep.x)} y={1.5} width={sweep.dx.abs()} height={STRIP - 3} radius={4} color="fill" border="text-muted" borderWidth={1} opacity={sweep.active} />
      <For each={cells}>
        {(cell) => {
          // Its marks show only while the bar leaves them room: the time signature before the tempo.
          const room = () => (cell.to - cell.from) * scale() - 12;
          const both = [cell.signature, cell.tempo].filter(Boolean).join("  ");
          const marks = () => (widthOf(both) <= room() ? both : widthOf(cell.signature) <= room() ? cell.signature : "");
          return [
            <Rect x={xOf(page, cell.from)} y={0} width={1} height={STRIP} color="border" />,
            <Label x={() => cell.from * scale() + 6} y={NUMBER} text={String(cell.number)} color={() => (inChosen(cell.index) ? "text" : "text-secondary")} />,
            <Label x={() => cell.from * scale() + 6} y={MARKS} text={marks} color="text-muted" />,
          ];
        }}
      </For>
      <Rect x={() => (starts().at(-1) ?? 0) * scale()} y={0} width={1} height={STRIP} color="border" />
      <Rect key="add-bar" x={() => (starts().at(-1) ?? 0) * scale() + 10} y={(STRIP - 24) / 2} width={24} height={24} radius={6} color="none" hover="fill" cursor="pointer"
        onClick={() => addBar(page, false)} />
      <Rect x={() => (starts().at(-1) ?? 0) * scale() + 17} y={STRIP / 2 - 0.75} width={10} height={1.5} color="text-secondary" />
      <Rect x={() => (starts().at(-1) ?? 0) * scale() + 21.25} y={STRIP / 2 - 5} width={1.5} height={10} color="text-secondary" />
      <Circle x={playhead.x} y={HEAD} radius={3.5} color="text" opacity={playhead.inside} />
    </Space>
  );
};

/** Whether no instrument plays a note in any section: the first row then says how to write some. */
const silentOf = ({ rows }: Page) => memo(() => rows().every((each) => each.children.every((child) => child.tool !== "timeline" || child.state.notes.length === 0)));

/**
 * A row per instrument with the notes of its part in the open section, small, in its colour, the
 * bar lines through them, which the heads scroll with; under them the room beside the returns
 * and the master. A press chooses an instrument as its head does; a double click opens the notes
 * of its part, which it makes when there is none.
 */
export const Rows = ({ page }: { page: Page }) => {
  const { rows, returnsHeight, starts, scale, openId, memory, partAt, openPanel, openEditor } = page;
  const space = now.space("rows");
  const silent = silentOf(page);
  const chosen = () => chosenSpan(page);
  const playhead = playheadOf(page);
  const press = ({ y, clicks, button }: Press) => {
    const each = rows()[Math.floor(y)];
    const id = openId();
    if (!each || id === null || button === "right") return;
    memory.chosen = null;
    const hasPart = partAt(each, id) !== undefined;
    if (clicks === 2) {
      if (!hasPart) addPart(page, each.name, id);
      openEditor(each.name);
    } else if (memory.below?.kind === "editor" && hasPart) openEditor(each.name);
    else openPanel(each.name);
  };
  return (
    <>
      <div style={{ grow: true }}>
        <Space key="rows" grow unit={{ y: ROW }} extent={() => ({ x: [0, extentOf(page)], y: [0, rows().length + 1] })} onPress={press}>
          <Rect x={() => chosen().x} y={0} width={() => chosen().width} height={() => rows().length} color="selected" opacity={() => chosen().shown} />
          <For each={starts}>{(tick) => <Rect x={xOf(page, tick)} y={0} width={1} height={() => rows().length} color="border" />}</For>
          <For each={rows}>
            {(each, index) => {
              const notes = () => {
                const id = openId();
                return (id === null ? undefined : partAt(each, id))?.state.notes ?? [];
              };
              // Between its lowest and highest pitch, a line each, an octave tall at least, in the middle of the row.
              const miniature = memo(() => {
                const pitches = notes().map((note) => note.pitch);
                const [lowest, highest] = [Math.min(...pitches), Math.max(...pitches)];
                const range = highest - lowest + 1;
                const shown = Math.max(range, 12);
                const step = (ROW - 24) / shown;
                const height = clamp(step, 2, 4);
                const bottom = ROW - 12 - ((shown - range) / 2) * step;
                return { lowest, step, height, bottom };
              });
              return (
                <>
                  <Rect x={-1e6} y={index} width={2e6} height={space.pointY} color="border" />
                  <For each={notes}>
                    {(note) => (
                      <Rect x={() => note.start * scale()} width={() => Math.max(note.length * scale() - 1, 1)} color={() => colourOf(each)} opacity={() => (muted(each) ? DIM : 1)}
                        y={() => {
                          const { bottom, lowest, step, height } = miniature();
                          return index() + (bottom - (note.pitch - lowest + 0.5) * step - height / 2) / ROW;
                        }}
                        height={() => miniature().height / ROW} />
                    )}
                  </For>
                </>
              );
            }}
          </For>
          <Rect x={-1e6} y={() => rows().length} width={2e6} height={space.pointY} color="border" />
          <Label x={12} y={() => 0.5 - 8 / ROW} text={() => (silent() && rows().length > 0 ? "Double-click a bar to write notes" : "")} color="text-muted" />
          <Line from={() => [playhead.x(), 0] as const} to={() => [playhead.x(), rows().length] as const} color="text" width={1} opacity={playhead.inside} />
        </Space>
        <Show when={() => rows().length === 0}>{() => <EmptyPiece page={page} />}</Show>
      </div>
      <div style={() => ({ height: returnsHeight() + LOW_ROW, keep: true })} />
    </>
  );
};

/** Over the rows while the piece has no instrument: what to start with, as the window's empty project says it. */
const EmptyPiece = ({ page }: { page: Page }) => (
  <div style={{ absolute: { top: 0, left: 0, right: 0, bottom: 0 }, align: "center", justify: "center", gap: "xs" }}>
    <Text variant="title">An empty piece</Text>
    <Text tone="muted">Add an instrument to write for, or ask the agent for one.</Text>
    <div style={{ paddingTop: "md" }}>
      <Button key="empty-instrument" label="Instrument" icon="plus" variant="subtle" size="md" onClick={() => addTrack(page, 0)} />
    </div>
  </div>
);
