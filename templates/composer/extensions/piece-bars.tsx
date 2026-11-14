import { Button, Circle, For, Fragment, h, Label, Line, Live, memo, now, Rect, Show, Space, Text, type Drag, type Press } from "./sdk";
import { addBar, choiceMenu, choiceOf, chooseRows } from "./piece-choice";
import { beatOf, tempoOf, type Bar } from "./piece-form";
import { AFTER, chooseBars, clamp, colourOf, DIM, LOW_ROW, muted, ROW, STRIP, type Page } from "./piece-page";
import { addTrack } from "./piece-tracks";

// The bars of the open section: the strip over the rows, which chooses them, and a row of notes
// per instrument under it. Across, a unit is a point: a bar is as wide as it is long.

/** Where the two lines of the strip start, the foot of the outline of the chosen bars, and under it the playhead's dot, on the line where the piece repeats. */
const [NUMBER, MARKS, HEAD, FOOT] = [5, 21, STRIP - 3, STRIP - 6];
/** The points a bar's number needs before the next one. */
const NUMBER_ROOM = 26;
/** About how many points a label of 12 points takes. */
const widthOf = (text: string) => text.length * 5.6;
const HINT = "Double-click a bar to write notes";
/**
 * What the strip says under the number of bar `index`, from the longest to the shortest it may
 * show: its time signature and its tempo, where they change, and both at the first bar. A change
 * is never hidden: the narrowest bar still shows a dot.
 */
const marksAt = (bars: readonly Bar[], index: number): string[] => {
  const [bar, before] = [bars[index]!, bars[index - 1]];
  const signature = bar.signature !== before?.signature ? bar.signature : "";
  const said = (each: Bar) => `${beatOf(each.signature).sign} = ${tempoOf(each)}`;
  const tempo = !before || said(bar) !== said(before) ? said(bar) : "";
  if (!signature && !tempo) return [];
  if (!tempo) return [signature, "•"];
  return signature ? [`${signature}  ${tempo}`, `${signature} •`, "•"] : [tempo, "•"];
};

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
/** The chosen bars, from where the first starts to where the last ends, in points, and the rows of the chosen instruments: none while nothing is chosen. */
const chosenSpan = (page: Page) => {
  const { starts, scale, rows } = page;
  const choice = choiceOf(page);
  if (!choice) return { x: 0, width: 0, whole: 0, music: 0, top: 0, height: 0 };
  const [from, to] = [starts()[choice.from]!, starts()[choice.to + 1]!];
  const span = { x: from * scale(), width: (to - from) * scale() };
  if (choice.rows === null) return { ...span, whole: 1, music: 0, top: 0, height: 0 };
  const indexes = choice.rows.map((name) => rows().findIndex((each) => each.name === name));
  const top = Math.min(...indexes);
  return { ...span, whole: 0, music: 1, top, height: Math.max(...indexes) - top + 1 };
};
const extentOf = ({ starts, scale }: Page) => (starts().at(-1) ?? 0) * scale() + AFTER;
/** The bar of the open section at `x` points, the last past its end. */
const barNear = ({ starts, scale, barAt }: Page, x: number) => barAt(clamp(x, 0, (starts().at(-1) ?? 0) * scale() - 1))!;

/** Where the clock repeats inside the open section, in points: none while it does not. */
const repeatSpan = ({ clock, place, scale }: Page) => {
  const [repeat, at] = [clock.repeat, place()];
  if (!repeat || !at) return { x: 0, width: 0, shown: 0 };
  const [from, to] = [Math.max(repeat.from, at.start) - at.start, Math.min(repeat.to, at.start + at.length) - at.start];
  return to > from ? { x: from * scale(), width: (to - from) * scale(), shown: 1 } : { x: 0, width: 0, shown: 0 };
};

/**
 * The strip over the rows: a bar's number, and under it its time signature and tempo where they
 * change. A press chooses a bar, shift-press or a drag a range, and puts the playhead there; a
 * right press opens the menu of the chosen bars. The plus after the last adds one. At its foot,
 * a line where the piece repeats.
 */
