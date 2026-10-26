import { Circle, For, Fragment, Group, h, Input, Label, Line, memo, now, Rect, Space, type Clock, type Drag, type Press, type TextPart } from "./sdk";
import { ACROSS, barStarts, BEAT, COLOR, LEAD, RULER, type Page, type Runs } from "./song-page";

// The ruler: the sections on its top line; bars, time signatures and tempo changes on the line
// under it; at its foot the repeat range.

/** What starts at a bar of the ruler: a section, a time signature change or a tempo change, at `tick`. */
export type Mark = { kind: "section" | "signature" | "tempo"; bar: number; tick: number };

/** The bar each run starts at, from 1: where a time signature change is, even to the same one. */
function changeBars(runs: Runs): number[] {
  const bars = [1];
  for (const run of runs.slice(0, -1)) bars.push(bars.at(-1)! + run.bars);
  return bars;
}
/** Where the line under the sections starts, and the points between two labels on it. */
const LINE = 18;
const GAP = 6;
/** About how many points a label of 12 points takes. */
const widthOf = (parts: readonly TextPart[]) => parts.reduce((width, part) => width + part.text.length * 5.6 + (part.border ? 2.5 : 0), 0);
/** A tempo as the ruler and the tempo lane show it, to a tenth. */
export const bpmText = (bpm: number) => String(Math.round(bpm * 10) / 10);

/** The tick nearest `tick` on the grid of the snap in the corner. */
const snapped = ({ memory }: Page, tick: number) => Math.max(memory.snap > 0 ? Math.round(tick / memory.snap) * memory.snap : Math.round(tick), 0);
/** The bar whose line is nearest `tick`, from 1. */
const nearestBar = ({ starts }: Page, tick: number) => {
  const lines = starts();
  return lines.reduce((best, start, index) => (Math.abs(start - tick) < Math.abs((lines[best] ?? 0) - tick) ? index : best), 0) + 1;
};
/** A section at the bar line nearest `tick`, named by how many there are, its name open to change. */
function addSection(page: Page, tick: number) {
  const { clock, memory, starts } = page;
  const bar = nearestBar(page, tick);
  if (!clock.sections.some((each) => each.bar === bar)) {
    clock.update("Add a section", (song) => {
      song.sections = [...song.sections, { bar, name: `Section ${song.sections.length + 1}` }].sort((a, b) => a.bar - b.bar);
    });
  }
  memory.editing = { bar, tick: starts()[bar - 1] ?? 0 };
}

/**
 * A press on the ruler or the tempo lane, as the arrangement's: a click plays from the grid, a
 * double click on the ruler adds a section. The menu adds a tempo change on the grid, and a time
 * signature or a section on the nearest bar line; it removes the mark there.
 */
export const pressTime = (page: Page, sections: boolean) => async ({ button, x, clicks }: Press) => {
  const { clock, memory, runs, starts, openMenu } = page;
  if (button !== "right") {
    memory.mark = null;
    if (clicks === 2 && sections) addSection(page, x);
    else clock.jump(snapped(page, x));
    return;
  }
  const [tick, bar] = [snapped(page, x), nearestBar(page, x)];
  const line = starts()[bar - 1] ?? 0;
  const section = clock.sections.find((each) => each.bar === bar);
  const signature = bar > 1 && changeBars(runs()).includes(bar);
  const tempo = clock.tempo_map.tempo_changes.find((each) => each.tick > 0 && (each.tick === tick || each.tick === line));
  const marked = section !== undefined || signature || tempo !== undefined;
  const picked = await openMenu([
    { label: "Tempo", icon: "tempo-change", disabled: clock.tempo_map.tempo_changes.some((each) => each.tick === tick) },
    { label: "Time signature", icon: "time-signature", disabled: bar === 1 || signature },
    { label: "Section", icon: "section", disabled: section !== undefined },
    ...(marked ? [{ label: "Remove", icon: "trash-2" as const, shortcut: "backspace" as const, separator: true }] : []),
  ]);
  // A tempo or a time signature added is chosen with the playhead on it, for the transport to change.
  if (picked === 0) {
    addTempo(clock, memory, tick);
    clock.jump(tick);
  } else if (picked === 1) {
    addSignature(clock, bar);
    memory.mark = { kind: "signature", bar, tick: line };
    clock.jump(line);
  } else if (picked === 2) addSection(page, line);
  else if (picked === 3) {
    const mark = section ? { kind: "section" as const, bar, tick: line } : signature ? { kind: "signature" as const, bar, tick: line } : tempo ? { kind: "tempo" as const, bar, tick: tempo.tick } : null;
    if (mark) removeMark(clock, mark);
  }
};
/** The tempo changes after the first, each with its mark. */
export const tempoMarks = ({ clock, barOf }: Page) =>
  clock.tempo_map.tempo_changes.filter((change) => change.tick > 0).map((change) => ({ ...change, mark: { kind: "tempo" as const, bar: barOf(change.tick), tick: change.tick } }));
