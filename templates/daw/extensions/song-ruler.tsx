import { Circle, For, Fragment, h, Input, Label, Line, memo, now, Rect, Space, type Clock, type Color, type Drag, type Press, type TextPart } from "./sdk";
import { ACROSS, barStarts, BEAT, COLOR, LEAD, RULER, type Page, type Runs } from "./song-page";

// The ruler: bars, sections, tempo changes and time signatures; under it the repeat range.

/** What starts at a bar of the ruler: a section, a time signature change or a tempo change, at `tick`. */
export type Mark = { kind: "section" | "signature" | "tempo"; bar: number; tick: number };

/** The signature of each of `count` bars. */
function signaturesOf(runs: Runs, count: number): string[] {
  const each: string[] = [];
  runs.forEach((run, index) => {
    const bars = index === runs.length - 1 ? Math.max(count - each.length, 0) : run.bars;
    for (let bar = 0; bar < bars; bar++) each.push(run.signature);
  });
  return each.slice(0, count);
}
/** The bar each run starts at, from 1: where a time signature change is, even to the same one. */
function changeBars(runs: Runs): number[] {
  const bars = [1];
  for (const run of runs.slice(0, -1)) bars.push(bars.at(-1)! + run.bars);
  return bars;
}

export const Ruler = ({ page }: { page: Page }) => {
  const { memory, clock, openMenu, runs, starts, end, choose, timeline, playhead, handle } = page;
  const loop = now.drag("loop");
  // The ruler's label of bar `index`: its number, with the time signature where one starts.
  const barSignatures = memo(() => signaturesOf(runs(), starts().length));
  /** A bar's number, its time signature where it changes, and the name of a section that starts there. */
  const rulerLabel = (index: number): TextPart[] => {
    const here = barSignatures()[index];
    const section = clock.sections.find((each) => each.bar === index + 1);
    const changes = changeBars(runs()).includes(index + 1);
    const tempo = clock.tempo_map.tempo_changes.find((change) => change.tick > 0 && change.tick === starts()[index]);
    // The chosen mark in a light box.
    const chosen = (kind: Mark["kind"]) => (memory.mark?.kind === kind && memory.mark.bar === index + 1 ? ("gray-800" as const) : undefined);
    return [
      { text: String(index + 1) },
      ...(changes ? [{ text: "  " }, { text: here ?? "4/4", border: chosen("signature") }] : []),
      ...(section ? [{ text: "   " }, { text: section.name, color: "gray-950" as const, weight: "medium" as const, border: chosen("section") }] : []),
      ...(tempo ? tempoParts(tempo.bpm, chosen("tempo")) : []),
    ];
  };
  /** The mark a press on the label of bar `index` takes: its section, then its time signature change, then its tempo change, as the arrangement's did. */
  const markAt = (index: number): Mark | null => {
    const [bar, tick] = [index + 1, starts()[index] ?? 0];
    if (clock.sections.some((each) => each.bar === bar)) return { kind: "section", bar, tick };
    if (index > 0 && changeBars(runs()).includes(bar)) return { kind: "signature", bar, tick };
    if (tick > 0 && clock.tempo_map.tempo_changes.some((each) => each.tick === tick)) return { kind: "tempo", bar, tick };
    return null;
  };
  /** A press on a mark chooses it and plays from it; a double click on a section names it. */
  const pressMark = (mark: Mark) => ({ clicks }: Press) => {
    choose(new Set());
    memory.mark = mark;
    clock.jump(mark.tick);
    if (clicks === 2 && mark.kind === "section") memory.editing = mark;
  };
  /** About how many points the label of bar `index` takes, 8 after its line, its box too. */
  const labelWidth = (index: number) => 8 + rulerLabel(index).reduce((width, part) => width + part.text.length * 5.6 + (part.border ? 2.5 : 0), 0);
  /** 1 while the label of the bar before leaves room for the label of bar `index`, which hides under it, as the arrangement's did. */
  const roomFor = (index: number) => (index === 0 ? 1 : timeline.pointX.times(labelWidth(index - 1) + 4).plus(starts()[index - 1] ?? 0).lt(starts()[index] ?? 0));
  // A tempo change in a box after what its bar shows, as the arrangement had it.
  const tempoParts = (bpm: number, chosen?: Color): TextPart[] => [{ text: "   " }, { text: String(bpm), color: "gray-950", border: chosen ?? "alpha/10" }, { text: " bpm", border: chosen ?? "alpha/10" }];
  /** The tick nearest `tick` on the grid of the snap in the corner. */
  const snapped = (tick: number) => Math.max(memory.snap > 0 ? Math.round(tick / memory.snap) * memory.snap : Math.round(tick), 0);
  /** The bar whose line is nearest `tick`, from 1. */
  const nearestBar = (tick: number) => {
    const lines = starts();
    return lines.reduce((best, start, index) => (Math.abs(start - tick) < Math.abs((lines[best] ?? 0) - tick) ? index : best), 0) + 1;
  };
  /** A section at the bar line nearest `tick`, named by how many there are, its name open to change. */
  const addSection = (tick: number) => {
    const bar = nearestBar(tick);
    if (!clock.sections.some((each) => each.bar === bar)) {
      clock.update("Add a section", (song) => {
        song.sections = [...song.sections, { bar, name: `Section ${song.sections.length + 1}` }].sort((a, b) => a.bar - b.bar);
      });
    }
    memory.editing = { bar, tick: starts()[bar - 1] ?? 0 };
  };
  // As the arrangement's: a click plays from the grid, a double click adds a section. The menu
  // adds a tempo change on the grid, and a time signature or a section on the nearest bar line;
  // it removes the mark there.
  const pressRuler = async ({ button, x, clicks }: Press) => {
    if (button !== "right") {
      memory.mark = null;
      if (clicks === 2) addSection(x);
      else clock.jump(snapped(x));
      return;
    }
    const [tick, bar] = [snapped(x), nearestBar(x)];
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
    } else if (picked === 2) addSection(line);
    else if (picked === 3) {
      const mark = section ? { kind: "section" as const, bar, tick: line } : signature ? { kind: "signature" as const, bar, tick: line } : tempo ? { kind: "tempo" as const, bar, tick: tempo.tick } : null;
      if (mark) removeMark(clock, mark);
    }
  };
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
      onPress={pressRuler} drag={loopDrag}>
      <Shades page={page} height={RULER - 1} />
      <For each={starts}>
        {(tick, bar) => [
          <Rect x={tick} y={RULER - 8} width={timeline.pointX} height={8} color="alpha/10" />,
          <Label x={timeline.pointX.times(8).plus(tick)} y={8} text={() => rulerLabel(bar())} size={12} color="gray-700" opacity={() => roomFor(bar())} />,
          () => {
            const mark = markAt(bar());
            return mark ? <Rect x={timeline.pointX.times(4).plus(tick)} y={2} width={() => timeline.pointX.times(labelWidth(bar()))} height={RULER - 16} color={COLOR.clear} cursor="pointer" onPress={pressMark(mark)} /> : null;
          },
        ]}
      </For>
      <For each={() => clock.tempo_map.tempo_changes.filter((change) => change.tick > 0)}>
        {(change) => [
          <Rect x={() => change.tick} y={0} width={timeline.pointX} height={RULER - 1} color="alpha/20" />,
          // One off a bar line is labelled here; one on it, after its bar.
          () => (starts().includes(change.tick) ? null : <Label x={timeline.pointX.times(2).plus(change.tick)} y={8} text={tempoParts(change.bpm)} size={12} color="gray-700" />),
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
      <Line from={[playhead, RULER / 2]} to={[playhead, RULER - 1]} color="gray-950" width={1} />
      <Circle x={playhead} y={RULER / 2} radius={3.5} color="gray-950" />
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