export const BarStrip = ({ page }: { page: Page }) => {
  const { memory, bars, starts, place, scale, barAt } = page;
  const cells = memo(() => bars().map((_, index) => ({ index, from: starts()[index]!, to: starts()[index + 1]!, number: (place()?.bar ?? 1) + index, marks: marksAt(bars(), index) })));
  const chosen = () => chosenSpan(page);
  // In a long section every second, fourth, eighth … bar says its number, so the numbers keep their room.
  const numbered = memo(() => {
    const narrowest = Math.min(...cells().map((cell) => (cell.to - cell.from) * scale()));
    return 2 ** Math.max(Math.ceil(Math.log2(NUMBER_ROOM / Math.max(narrowest, 1))), 0);
  });
  // The first chosen bar always says its number, and the bar before it gives it the room.
  const numberShows = (index: number) => {
    const first = memory.chosen?.from;
    return index === first || (index % numbered() === 0 && (numbered() === 1 || index + 1 !== first));
  };
  const inChosen = (index: number) => {
    const at = memory.chosen;
    return at !== null && index >= at.from && index <= at.to;
  };
  const repeat = () => repeatSpan(page);
  const playhead = playheadOf(page);
  const sweep = now.drag("bars");
  const sweepDrag: Drag = {
    id: "bars",
    axis: "x",
    onEnd: ({ startX, x }) => {
      const from = barAt(startX);
      if (from !== null) chooseBars(page, from, barNear(page, x));
    },
  };
  const press = ({ x, shift, button }: Press) => {
    const bar = barAt(x);
    if (bar === null) return;
    const at = memory.chosen;
    const among = at !== null && at.rows === null && bar >= at.from && bar <= at.to;
    if (button === "right") {
      if (!among) chooseBars(page, bar, bar);
      void choiceMenu(page);
    } else if (shift && at !== null) chooseBars(page, Math.min(at.from, bar), Math.max(at.to, bar));
    else chooseBars(page, bar, bar);
  };
  return (
    <Space key="strip" height={STRIP} extent={() => ({ x: [0, extentOf(page)] })} follow={{ x: "rows" }} onPress={press} drag={sweepDrag} cursor="pointer">
      <Rect x={() => chosen().x + 0.5} y={1.5} width={() => Math.max(chosen().width - 1, 0)} height={FOOT - 1.5} radius={4} color="selected" border="text" borderWidth={1} opacity={() => chosen().whole} />
      <Rect x={sweep.startX.min(sweep.x)} y={1.5} width={sweep.dx.abs()} height={FOOT - 1.5} radius={4} color="fill" border="text-muted" borderWidth={1} opacity={sweep.active} />
      <For each={cells}>
        {(cell) => {
          // The longest of its marks that fits before the next bar with marks.
          const room = () => ((cells().find((each) => each.index > cell.index && each.marks.length > 0)?.from ?? starts().at(-1) ?? cell.to) - cell.from) * scale() - 12;
          const marks = () => cell.marks.find((text) => widthOf(text) <= room()) ?? cell.marks.at(-1) ?? "";
          return [
            <Rect x={xOf(page, cell.from)} y={0} width={1} height={STRIP} color="border" />,
            <Label x={() => cell.from * scale() + 6} y={NUMBER} text={String(cell.number)} color={() => (inChosen(cell.index) ? "text" : "text-secondary")}
              opacity={() => (numberShows(cell.index) ? 1 : 0)} />,
            <Label x={() => cell.from * scale() + 6} y={MARKS} text={marks} color="text-muted" />,
          ];
        }}
      </For>
      <Rect x={() => (starts().at(-1) ?? 0) * scale()} y={0} width={1} height={STRIP} color="border" />
      <Rect key="add-bar" x={() => (starts().at(-1) ?? 0) * scale() + 10} y={(STRIP - 24) / 2} width={24} height={24} radius={6} color="none" hover="fill" cursor="pointer"
        onClick={() => addBar(page, "end")} />
      <Rect x={() => (starts().at(-1) ?? 0) * scale() + 17} y={STRIP / 2 - 0.75} width={10} height={1.5} color="text-secondary" />
      <Rect x={() => (starts().at(-1) ?? 0) * scale() + 21.25} y={STRIP / 2 - 5} width={1.5} height={10} color="text-secondary" />
      <Rect x={() => repeat().x + 1} y={STRIP - 2.5} width={() => Math.max(repeat().width - 2, 0)} height={2} radius={1} color="text" opacity={() => repeat().shown} />
      <Circle x={playhead.x} y={HEAD} radius={3} color="text" opacity={playhead.inside} />
    </Space>
  );
};

/** Whether no instrument plays a note in any section: the first row then says how to write some. */
const silentOf = ({ rows }: Page) => memo(() => rows().every((each) => each.children.every((child) => child.tool !== "timeline" || child.state.notes.length === 0)));