/** A press on a mark chooses it and plays from it; a double click on a section names it. A right-click opens the menu there. */
export const pressMark = (page: Page, mark: Mark) => (press: Press) => {
  if (press.button === "right") return pressTime(page, false)(press);
  page.choose(new Set());
  page.memory.mark = mark;
  page.clock.jump(mark.tick);
  if (press.clicks === 2 && mark.kind === "section") page.memory.editing = mark;
};

export const Ruler = ({ page }: { page: Page }) => {
  const { memory, clock, runs, starts, end, timeline, playhead, handle } = page;
  const loop = now.drag("loop");
  const chosen = (kind: Mark["kind"], bar: number) => (memory.mark?.kind === kind && memory.mark.bar === bar ? ("gray-800" as const) : undefined);
  // Each section from its bar line to the next one's.
  const sections = memo(() =>
    clock.sections.map((section, index) => ({ ...section, from: starts()[section.bar - 1] ?? end(), to: starts()[(clock.sections[index + 1]?.bar ?? Infinity) - 1] ?? end() })),
  );
  // Where each time signature starts, and the points its label takes: its bar and the signature.
  const changes = memo(() =>
    changeBars(runs()).flatMap((bar, index) => {
      const tick = starts()[bar - 1];
      const signature = runs()[index]?.signature ?? "4/4";
      return tick === undefined ? [] : [{ bar, tick, signature, width: widthOf([{ text: `${bar}  ` }, { text: signature, border: "gray-800" }]) }];
    }),
  );
  /** 1 while the label of the time signature change before leaves room for that at `index`. */
  const changeShows = (index: number) => {
    const [before, change] = [changes()[index - 1], changes()[index]];
    return before && change ? timeline.pointX.le((change.tick - before.tick) / (before.width + GAP)) : 1;
  };
  /**
   * 1 while the number of bar `index` shows: zoomed out, every second, fourth, eighth … bar keeps
   * 40 points between numbers, and a time signature's label takes the room it needs first.
   */
  const numberShows = (index: number) => {
    const tick = starts()[index] ?? 0;
    const before = changes().findLast((change) => change.tick <= tick);
    const after = changes().find((change) => change.tick > tick);
    // The most bars between this number and the next that shows as far out as it does.
    const apart = index & -index;
    let shows = timeline.pointX.le((apart * 4 * BEAT) / 40);
    if (before) shows = shows.times(timeline.pointX.le((tick - before.tick) / (before.width + GAP)));
    if (after) shows = shows.times(timeline.pointX.le((after.tick - tick) / (String(index + 1).length * 5.6 + GAP)));
    return shows;
  };
  // The tempo changes, a tick each at the foot while the tempo lane is closed: the pointer over
  // one, or the chosen one, reads its tempo.
  const tempos = memo(() => (memory.tempoLane ? [] : tempoMarks(page)));
  const tempoChosen = (tick: number) => memory.mark?.kind === "tempo" && memory.mark.tick === tick;
  const loopDrag: Drag = {
    id: "loop",
    snap: { x: BEAT, grid: true },
    onEnd: ({ startX, x }) => {
      const [from, to] = [Math.max(Math.round(Math.min(startX, x) / BEAT) * BEAT, 0), Math.round(Math.max(startX, x) / BEAT) * BEAT];
      if (to > from) clock.setRepeat({ from, to });
    },
  };
  // The repeat's bar under the ruler, as the arrangement had it: a click turns it on or off,
  // its body moves it, an edge moves that end, on the beats. Off, it keeps its range.
  const repeat = () => clock.repeat ?? memory.repeatRange;
  const setRepeatRange = (from: number, to: number) => {
    if (to <= from) return;
    if (clock.repeat) clock.setRepeat({ from, to });
    else memory.repeatRange = { from, to };
  };
  const toggleRepeat = () => {
    if (clock.repeat) memory.repeatRange = { ...clock.repeat };
    clock.setRepeat(clock.repeat ? null : { ...memory.repeatRange });
  };
  const repeatDrag = (part: "move" | "start" | "end"): Drag => ({
    id: `repeat-${part}`,
    axis: "x",
    snap: { x: BEAT, grid: true },
    onEnd: ({ dx }) => {
      const { from, to } = repeat();
      if (part === "move") {
        const moved = Math.max(from + dx, 0) - from;
        setRepeatRange(from + moved, to + moved);
      } else if (part === "start") setRepeatRange(Math.min(Math.max(from + dx, 0), to - BEAT), to);
      else setRepeatRange(from, Math.max(to + dx, from + BEAT));
    },
  });
  const [repeatMove, repeatStart, repeatEnd] = (["move", "start", "end"] as const).map((part) => repeatDrag(part));
  const [moving, starting, ending] = (["move", "start", "end"] as const).map((part) => now.drag(`repeat-${part}`));

  return (
    <Space key="ruler" height={RULER - 1} unit={{ x: ACROSS }} extent={() => ({ x: [-LEAD, end()] })} scroll={{ x: -LEAD }} zoom={{ x: [0.1, 8] }} follow={{ x: "timeline" }}
      onPress={pressTime(page, true)} drag={loopDrag}>
      <Shades page={page} height={RULER - 1} />
      <For each={sections}>
        {(section) => {
          // Its name stays at the left edge while the section goes on past it, and is cut at its end.
          const left = timeline.left.max(section.from);
          const room = left.negate().plus(section.to).minus(timeline.pointX.times(4)).max(0);
          return [
            <Rect x={section.from} y={0} width={timeline.pointX} height={LINE - 2} color="alpha/20" />,
            <Group x={left} clip={{ width: room, height: LINE }}>
              <Rect x={0} y={0} width={room.min(timeline.pointX.times(widthOf([{ text: section.name }]) + 12))} height={LINE} color={COLOR.clear} cursor="pointer"
                onPress={pressMark(page, { kind: "section", bar: section.bar, tick: section.from })} />
              <Label x={timeline.pointX.times(6)} y={2} text={() => [{ text: section.name, color: "gray-950", weight: "medium", border: chosen("section", section.bar) }]} size={12} />
            </Group>,
          ];
        }}
      </For>
      <For each={starts}>
        {(tick, bar) => [
          <Rect x={tick} y={RULER - 8} width={timeline.pointX} height={8} color="alpha/10" />,
          () => (changes().some((change) => change.tick === tick) ? null : (
            <Label x={timeline.pointX.times(6).plus(tick)} y={LINE + 3} text={String(bar() + 1)} size={12} color="gray-700" opacity={() => numberShows(bar())} />
          )),
        ]}
      </For>
      <For each={changes}>
        {(change, index) => [
          <Label x={timeline.pointX.times(6).plus(change.tick)} y={LINE + 3} size={12} color="gray-700" opacity={() => changeShows(index())}
            text={() => [{ text: `${change.bar}  ` }, { text: change.signature, border: chosen("signature", change.bar) }]} />,
          change.bar > 1 ? (
            <Rect x={timeline.pointX.times(4).plus(change.tick)} y={LINE} width={timeline.pointX.times(change.width + 4)} height={16} color={COLOR.clear} cursor="pointer"
              onPress={pressMark(page, { kind: "signature", bar: change.bar, tick: change.tick })} />
          ) : null,
        ]}
      </For>
      <For each={tempos}>
        {(change) => [
          <Rect x={change.tick} y={RULER - 8} width={timeline.pointX} height={8} color={() => (tempoChosen(change.tick) ? "gray-950" : "alpha/40")} />,
          <Rect key={`ruler-tempo-${change.tick}`} x={timeline.pointX.times(-3).plus(change.tick)} y={LINE} width={timeline.pointX.times(7)} height={RULER - LINE - 10}
            color={COLOR.clear} cursor="pointer" onPress={pressMark(page, change.mark)} />,
        ]}
      </For>
      <Rect x={() => moving!.dx.plus(starting!.dx).plus(repeat().from)} y={RULER - 10}
        width={() => ending!.dx.minus(starting!.dx).plus(repeat().to - repeat().from)} height={8} radius={2} color={() => (clock.repeat ? "gray-900" : "alpha/15")} />
      <Rect x={() => moving!.dx.plus(repeat().from)} y={RULER - 14} width={() => repeat().to - repeat().from} height={14}
        color={COLOR.clear} cursor="pointer" drag={repeatMove!} onClick={toggleRepeat} />
      <Rect x={() => starting!.dx.plus(repeat().from)} y={RULER - 14} width={handle} height={14}
        color={COLOR.clear} cursor="ew-resize" drag={repeatStart!} onClick={toggleRepeat} />
      <Rect x={() => ending!.dx.plus(repeat().to).minus(handle)} y={RULER - 14} width={handle} height={14}
        anchor={() => ({ x: repeat().to })} color={COLOR.clear} cursor="ew-resize" drag={repeatEnd!} onClick={toggleRepeat} />
      <Rect x={loop.startX.min(loop.x)} y={RULER - 10} width={loop.dx.abs()} height={8} radius={2} color="gray-900" opacity={loop.active.times(loop.dx.abs().gt(0))} />
      <For each={tempos}>
        {(change) => {
          const text = `${bpmText(change.bpm)} bpm`;
          const shown = () => (tempoChosen(change.tick) ? 1 : now.hover(`ruler-tempo-${change.tick}`));
          return [
            <Rect x={timeline.pointX.times(6).plus(change.tick)} y={LINE} width={timeline.pointX.times(text.length * 5.6 + 12)} height={18} radius={4} color="gray-100" border="alpha/10" borderWidth={1} opacity={shown} />,
            <Label x={timeline.pointX.times(12).plus(change.tick)} y={LINE + 3} text={text} size={12} color="gray-950" opacity={shown} />,
          ];
        }}
      </For>
      <Line from={[playhead, LINE + 8]} to={[playhead, RULER - 1]} color="gray-950" width={1} />
      <Circle x={playhead} y={LINE + 8} radius={3.5} color="gray-950" />
    </Space>
  );
};

/** Every other part of the song from the second, the bars before the first section a part too: a shade lighter, `height` tall. */
export const Shades = ({ page: { clock, starts, end }, height }: { page: Page; height: number }) => {
  const shaded = memo(() => {
    const edges = clock.sections.map((each) => starts()[each.bar - 1] ?? end());
    if (edges[0] !== 0) edges.unshift(0);
    edges.push(end());
    return edges.slice(1, -1).flatMap((from, index) => (index % 2 === 0 ? [{ from, to: edges[index + 2]! }] : []));
  });
  return <For each={shaded}>{(part) => <Rect x={part.from} y={0} width={part.to - part.from} height={height} color="alpha/4" />}</For>;
};

/** The field to name the section being named, over the timeline. */
export const SectionName = ({ page }: { page: Page }) => {
  const { memory, clock } = page;
  const commitEdit = (value: string) => {
    const editing = memory.editing;
    memory.editing = null;
    if (!editing) return;
    clock.update("Name a section", (song) => {
      const rest = song.sections.filter((each) => each.bar !== editing.bar);
      song.sections = value.trim() === "" ? rest : [...rest, { bar: editing.bar, name: value.trim() }].sort((a, b) => a.bar - b.bar);
    });
  };
  const editedValue = () => {
    const editing = memory.editing;
    return editing ? (clock.sections.find((each) => each.bar === editing.bar)?.name ?? "") : "";
  };
  return (
    <>
      {() => {
        const editing = memory.editing;
        if (!editing) return null;
        const label = `Section at bar ${editing.bar}`;
        return (
          <div key="ruler-edit" style={{ direction: "row", gap: 6, align: "center" }}>
            <div style={{ fontSize: 11, color: COLOR.text }}>{label}</div>
            <Input key="ruler-edit-field" value={editedValue} width={100} focus onCommit={commitEdit} />
          </div>
        );
      }}
    </>
  );
};