/**
 * A row per instrument with the notes of its part in the open section, small, in its colour, the
 * bar lines through them, which the heads scroll with; under them the room beside the returns
 * and the master. A press chooses that bar of that instrument and the instrument, shift-press
 * takes in the bars and instruments up to it, a drag those it goes over; a right press opens the
 * menu of what is chosen; a double click opens the notes of its part. A press under the rows lets go.
 */
export const Rows = ({ page }: { page: Page }) => {
  const { rows, returnsHeight, starts, scale, openId, memory, partAt, openPanel, openEditor, barAt } = page;
  const space = now.space("rows");
  const silent = silentOf(page);
  const chosen = () => chosenSpan(page);
  const playhead = playheadOf(page);
  const names = () => rows().map((each) => each.name);
  const press = ({ x, y, clicks, shift, button }: Press) => {
    const each = rows()[Math.floor(y)];
    const bar = barAt(x);
    if (!each) {
      if (button === "left") memory.chosen = null;
      return;
    }
    if (bar === null) {
      memory.chosen = null;
      return openPanel(each.name);
    }
    const at = choiceOf(page);
    if (button === "right") {
      const among = at?.rows?.includes(each.name) && bar >= at.from && bar <= at.to;
      if (!among) chooseRows(page, bar, bar, [each.name]);
      void choiceMenu(page);
    } else if (clicks === 2) openEditor(each.name);
    else if (shift && at?.rows) {
      const indexes = [names().indexOf(each.name), names().indexOf(at.rows[0]!), names().indexOf(at.rows.at(-1)!)];
      chooseRows(page, Math.min(at.from, bar), Math.max(at.to, bar), names().slice(Math.min(...indexes), Math.max(...indexes) + 1));
    } else if (shift && at) chooseBars(page, Math.min(at.from, bar), Math.max(at.to, bar));
    else chooseRows(page, bar, bar, [each.name]);
  };
  // A drag chooses whole bars of whole rows: what it will choose shows as it goes, from the bar lines under its ends.
  const sweep = now.drag("music");
  const sweepDrag: Drag = {
    id: "music",
    onEnd: ({ startX, startY, x, y }) => {
      const last = rows().length - 1;
      const [from, to] = [barAt(startX), barNear(page, x)];
      const [top, bottom] = [clamp(Math.floor(Math.min(startY, y)), 0, last), clamp(Math.floor(Math.max(startY, y)), 0, last)];
      if (from !== null && last >= 0) chooseRows(page, from, to, names().slice(top, bottom + 1));
    },
  };
  const [left, right] = [sweep.startX.min(sweep.x), sweep.startX.max(sweep.x).max(0.5)];
  const swept = () => {
    const [ticks, points] = [starts(), scale()];
    const sum = (each: (start: number, index: number) => Live) => ticks.slice(0, -1).reduce((total, start, index) => total.plus(each(start, index)), Live.of(0));
    const from = sum((start, index) => (index === 0 ? Live.of(0) : left.ge(start * points).times((start - ticks[index - 1]!) * points)));
    const to = sum((start, index) => right.gt(start * points).times((ticks[index + 1]! - start) * points));
    const last = Math.max(rows().length - 1, 0);
    const top = sweep.startY.min(sweep.y).floor().clamp(0, last);
    return { from, width: to.minus(from), top, height: sweep.startY.max(sweep.y).floor().clamp(0, last).plus(1).minus(top) };
  };
  return (
    <>
      <div style={{ grow: true }}>
        <Space key="rows" grow unit={{ y: ROW }} extent={() => ({ x: [0, extentOf(page)], y: [0, rows().length + 1] })} onPress={press} drag={sweepDrag}>
          <Rect x={() => chosen().x} y={0} width={() => chosen().width} height={() => rows().length} color="selected" opacity={() => chosen().whole} />
          <Rect x={() => chosen().x + 0.5} y={() => chosen().top + 2 / ROW} width={() => Math.max(chosen().width - 1, 0)} height={() => chosen().height - 4 / ROW} radius={4}
            color="selected" border="text" borderWidth={1} opacity={() => chosen().music} />
          <Rect x={() => swept().from} y={() => swept().top} width={() => swept().width} height={() => swept().height} radius={4}
            color="fill" border="text-muted" borderWidth={1} opacity={sweep.active} />
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
          {/* On the background, so a bar line under it does not cross it. */}
          <Show when={() => silent() && rows().length > 0}>
            {() => [
              <Rect x={6} y={0.5 - 12 / ROW} width={widthOf(HINT) + 12} height={24 / ROW} radius={4} color="background" />,
              <Label x={12} y={0.5 - 8 / ROW} text={HINT} color="text-muted" />,
            ]}
          </Show>
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