/**
 * T: a tempo change at the playhead with the tempo that plays there, chosen. While it plays, on
 * the grid; stopped, where a click on the ruler put it.
 */
export function addTempoAtPlayhead(clock: Clock, memory: { snap: number; mark: Mark | null }) {
  addTempo(clock, memory, clock.playing && memory.snap > 0 ? Math.round(clock.tick / memory.snap) * memory.snap : Math.round(clock.tick));
}

/** A tempo change at `tick` with the tempo that plays there, chosen; one there already is chosen. */
function addTempo(clock: Clock, memory: { mark: Mark | null }, tick: number) {
  const changes = clock.tempo_map.tempo_changes;
  if (!changes.some((change) => change.tick === tick)) {
    const bpm = [...changes].reverse().find((change) => change.tick <= tick)?.bpm ?? 120;
    clock.update("Add a tempo change", (song) => (song.tempo_map.tempo_changes = [...song.tempo_map.tempo_changes, { tick, bpm }].sort((a, b) => a.tick - b.tick)));
  }
  const starts = barStarts(clock.tempo_map.time_signatures, tick + 1);
  memory.mark = { kind: "tempo", bar: Math.max(starts.findIndex((start) => start > tick), 1), tick };
}

/**
 * A change at bar `bar` to the time signature that plays there, so nothing moves until the
 * transport changes it: the run it is in, cut in two. None where one starts already.
 */
function addSignature(clock: Clock, bar: number) {
  const runs = clock.tempo_map.time_signatures;
  if (changeBars(runs).includes(bar)) return;
  clock.update("Add a time signature change", (song) => {
    const all = song.tempo_map.time_signatures.length > 0 ? song.tempo_map.time_signatures : [{ signature: "4/4", bars: 1 }];
    const firsts = changeBars(all);
    const index = firsts.findLastIndex((first) => first <= bar);
    const [run, first] = [all[index]!, firsts[index]!];
    song.tempo_map.time_signatures = [
      ...all.slice(0, index),
      { signature: run.signature, bars: bar - first },
      // The last run goes on, whatever its count says.
      { signature: run.signature, bars: Math.max(run.bars - (bar - first), 1) },
      ...all.slice(index + 1),
    ];
  });
}

/** Removes a mark of the ruler as one undo step: the bars of a time signature change join the run before. */
export function removeMark(clock: Clock, { kind, bar, tick }: Mark) {
  if (kind === "section") clock.update("Remove a section", (song) => (song.sections = song.sections.filter((each) => each.bar !== bar)));
  else if (kind === "tempo") clock.update("Remove a tempo change", (song) => (song.tempo_map.tempo_changes = song.tempo_map.tempo_changes.filter((each) => each.tick !== tick)));
  else {
    clock.update("Remove a time signature change", (song) => {
      const runs = song.tempo_map.time_signatures;
      const index = changeBars(runs).indexOf(bar);
      const [before, run] = [runs[index - 1], runs[index]];
      if (!before || !run) return;
      song.tempo_map.time_signatures = [...runs.slice(0, index - 1), { ...before, bars: before.bars + run.bars }, ...runs.slice(index + 1)];
    });
  }
}
